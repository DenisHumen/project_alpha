// SPDX-License-Identifier: MPL-2.0
//! Secret storage through the exported FFI functions, with a platform
//! callback as Android and iOS use it.
//!
//! One test function on purpose: the store is a process-wide singleton.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pa_ffi::{
    CoreConfig, CoreDirs, PlatformSecretStore, SecretBackend, SecretStoreError, core_init,
    core_shutdown, redact_secrets, secret_delete, secret_get, secret_set, secrets_close,
    secrets_info, secrets_open,
};

#[derive(Default)]
struct FakeKeystore {
    items: Mutex<HashMap<String, String>>,
    locked: Mutex<bool>,
}

impl PlatformSecretStore for FakeKeystore {
    fn load(&self, key: String) -> Result<Option<String>, SecretStoreError> {
        if *self.locked.lock().unwrap() {
            return Err(SecretStoreError::Locked {
                details: "device is locked".into(),
            });
        }
        Ok(self.items.lock().unwrap().get(&key).cloned())
    }
    fn save(&self, key: String, value: String) -> Result<(), SecretStoreError> {
        self.items.lock().unwrap().insert(key, value);
        Ok(())
    }
    fn remove(&self, key: String) -> Result<bool, SecretStoreError> {
        Ok(self.items.lock().unwrap().remove(&key).is_some())
    }
}

#[test]
fn secrets_over_ffi() {
    let tmp = tempfile::tempdir().unwrap();
    let path = |p: &str| tmp.path().join(p).to_string_lossy().into_owned();
    core_init(
        CoreConfig {
            dirs: Some(CoreDirs {
                data: path("files"),
                config: path("files"),
                cache: path("cache"),
            }),
            worker_threads: Some(1),
            start_in_background: false,
        },
        None,
    )
    .unwrap();

    assert_eq!(secrets_info(), None);
    let keystore = Arc::new(FakeKeystore::default());
    let info = secrets_open(Some(keystore.clone())).unwrap();
    assert_eq!(info.backend, SecretBackend::Platform);
    assert!(info.secure);
    assert_eq!(info.fallback_reason, None);
    assert_eq!(secrets_info(), Some(info));

    let key = "source.password:nas-1".to_owned();
    assert_eq!(secret_get(key.clone()).unwrap(), None);
    secret_set(key.clone(), "SmbPassw0rd".into()).unwrap();
    assert_eq!(
        keystore.items.lock().unwrap().get(&key).map(String::as_str),
        Some("SmbPassw0rd")
    );
    assert_eq!(
        secret_get(key.clone()).unwrap().as_deref(),
        Some("SmbPassw0rd")
    );
    assert_eq!(
        redact_secrets("mount failed with SmbPassw0rd".into()),
        "mount failed with ***"
    );

    // Errors of the platform pass through with their kind.
    *keystore.locked.lock().unwrap() = true;
    assert!(matches!(
        secret_get(key.clone()),
        Err(SecretStoreError::Locked { .. })
    ));
    *keystore.locked.lock().unwrap() = false;

    assert!(matches!(
        secret_set("bad key".into(), "x".into()),
        Err(SecretStoreError::InvalidKey { .. })
    ));
    assert!(matches!(
        secret_set(key.clone(), "x".repeat(10_000)),
        Err(SecretStoreError::TooLarge { .. })
    ));

    assert!(secret_delete(key.clone()).unwrap());
    assert!(!secret_delete(key.clone()).unwrap());
    assert_eq!(secret_get(key).unwrap(), None);

    secrets_close();
    assert_eq!(secrets_info(), None);
    core_shutdown().unwrap();
}
