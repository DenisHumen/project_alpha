// SPDX-License-Identifier: MPL-2.0
//! Integration test against the real OS keychain: macOS Keychain, Windows
//! Credential Manager, Linux Secret Service.
//!
//! Runs only with `PA_SECRETS_NATIVE_TEST=1`, and then fails if the keychain
//! does not work. Without it the test prints a notice and passes: on a
//! developer machine a locked login keyring would show an unlock prompt and
//! the test would hang. CI sets the variable on macOS and Windows, and on
//! Linux runs the test under `dbus-run-session` with a throwaway unlocked
//! gnome-keyring (see docs/dev/secrets.md).

#![cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
))]

use pa_secrets::{Backend, NativeStore, OpenOptions, SecretKey, SecretStore, SecretString, open};

fn native_test_enabled() -> bool {
    std::env::var("PA_SECRETS_NATIVE_TEST").is_ok_and(|v| v == "1")
}

/// A service name of its own per run, so the test never touches the app's
/// real items and parallel runs do not collide.
fn test_service() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("pa-secrets-test-{}-{nanos}", std::process::id())
}

#[test]
fn os_keychain_round_trip() {
    if !native_test_enabled() {
        eprintln!("skipped: set PA_SECRETS_NATIVE_TEST=1 to test the OS keychain");
        return;
    }
    let service = test_service();
    let store = NativeStore::open(&service).expect("OS keychain must work");
    let expected = if cfg!(target_os = "macos") {
        Backend::Keychain
    } else if cfg!(windows) {
        Backend::CredentialManager
    } else {
        Backend::SecretService
    };
    assert_eq!(store.backend(), expected);
    assert!(store.backend().is_secure());

    let password = SecretKey::source_password("it-nas").unwrap();
    let tokens = SecretKey::twitch_tokens("it-123").unwrap();

    // Clean up even if an assertion fails.
    struct Cleanup<'a>(&'a NativeStore, [&'a SecretKey; 2]);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            for key in self.1 {
                let _ = self.0.delete(key);
            }
        }
    }
    let _cleanup = Cleanup(&store, [&password, &tokens]);

    assert_eq!(store.get(&password).unwrap(), None);
    store.set(&password, &"первый пароль 1".into()).unwrap();
    assert_eq!(
        store.get(&password).unwrap().unwrap().expose(),
        "первый пароль 1"
    );
    store.set(&password, &"second password".into()).unwrap();
    assert_eq!(
        store.get(&password).unwrap().unwrap().expose(),
        "second password"
    );

    let doc = SecretString::new(format!(
        r#"{{"access_token":"{}","refresh_token":"{}"}}"#,
        "a".repeat(300),
        "r".repeat(300)
    ));
    store.set(&tokens, &doc).unwrap();
    assert_eq!(store.get(&tokens).unwrap().unwrap(), doc);

    // A second handle sees the same items (they live in the OS, not in us).
    let again = NativeStore::open(&service).unwrap();
    assert_eq!(again.get(&tokens).unwrap().unwrap(), doc);

    assert!(store.delete(&password).unwrap());
    assert!(!store.delete(&password).unwrap());
    assert_eq!(store.get(&password).unwrap(), None);
    assert!(store.delete(&tokens).unwrap());

    // `open` picks the keychain and does not create a fallback file.
    let tmp = tempfile::tempdir().unwrap();
    let opened = open(OpenOptions {
        service: service.clone(),
        ..OpenOptions::new(Some(tmp.path().to_path_buf()))
    })
    .unwrap();
    assert_eq!(opened.backend(), expected);
    assert_eq!(opened.fallback_reason, None);
    assert!(!tmp.path().join(pa_secrets::SECRETS_FILE).exists());
}
