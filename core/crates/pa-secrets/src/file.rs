// SPDX-License-Identifier: MPL-2.0
//! Fallback store: one file encrypted with a key bound to this machine.
//!
//! Used only when the OS has no keychain. Layout of [`SECRETS_FILE`]:
//!
//! ```text
//! magic "PASECv1\n" (8) | salt (16) | nonce (24) | XChaCha20-Poly1305(JSON map)
//! ```
//!
//! The encryption key is `HKDF-SHA256(salt, KEY_FILE bytes ‖ machine id)`.
//! [`KEY_FILE`] holds 32 random bytes made on first use; the machine id is
//! `/etc/machine-id` on Linux and BSD (empty elsewhere). Both files are
//! created with mode 0600 on Unix and replaced atomically.
//!
//! This is obfuscation, not a keychain: it keeps secrets out of plain text
//! in backups, sync folders and `grep`, and makes a copied data directory
//! useless on another machine, but any program running as the same user can
//! read the key file. [`open`](crate::open) reports the fallback so the app
//! can warn the user.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{Backend, Result, SecretError, SecretKey, SecretStore, SecretString};

/// Name of the encrypted secrets file inside the store directory.
pub const SECRETS_FILE: &str = "secrets.enc";
/// Name of the random key file inside the store directory.
pub const KEY_FILE: &str = "secrets.key";

const MAGIC: &[u8; 8] = b"PASECv1\n";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const HKDF_INFO: &[u8] = b"project_alpha pa-secrets file v1";

type SecretMap = BTreeMap<String, SecretString>;

/// Secrets in an encrypted file. See the module docs for what it protects
/// against and what not.
pub struct EncryptedFileStore {
    dir: PathBuf,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; KEY_LEN]>,
    map: Mutex<SecretMap>,
}

impl EncryptedFileStore {
    /// Opens the store in `dir` (created if missing), reading
    /// [`SECRETS_FILE`] if it exists.
    ///
    /// Fails with [`SecretError::Corrupted`] if the file exists but cannot
    /// be decrypted (damaged, or the key file or machine id changed).
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_machine_id(dir, &machine_id())
    }

    /// Like [`open`](Self::open) with an explicit machine id (tests).
    pub fn open_with_machine_id(dir: impl Into<PathBuf>, machine_id: &[u8]) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let key_file = load_or_create_key_file(&dir.join(KEY_FILE))?;
        let path = dir.join(SECRETS_FILE);
        let existing = match fs::read(&path) {
            Ok(bytes) => Some(Zeroizing::new(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };

        let (salt, map) = match existing {
            Some(bytes) => {
                let salt = parse_salt(&bytes)?;
                let key = derive_key(&salt, &key_file, machine_id);
                (salt, decrypt(&bytes, &key)?)
            }
            None => (random_array()?, SecretMap::new()),
        };
        let key = derive_key(&salt, &key_file, machine_id);
        Ok(Self {
            dir,
            salt,
            key,
            map: Mutex::new(map),
        })
    }

    /// Whether `dir` holds a secrets file.
    pub fn exists(dir: &Path) -> bool {
        dir.join(SECRETS_FILE).is_file()
    }

    /// Deletes the secrets file and the key file in `dir`. Missing files are
    /// not an error.
    pub fn remove_files(dir: &Path) -> Result<()> {
        for name in [SECRETS_FILE, KEY_FILE] {
            match fs::remove_file(dir.join(name)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Keys of all stored secrets, sorted. Keys that are not valid
    /// [`SecretKey`]s (written by a newer core) are skipped.
    pub fn keys(&self) -> Vec<SecretKey> {
        self.lock()
            .keys()
            .filter_map(|k| SecretKey::new(k.as_str()).ok())
            .collect()
    }

    /// Directory of the store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SecretMap> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn persist(&self, map: &SecretMap) -> Result<()> {
        let plain: BTreeMap<&str, &str> =
            map.iter().map(|(k, v)| (k.as_str(), v.expose())).collect();
        let plain = Zeroizing::new(
            serde_json::to_vec(&plain).map_err(|e| SecretError::Backend(e.to_string()))?,
        );
        let nonce: [u8; NONCE_LEN] = random_array()?;
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_ref())
            .map_err(|_| SecretError::Backend("bad key length".into()))?;
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &plain,
                    aad: MAGIC,
                },
            )
            .map_err(|_| SecretError::Backend("encryption failed".into()))?;
        let mut out = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        write_atomic(&self.dir.join(SECRETS_FILE), &out)
    }
}

impl std::fmt::Debug for EncryptedFileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedFileStore")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl SecretStore for EncryptedFileStore {
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>> {
        Ok(self.lock().get(key.as_str()).cloned())
    }

    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()> {
        value.check_len()?;
        let mut map = self.lock();
        let previous = map.insert(key.as_str().to_owned(), value.clone());
        if let Err(e) = self.persist(&map) {
            match previous {
                Some(old) => map.insert(key.as_str().to_owned(), old),
                None => map.remove(key.as_str()),
            };
            return Err(e);
        }
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<bool> {
        let mut map = self.lock();
        let Some(old) = map.remove(key.as_str()) else {
            return Ok(false);
        };
        if let Err(e) = self.persist(&map) {
            map.insert(key.as_str().to_owned(), old);
            return Err(e);
        }
        Ok(true)
    }

    fn backend(&self) -> Backend {
        Backend::EncryptedFile
    }
}

