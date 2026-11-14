// SPDX-License-Identifier: MPL-2.0
//! The secret store (`pa-secrets`, CORE-007/CORE-015) for Dart.
//!
//! On Android (and iOS, once the app ships there) the core keeps secrets in
//! the secure storage of the platform: an Android Keystore key with AES-GCM
//! over app-private storage, the Keychain with `AfterFirstUnlock`. That code
//! is Kotlin/Swift, so Dart passes a callback to [`secrets_open_platform`]
//! at app start; the callback forwards each call over a platform channel
//! (package `pa_platform_secrets`). Desktop calls [`secrets_open_default`]
//! (OS keychain, encrypted fallback file) or nothing at all: the first
//! secret call, or [`CoreSources::open`](super::sources::CoreSources::open),
//! opens the default store.
//!
//! The callback is async on the Dart side while `pa-secrets` is blocking:
//! the core thread that needs a secret waits for the Dart reply (at most
//! [`PLATFORM_CALL_TIMEOUT`]). None of the functions here is
//! `#[frb(sync)]`: a secret call on the Dart thread would wait for itself.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, PoisonError, RwLock};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use flutter_rust_bridge::DartFnFuture;
use pa_secrets::{
    OpenOptions, OpenedStore, PlatformKeystore, SecretError, SecretKey, SecretString,
};

/// How long a core thread waits for the platform callback. The Keystore and
/// the Keychain answer in milliseconds; this only ends a wait for a Dart
/// isolate that is gone.
pub(crate) const PLATFORM_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Error of a secret call; Dart receives it as an exception. Never contains
/// a secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretsError {
    /// `invalid_key`, `too_large`, `unavailable`, `locked` or `failed`.
    pub code: String,
    /// Human-readable message for logs.
    pub message: String,
}

impl From<SecretError> for SecretsError {
    fn from(e: SecretError) -> Self {
        let code = match &e {
            SecretError::InvalidKey(_) => "invalid_key",
            SecretError::TooLarge { .. } => "too_large",
            SecretError::Unavailable(_) => "unavailable",
            SecretError::Locked(_) => "locked",
            _ => "failed",
        };
        Self {
            code: code.into(),
            message: e.to_string(),
        }
    }
}

type SecretsResult<T> = std::result::Result<T, SecretsError>;

/// One call of the core to the platform store.
#[derive(Clone, PartialEq, Eq)]
pub struct PlatformSecretCall {
    /// `load`, `save` or `remove`.
    pub op: String,
    /// Secret key, `[A-Za-z0-9._:@-]`, at most 128 bytes.
    pub key: String,
    /// The value to store (`save` only).
    pub value: Option<String>,
}

impl fmt::Debug for PlatformSecretCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformSecretCall")
            .field("op", &self.op)
            .field("key", &self.key)
            .field("value", &self.value.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Answer of the platform store to a [`PlatformSecretCall`].
#[derive(Clone, PartialEq, Eq, Default)]
pub struct PlatformSecretReply {
    /// `load`: the value, `None` if missing.
    pub value: Option<String>,
    /// `remove`: whether the value existed.
    pub existed: bool,
    /// `None` on success; else `unavailable`, `locked` or `failed`.
    pub error_code: Option<String>,
    /// Error details for logs (no secret values).
    pub error_message: Option<String>,
}

impl fmt::Debug for PlatformSecretReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformSecretReply")
            .field("value", &self.value.as_ref().map(|_| "***"))
            .field("existed", &self.existed)
            .field("error_code", &self.error_code)
            .field("error_message", &self.error_message)
            .finish()
    }
}

/// The opened secret store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSecretStoreInfo {
    /// `platform`, `keychain`, `secret-service`, `credential-manager`,
    /// `encrypted-file`…
    pub backend: String,
    /// Whether the backend is OS-protected storage.
    pub secure: bool,
    /// Why the encrypted fallback file is used (the app warns the user).
    pub fallback_reason: Option<String>,
    /// Secrets moved from an old fallback file into the secure backend.
    pub migrated: u32,
    /// An unreadable fallback file was set aside; its secrets must be
    /// entered again.
    pub discarded_corrupted: bool,
}

fn info(opened: &OpenedStore) -> CoreSecretStoreInfo {
    CoreSecretStoreInfo {
        backend: opened.backend().as_str().into(),
        secure: opened.backend().is_secure(),
        fallback_reason: opened.fallback_reason.clone(),
        migrated: u32::try_from(opened.migrated).unwrap_or(u32::MAX),
        discarded_corrupted: opened.discarded_corrupted,
    }
}

type PlatformFn =
    dyn Fn(PlatformSecretCall) -> DartFnFuture<PlatformSecretReply> + Send + Sync + 'static;

/// [`PlatformKeystore`] over the Dart callback.
struct DartKeystore {
    callback: Box<PlatformFn>,
    timeout: Duration,
}

