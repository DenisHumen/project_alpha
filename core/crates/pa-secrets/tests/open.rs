// SPDX-License-Identifier: MPL-2.0
//! Backend selection, fallback and migration (no OS keychain involved).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pa_secrets::{
    Backend, EncryptedFileStore, OpenOptions, PlatformKeystore, SecretError, SecretKey,
    SecretString, open, redact,
};

/// In-memory stand-in for the Android Keystore / iOS Keychain callback.
#[derive(Default)]
struct FakeKeystore(Mutex<HashMap<String, String>>);

impl PlatformKeystore for FakeKeystore {
    fn load(&self, key: &str) -> pa_secrets::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn save(&self, key: &str, value: &str) -> pa_secrets::Result<()> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn remove(&self, key: &str) -> pa_secrets::Result<bool> {
        Ok(self.0.lock().unwrap().remove(key).is_some())
    }
}

fn no_native(dir: &std::path::Path) -> OpenOptions {
    OpenOptions {
        native: false,
        allow_fallback: true,
        ..OpenOptions::new(Some(dir.to_path_buf()))
    }
}

#[test]
fn platform_callback_is_preferred() {
    let tmp = tempfile::tempdir().unwrap();
    let keystore = Arc::new(FakeKeystore::default());
    let opened = open(no_native(tmp.path()).with_platform(keystore.clone())).unwrap();
    assert_eq!(opened.backend(), Backend::Platform);
    assert!(opened.backend().is_secure());
    assert_eq!(opened.fallback_reason, None);

    let key = SecretKey::source_password("nas-1").unwrap();
    opened.store.set(&key, &"Pl@tformPass".into()).unwrap();
    assert_eq!(
        keystore.0.lock().unwrap().get("source.password:nas-1").unwrap(),
        "Pl@tformPass"
    );
    assert_eq!(
        opened.store.get(&key).unwrap().unwrap().expose(),
        "Pl@tformPass"
    );
    // Values that went through the store are masked in logs.
    assert_eq!(redact("got Pl@tformPass"), "got ***");
    assert!(opened.store.delete(&key).unwrap());
    assert!(!opened.store.delete(&key).unwrap());
    assert_eq!(opened.store.get(&key).unwrap(), None);
    assert!(!tmp.path().join(pa_secrets::SECRETS_FILE).exists());
}

#[test]
fn falls_back_to_encrypted_file_with_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let opened = open(no_native(tmp.path())).unwrap();
    assert_eq!(opened.backend(), Backend::EncryptedFile);
    assert!(!opened.backend().is_secure());
    assert!(opened.fallback_reason.is_some());

    let key = SecretKey::twitch_tokens("42").unwrap();
    let tokens = SecretString::new(r#"{"access":"a1b2c3d4","refresh":"r1r2r3r4"}"#);
    opened.store.set(&key, &tokens).unwrap();
    drop(opened);

    let opened = open(no_native(tmp.path())).unwrap();
    assert_eq!(opened.store.get(&key).unwrap().unwrap(), tokens);
}

#[test]
fn fallback_can_be_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let options = OpenOptions {
        allow_fallback: false,
        ..no_native(tmp.path())
    };
    assert!(matches!(open(options), Err(SecretError::Unavailable(_))));

    let options = OpenOptions {
        fallback_dir: None,
        ..no_native(tmp.path())
    };
    assert!(matches!(open(options), Err(SecretError::Unavailable(_))));
}

#[test]
fn fallback_secrets_move_to_secure_backend() {
    let tmp = tempfile::tempdir().unwrap();
    let a = SecretKey::source_password("a").unwrap();
    let b = SecretKey::tmdb_api_key();
    {
        let file = EncryptedFileStore::open(tmp.path()).unwrap();
        use pa_secrets::SecretStore;
        file.set(&a, &"from-file-a".into()).unwrap();
        file.set(&b, &"from-file-b".into()).unwrap();
    }

    let keystore = Arc::new(FakeKeystore::default());
    // A value already in the secure store is newer and wins.
    keystore.save(b.as_str(), "already-there").unwrap();

    let opened = open(no_native(tmp.path()).with_platform(keystore.clone())).unwrap();
    assert_eq!(opened.backend(), Backend::Platform);
    assert_eq!(opened.migrated, 1);
    assert_eq!(opened.store.get(&a).unwrap().unwrap().expose(), "from-file-a");
    assert_eq!(opened.store.get(&b).unwrap().unwrap().expose(), "already-there");
    assert!(!EncryptedFileStore::exists(tmp.path()));
    assert!(!tmp.path().join(pa_secrets::KEY_FILE).exists());
}

#[test]
fn corrupted_fallback_is_set_aside() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(pa_secrets::SECRETS_FILE), b"not ours").unwrap();
    let opened = open(no_native(tmp.path())).unwrap();
    assert!(opened.discarded_corrupted);
    assert!(tmp.path().join("secrets.enc.corrupt").exists());
    let key = SecretKey::new("k").unwrap();
    opened.store.set(&key, &"v".into()).unwrap();
    assert_eq!(opened.store.get(&key).unwrap().unwrap().expose(), "v");
}

#[test]
fn too_large_values_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let opened = open(no_native(tmp.path())).unwrap();
    let big = SecretString::new("x".repeat(pa_secrets::MAX_SECRET_LEN + 1));
    assert!(matches!(
        opened.store.set(&SecretKey::new("k").unwrap(), &big),
        Err(SecretError::TooLarge { .. })
    ));
}