fn parse_salt(bytes: &[u8]) -> Result<[u8; SALT_LEN]> {
    if bytes.len() < MAGIC.len() + SALT_LEN + NONCE_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SecretError::Corrupted);
    }
    let mut salt = [0; SALT_LEN];
    salt.copy_from_slice(&bytes[MAGIC.len()..MAGIC.len() + SALT_LEN]);
    Ok(salt)
}

fn decrypt(bytes: &[u8], key: &[u8; KEY_LEN]) -> Result<SecretMap> {
    let body = &bytes[MAGIC.len() + SALT_LEN..];
    let (nonce, ciphertext) = body.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SecretError::Corrupted)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| SecretError::Corrupted)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: MAGIC,
                },
            )
            .map_err(|_| SecretError::Corrupted)?,
    );
    let map: BTreeMap<String, String> =
        serde_json::from_slice(&plain).map_err(|_| SecretError::Corrupted)?;
    Ok(map
        .into_iter()
        .map(|(k, v)| (k, SecretString::new(v)))
        .collect())
}

fn derive_key(salt: &[u8], key_file: &[u8], machine_id: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
    let mut ikm = Zeroizing::new(Vec::with_capacity(key_file.len() + machine_id.len()));
    ikm.extend_from_slice(key_file);
    ikm.extend_from_slice(machine_id);
    let mut okm = Zeroizing::new([0u8; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(salt), &ikm)
        .expand(HKDF_INFO, okm.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

fn load_or_create_key_file(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    match File::open(path) {
        Ok(mut file) => {
            let mut bytes = Zeroizing::new(Vec::new());
            file.read_to_end(&mut bytes)?;
            if bytes.len() != KEY_LEN {
                return Err(SecretError::Corrupted);
            }
            Ok(bytes)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key: Zeroizing<[u8; KEY_LEN]> = Zeroizing::new(random_array()?);
            write_atomic(path, key.as_ref())?;
            Ok(Zeroizing::new(key.to_vec()))
        }
        Err(e) => Err(e.into()),
    }
}

/// Writes `tmp`, syncs it and renames it over `path`, mode 0600 on Unix.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf)
        .map_err(|e| SecretError::Backend(format!("no system randomness: {e}")))?;
    Ok(buf)
}

/// Machine id that binds the fallback file to this installation of the OS.
fn machine_id() -> Vec<u8> {
    #[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = fs::read_to_string(path) {
            let id = id.trim();
            if !id.is_empty() {
                return id.as_bytes().to_vec();
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> SecretKey {
        SecretKey::new(k).unwrap()
    }

    #[test]
    fn round_trip_and_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1").unwrap();
        assert_eq!(store.get(&key("a")).unwrap(), None);
        store.set(&key("a"), &"пароль 1".into()).unwrap();
        store.set(&key("b"), &"token".into()).unwrap();
        store.set(&key("a"), &"пароль 2".into()).unwrap();
        assert!(store.delete(&key("b")).unwrap());
        assert!(!store.delete(&key("b")).unwrap());
        drop(store);

        let store = EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1").unwrap();
        assert_eq!(store.get(&key("a")).unwrap().unwrap().expose(), "пароль 2");
        assert_eq!(store.get(&key("b")).unwrap(), None);
        assert_eq!(store.keys(), vec![key("a")]);
        assert_eq!(store.backend(), Backend::EncryptedFile);
    }

    #[test]
    fn file_does_not_contain_plain_text() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1").unwrap();
        store
            .set(&key("source.password:x"), &"VerySecretPassword".into())
            .unwrap();
        let bytes = fs::read(tmp.path().join(SECRETS_FILE)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("VerySecretPassword"));
        assert!(!text.contains("source.password"));
    }

    #[test]
    fn other_machine_or_tampering_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1").unwrap();
        store.set(&key("a"), &"x".into()).unwrap();
        drop(store);
        assert!(matches!(
            EncryptedFileStore::open_with_machine_id(tmp.path(), b"m2"),
            Err(SecretError::Corrupted)
        ));

        let path = tmp.path().join(SECRETS_FILE);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1"),
            Err(SecretError::Corrupted)
        ));

        fs::write(&path, b"garbage").unwrap();
        assert!(matches!(
            EncryptedFileStore::open_with_machine_id(tmp.path(), b"m1"),
            Err(SecretError::Corrupted)
        ));
    }

    #[test]
    fn remove_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::open(tmp.path()).unwrap();
        store.set(&key("a"), &"x".into()).unwrap();
        assert!(EncryptedFileStore::exists(tmp.path()));
        EncryptedFileStore::remove_files(tmp.path()).unwrap();
        assert!(!EncryptedFileStore::exists(tmp.path()));
        assert!(!tmp.path().join(KEY_FILE).exists());
        EncryptedFileStore::remove_files(tmp.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::open(tmp.path()).unwrap();
        store.set(&key("a"), &"x".into()).unwrap();
        for name in [SECRETS_FILE, KEY_FILE] {
            let mode = fs::metadata(tmp.path().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}