impl DartKeystore {
    fn call(
        &self,
        op: &str,
        key: &str,
        value: Option<&str>,
    ) -> pa_secrets::Result<PlatformSecretReply> {
        let call = PlatformSecretCall {
            op: op.into(),
            key: key.into(),
            value: value.map(Into::into),
        };
        let reply = block_on_timeout((self.callback)(call), self.timeout).ok_or_else(|| {
            SecretError::Unavailable(format!(
                "the platform secret store did not answer `{op}` in {:?}",
                self.timeout
            ))
        })?;
        let Some(code) = reply.error_code.as_deref() else {
            return Ok(reply);
        };
        let message = reply.error_message.clone().unwrap_or_default();
        Err(match code {
            "unavailable" => SecretError::Unavailable(message),
            "locked" => SecretError::Locked(message),
            _ => SecretError::Backend(format!("{code}: {message}")),
        })
    }
}

impl PlatformKeystore for DartKeystore {
    fn load(&self, key: &str) -> pa_secrets::Result<Option<String>> {
        Ok(self.call("load", key, None)?.value)
    }

    fn save(&self, key: &str, value: &str) -> pa_secrets::Result<()> {
        self.call("save", key, Some(value)).map(drop)
    }

    fn remove(&self, key: &str) -> pa_secrets::Result<bool> {
        Ok(self.call("remove", key, None)?.existed)
    }
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls `future` on this thread until it is ready or `timeout` passes.
fn block_on_timeout<F: Future>(future: F, timeout: Duration) -> Option<F::Output> {
    let deadline = Instant::now() + timeout;
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    // Box::pin, not std::pin::pin!: its `super let` expansion breaks the
    // FRB codegen's parser.
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return Some(out);
        }
        let left = deadline.checked_duration_since(Instant::now())?;
        thread::park_timeout(left);
    }
}

static STORE: RwLock<Option<OpenedStore>> = RwLock::new(None);

fn set_store(opened: OpenedStore) -> CoreSecretStoreInfo {
    let result = info(&opened);
    *STORE.write().unwrap_or_else(PoisonError::into_inner) = Some(opened);
    result
}

