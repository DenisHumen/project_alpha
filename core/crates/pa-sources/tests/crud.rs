// SPDX-License-Identifier: MPL-2.0
//! CRUD of sources (NET-011 "done when": CRUD tests).

use std::sync::Arc;

use pa_secrets::{Backend, MemoryStore, SecretError, SecretKey, SecretStore, SecretString};
use pa_sources::{
    Error, NewSource, PasswordChange, SmbEncryption, SmbVersion, SourceKind, SourceManager,
    SourceOptions, SourceUpdate,
};

fn manager() -> (SourceManager, Arc<MemoryStore>) {
    let secrets = Arc::new(MemoryStore::new());
    let m = SourceManager::new(pa_library::open_in_memory().unwrap(), secrets.clone());
    (m, secrets)
}

fn secret(store: &MemoryStore, key: &SecretKey) -> Option<String> {
    store.get(key).unwrap().map(|s| s.expose().to_owned())
}

fn only_key(store: &MemoryStore) -> SecretKey {
    let keys = store.keys();
    assert_eq!(keys.len(), 1, "{keys:?}");
    keys[0].clone()
}

#[test]
fn create_get_list() {
    let (m, secrets) = manager();
    let local = m
        .create(NewSource::new(SourceKind::Local, "Films", "/mnt/My Films"))
        .unwrap();
    assert_eq!(local.kind, SourceKind::Local);
    assert_eq!(local.uri, "file:///mnt/My%20Films");
    assert!(!local.has_password && local.username.is_none());
    assert!(local.scan_into_library && local.auto_scan && local.enabled);
    assert_eq!(local.status, Default::default());

    let options = SourceOptions {
        smb_domain: Some("WORKGROUP".into()),
        smb_version: SmbVersion::Smb3,
        smb_signing_required: true,
        smb_encryption: SmbEncryption::Required,
        ..Default::default()
    };
    let mut new = NewSource::new(SourceKind::Smb, " NAS ", "smb://nas.local/media/Movies/")
        .with_login("anna", SecretString::new("hunter2-smb"))
        .with_options(options.clone());
    new.auto_scan = false;
    let smb = m.create(new).unwrap();
    assert_eq!(smb.name, "NAS");
    assert_eq!(smb.uri, "smb://nas.local/media/Movies");
    assert_eq!(smb.username.as_deref(), Some("anna"));
    assert!(smb.has_password);
    assert_eq!(smb.options, options);
    assert!(!smb.auto_scan && !smb.wants_auto_scan());
    assert_eq!(
        secret(&secrets, &only_key(&secrets)).as_deref(),
        Some("hunter2-smb")
    );

    let dav = m
        .create(NewSource::new(
            SourceKind::WebDav,
            "Cloud",
            "davs://cloud.example/dav/",
        ))
        .unwrap();
    assert_eq!(dav.uri, "https://cloud.example/dav/");

    assert_eq!(m.get(smb.id).unwrap(), smb);
    let list = m.list().unwrap();
    assert_eq!(
        list.iter().map(|s| s.id).collect::<Vec<_>>(),
        [local.id, smb.id, dav.id]
    );
    assert!(list[0].position < list[1].position && list[1].position < list[2].position);
    assert_eq!(
        m.auto_scan_sources()
            .unwrap()
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        [local.id, dav.id]
    );
}