/// The store opened by [`secrets_open_platform`] or [`secrets_open_default`].
pub(crate) fn opened_store() -> Option<OpenedStore> {
    STORE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn store() -> SecretsResult<Arc<dyn pa_secrets::SecretStore>> {
    opened_store().map(|o| o.store).ok_or_else(|| SecretsError {
        code: "unavailable".into(),
        message: "the secret store is not open: call secrets_open_platform or secrets_open_default"
            .into(),
    })
}

fn open_platform(
    keystore: DartKeystore,
    data_dir: Option<String>,
) -> SecretsResult<CoreSecretStoreInfo> {
    let options = OpenOptions::new(data_dir.map(Into::into)).with_platform(Arc::new(keystore));
    Ok(set_store(pa_secrets::open(options)?))
}

/// Opens the platform secret store (Android Keystore, iOS Keychain) and
/// makes it the store of the core, replacing the one opened before.
///
/// `callback` answers the [`PlatformSecretCall`]s of the core; the package
/// `pa_platform_secrets` provides it. `data_dir` is the core data directory:
/// secrets in an old encrypted fallback file there are moved into the
/// platform store. Call once at app start, before [`CoreSources::open`](super::sources::CoreSources::open).
pub fn secrets_open_platform(
    data_dir: Option<String>,
    callback: impl Fn(PlatformSecretCall) -> DartFnFuture<PlatformSecretReply> + Send + Sync + 'static,
) -> SecretsResult<CoreSecretStoreInfo> {
    open_platform(
        DartKeystore {
            callback: Box::new(callback),
            timeout: PLATFORM_CALL_TIMEOUT,
        },
        data_dir,
    )
}

/// Opens the default store of this platform: the OS keychain on desktop,
/// else the encrypted fallback file in `data_dir`. Fails with `unavailable`
/// on mobile (use [`secrets_open_platform`] there).
pub fn secrets_open_default(data_dir: Option<String>) -> SecretsResult<CoreSecretStoreInfo> {
    Ok(set_store(pa_secrets::open(OpenOptions::new(
        data_dir.map(Into::into),
    ))?))
}

/// The opened store, `None` before an open call.
pub fn secrets_info() -> Option<CoreSecretStoreInfo> {
    opened_store().as_ref().map(info)
}

/// Forgets the opened store (the platform callback is released).
pub fn secrets_close() {
    *STORE.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The secret, `None` if it is not set.
pub fn secret_get(key: String) -> SecretsResult<Option<String>> {
    let value = store()?.get(&SecretKey::new(key)?)?;
    Ok(value.map(|v| v.expose().to_owned()))
}

/// Stores or replaces a secret.
pub fn secret_set(key: String, value: String) -> SecretsResult<()> {
    Ok(store()?.set(&SecretKey::new(key)?, &SecretString::new(value))?)
}

/// Deletes a secret; returns whether it existed.
pub fn secret_delete(key: String) -> SecretsResult<bool> {
    Ok(store()?.delete(&SecretKey::new(key)?)?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// A platform store in a map that answers from another thread, as the
    /// Dart isolate does.
    fn map_keystore(timeout: Duration) -> (DartKeystore, Arc<Mutex<HashMap<String, String>>>) {
        let map = Arc::new(Mutex::new(HashMap::new()));
        let shared = Arc::clone(&map);
        let callback = move |call: PlatformSecretCall| -> DartFnFuture<PlatformSecretReply> {
            let map = Arc::clone(&shared);
            let (tx, rx) = std::sync::mpsc::channel();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(5));
                let mut map = map.lock().unwrap();
                let reply = match call.op.as_str() {
                    "load" => PlatformSecretReply {
                        value: map.get(&call.key).cloned(),
                        ..Default::default()
                    },
                    "save" if call.key == "fail.locked" => PlatformSecretReply {
                        error_code: Some("locked".into()),
                        error_message: Some("device locked".into()),
                        ..Default::default()
                    },
                    "save" if call.key == "never.answer" => return,
                    "save" => {
                        map.insert(call.key, call.value.unwrap());
                        PlatformSecretReply::default()
                    }
                    "remove" => PlatformSecretReply {
                        existed: map.remove(&call.key).is_some(),
                        ..Default::default()
                    },
                    _ => unreachable!(),
                };
                tx.send(reply).unwrap();
            });
            Box::pin(ChannelFuture(Some(rx)))
        };
        (
            DartKeystore {
                callback: Box::new(callback),
                timeout,
            },
            map,
        )
    }

    /// Resolves when the mpsc receiver has a value; polls with a short
    /// self-wake (good enough for tests).
    struct ChannelFuture(Option<std::sync::mpsc::Receiver<PlatformSecretReply>>);

    impl Future for ChannelFuture {
        type Output = PlatformSecretReply;
        fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let Some(rx) = self.0.as_ref() else {
                return Poll::Pending;
            };
            match rx.try_recv() {
                Ok(v) => Poll::Ready(v),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    let waker = cx.waker().clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(2));
                        waker.wake();
                    });
                    Poll::Pending
                }
                // The "Dart side" dropped the call: never resolves.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.0 = None;
                    Poll::Pending
                }
            }
        }
    }

    #[test]
    fn crud_through_the_callback() {
        let (keystore, map) = map_keystore(Duration::from_secs(5));
        let dir = tempfile::tempdir().unwrap();
        let info = open_platform(keystore, Some(dir.path().to_str().unwrap().into())).unwrap();
        assert_eq!(info.backend, "platform");
        assert!(info.secure);
        assert_eq!(secrets_info(), Some(info));

        let key = "source.password:1".to_owned();
        assert_eq!(secret_get(key.clone()).unwrap(), None);
        secret_set(key.clone(), "hunter2-frb".into()).unwrap();
        assert_eq!(map.lock().unwrap()[&key], "hunter2-frb");
        assert_eq!(
            secret_get(key.clone()).unwrap().as_deref(),
            Some("hunter2-frb")
        );
        secret_set(key.clone(), "second".into()).unwrap();
        assert_eq!(secret_get(key.clone()).unwrap().as_deref(), Some("second"));
        // Known values are masked in logs.
        assert!(!pa_secrets::redact("pw second").contains("second"));
        assert!(secret_delete(key.clone()).unwrap());
        assert!(!secret_delete(key.clone()).unwrap());
        assert_eq!(secret_get(key).unwrap(), None);

        let err = secret_set("fail.locked".into(), "x".into()).unwrap_err();
        assert_eq!(err.code, "locked");
        let err = secret_get("bad key!".into()).unwrap_err();
        assert_eq!(err.code, "invalid_key");

        secrets_close();
        assert_eq!(secrets_info(), None);
        assert_eq!(secret_get("a".into()).unwrap_err().code, "unavailable");
    }

    #[test]
    fn a_silent_platform_store_times_out() {
        let (keystore, _) = map_keystore(Duration::from_millis(100));
        let started = Instant::now();
        let err = keystore.save("never.answer", "x").unwrap_err();
        assert!(matches!(err, SecretError::Unavailable(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn debug_hides_values() {
        let call = PlatformSecretCall {
            op: "save".into(),
            key: "k".into(),
            value: Some("hunter2".into()),
        };
        let reply = PlatformSecretReply {
            value: Some("hunter2".into()),
            ..Default::default()
        };
        assert!(!format!("{call:?}{reply:?}").contains("hunter2"));
    }
}