#[test]
fn update_settings_and_password() {
    let (m, secrets) = manager();
    let s = m
        .create(
            NewSource::new(SourceKind::WebDav, "Cloud", "https://cloud.example/dav/")
                .with_login("anna", SecretString::new("first-pass")),
        )
        .unwrap();
    let first_key = only_key(&secrets);

    // Settings only: the password stays.
    let s = m
        .update(
            s.id,
            SourceUpdate {
                name: Some("Home cloud".into()),
                address: Some("webdavs://cloud.example/remote.php/dav/".into()),
                scan_into_library: Some(false),
                options: Some(SourceOptions {
                    timeout_secs: Some(15),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(s.name, "Home cloud");
    assert_eq!(s.uri, "https://cloud.example/remote.php/dav/");
    assert!(!s.scan_into_library && s.has_password);
    assert_eq!(s.options.timeout_secs, Some(15));
    assert!(s.updated_at_ms.is_some());
    assert_eq!(only_key(&secrets), first_key);

    // New password: new key, the old secret is gone.
    let s = m
        .update(
            s.id,
            SourceUpdate {
                password: PasswordChange::Set(SecretString::new("second-pass")),
                ..Default::default()
            },
        )
        .unwrap();
    let second_key = only_key(&secrets);
    assert_ne!(second_key, first_key);
    assert_eq!(
        secret(&secrets, &second_key).as_deref(),
        Some("second-pass")
    );
    assert!(s.has_password);

    // Anonymous: user and password removed.
    let s = m
        .update(
            s.id,
            SourceUpdate {
                username: Some(None),
                password: PasswordChange::Clear,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!s.has_password && s.username.is_none());
    assert!(secrets.keys().is_empty());
}

#[test]
fn delete_removes_row_and_secret() {
    let (m, secrets) = manager();
    let s = m
        .create(
            NewSource::new(SourceKind::Smb, "NAS", "smb://nas/media")
                .with_login("anna", SecretString::new("hunter2-smb")),
        )
        .unwrap();
    m.delete(s.id).unwrap();
    assert!(matches!(m.get(s.id), Err(Error::NotFound(id)) if id == s.id));
    assert!(m.list().unwrap().is_empty());
    assert!(secrets.keys().is_empty());
    assert!(matches!(m.delete(s.id), Err(Error::NotFound(_))));
    assert!(matches!(
        m.update(s.id, SourceUpdate::default()),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn invalid_input_is_refused() {
    let (m, secrets) = manager();
    let bad = [
        NewSource::new(SourceKind::Local, "", "/mnt"),
        NewSource::new(SourceKind::Local, "Rel", "relative/path"),
        NewSource::new(SourceKind::Local, "Login", "/mnt").with_login("a", "b".into()),
        NewSource::new(SourceKind::Smb, "No share", "smb://nas"),
        NewSource::new(SourceKind::WebDav, "Ftp", "ftp://nas/"),
        NewSource::new(SourceKind::WebDav, "Pin", "https://nas/").with_options(SourceOptions {
            tls_pin: Some("sha256:nope".into()),
            ..Default::default()
        }),
    ];
    for new in bad {
        let name = new.name.clone();
        assert!(
            matches!(m.create(new), Err(Error::Invalid(_))),
            "{name} accepted"
        );
    }
    assert!(m.list().unwrap().is_empty());
    assert!(secrets.keys().is_empty());

    let s = m
        .create(NewSource::new(SourceKind::Local, "Films", "/mnt"))
        .unwrap();
    assert!(matches!(
        m.update(
            s.id,
            SourceUpdate {
                password: PasswordChange::Set("x".into()),
                ..Default::default()
            }
        ),
        Err(Error::Invalid(_))
    ));
}

/// A keychain that refuses to store anything (locked, cancelled prompt).
#[derive(Debug)]
struct LockedStore;

impl SecretStore for LockedStore {
    fn get(&self, _: &SecretKey) -> pa_secrets::Result<Option<SecretString>> {
        Err(SecretError::Locked("test".into()))
    }
    fn set(&self, _: &SecretKey, _: &SecretString) -> pa_secrets::Result<()> {
        Err(SecretError::Locked("test".into()))
    }
    fn delete(&self, _: &SecretKey) -> pa_secrets::Result<bool> {
        Err(SecretError::Locked("test".into()))
    }
    fn backend(&self) -> Backend {
        Backend::Memory
    }
}

#[test]
fn failed_secret_store_adds_nothing() {
    let m = SourceManager::new(pa_library::open_in_memory().unwrap(), Arc::new(LockedStore));
    let result = m.create(
        NewSource::new(SourceKind::Smb, "NAS", "smb://nas/media")
            .with_login("anna", SecretString::new("hunter2-smb")),
    );
    assert!(matches!(result, Err(Error::Secrets(_))), "{result:?}");
    assert!(m.list().unwrap().is_empty());
    // Without a password the store is not needed.
    m.create(NewSource::new(SourceKind::Smb, "Guest", "smb://nas/public"))
        .unwrap();
}

#[test]
fn scan_status_is_recorded() {
    let (m, _) = manager();
    let s = m
        .create(NewSource::new(SourceKind::Local, "Films", "/mnt"))
        .unwrap();
    m.record_scan_started(s.id).unwrap();
    let status = m.get(s.id).unwrap().status;
    assert!(status.is_scanning() && status.last_scan_started_at_ms.is_some());
    m.record_scan_finished(s.id, Some("read failed: password=oops"))
        .unwrap();
    let status = m.get(s.id).unwrap().status;
    assert!(!status.is_scanning());
    assert_eq!(
        status.last_error.as_deref(),
        Some("read failed: password=***")
    );
    assert!(status.last_error_at_ms.is_some());
    assert!(matches!(
        m.record_scan_started(999),
        Err(Error::NotFound(999))
    ));
}
