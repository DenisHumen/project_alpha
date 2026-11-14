// SPDX-License-Identifier: MPL-2.0
//! Client tests. Every key here is a THROWAWAY TEST KEY generated inside the
//! test run; no key material exists in the repository.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signer, SigningKey};

use crate::*;

const FILE: &str = DEFAULT_FILE_NAME;
const MIRROR_A: &str = "https://mirror-a.test/config";
const MIRROR_B: &str = "https://mirror-b.test/config";

/// A throwaway minisign key pair (test only), producing the same format as
/// `minisign -S` (prehashed `ED` signatures).
struct TestSigner {
    sk: SigningKey,
    key_id: [u8; 8],
}

impl TestSigner {
    fn generate() -> Self {
        let mut seed = [0u8; 32];
        let mut key_id = [0u8; 8];
        getrandom::fill(&mut seed).unwrap();
        getrandom::fill(&mut key_id).unwrap();
        Self {
            sk: SigningKey::from_bytes(&seed),
            key_id,
        }
    }

    fn public_b64(&self) -> String {
        let mut bin = b"Ed".to_vec();
        bin.extend_from_slice(&self.key_id);
        bin.extend_from_slice(&self.sk.verifying_key().to_bytes());
        STANDARD.encode(bin)
    }

    fn trusted_key(&self) -> TrustedKey {
        TrustedKey::from_base64(&self.public_b64()).unwrap()
    }

    fn sign_raw(&self, algorithm: &[u8; 2], message: &[u8], trusted_comment: &str) -> String {
        let signature = self.sk.sign(message).to_bytes();
        let mut bin = algorithm.to_vec();
        bin.extend_from_slice(&self.key_id);
        bin.extend_from_slice(&signature);
        let mut global = signature.to_vec();
        global.extend_from_slice(trusted_comment.as_bytes());
        let global_signature = self.sk.sign(&global).to_bytes();
        format!(
            "untrusted comment: signature from a throwaway test key\n{}\ntrusted comment: {}\n{}\n",
            STANDARD.encode(bin),
            trusted_comment,
            STANDARD.encode(global_signature)
        )
    }

    fn sign_with_comment(&self, body: &[u8], trusted_comment: &str) -> String {
        let hash = Blake2b512::digest(body);
        self.sign_raw(b"ED", hash.as_slice(), trusted_comment)
    }

    /// What `minisign -S -m remote-config.json` writes.
    fn sign(&self, body: &[u8]) -> String {
        self.sign_with_comment(body, &format!("timestamp:1763078400\tfile:{FILE}\thashed"))
    }
}

/// In-memory static hosting with ETags and a request log.
#[derive(Default)]
struct MockHost {
    files: Mutex<HashMap<String, (Vec<u8>, String)>>,
    next_etag: AtomicU64,
    offline: AtomicBool,
    log: Mutex<Vec<(String, Option<String>)>>,
}

impl MockHost {
    fn put(&self, url: &str, body: impl Into<Vec<u8>>) {
        let etag = format!("\"v{}\"", self.next_etag.fetch_add(1, Ordering::SeqCst));
        self.files
            .lock()
            .unwrap()
            .insert(url.to_owned(), (body.into(), etag));
    }

    /// Publishes a document and its signature on `mirror`.
    fn publish(&self, mirror: &str, body: &str, signature: &str) {
        self.put(&format!("{mirror}/{FILE}"), body);
        self.put(&format!("{mirror}/{FILE}.minisig"), signature);
    }

    fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
    }

    fn requests(&self) -> Vec<(String, Option<String>)> {
        self.log.lock().unwrap().clone()
    }
}

impl ConfigFetcher for MockHost {
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<Fetched, FetchError> {
        self.log
            .lock()
            .unwrap()
            .push((url.to_owned(), if_none_match.map(str::to_owned)));
        if self.offline.load(Ordering::SeqCst) {
            return Err(FetchError("host not found".into()));
        }
        Ok(match self.files.lock().unwrap().get(url) {
            None => Fetched {
                status: 404,
                ..Fetched::default()
            },
            Some((_, etag)) if if_none_match == Some(etag.as_str()) => Fetched {
                status: 304,
                etag: Some(etag.clone()),
                body: Vec::new(),
            },
            Some((body, etag)) => Fetched {
                status: 200,
                etag: Some(etag.clone()),
                body: body.clone(),
            },
        })
    }
}

fn doc(serial: u64, gql: bool) -> String {
    format!(
        r#"{{
  "schema": 1,
  "serial": {serial},
  "flags": {{ "twitch.gql_playback": {gql} }},
  "twitch": {{ "gql_client_id": "public-web-client-id", "player": "native",
              "persisted_queries": {{ "PlaybackAccessToken": "0123abcd" }} }}
}}
"#
    )
}

fn options(dir: &Path, mirrors: &[&str]) -> ClientOptions {
    ClientOptions {
        cache_dir: dir.to_owned(),
        file_name: FILE.to_owned(),
        endpoints: mirrors.iter().map(|m| Endpoint::new(m, FILE)).collect(),
    }
}

fn client(key: Option<TrustedKey>, dir: &Path, host: &Arc<MockHost>) -> RemoteConfigClient {
    RemoteConfigClient::open(key, options(dir, &[MIRROR_A]), host.clone())
}

fn android() -> ClientContext {
    ClientContext {
        app_version: "0.1.0-alpha.3".into(),
        platform: "android".into(),
    }
}

#[test]
fn builtin_copy_and_repo_example_are_valid() {
    let b = builtin();
    assert_eq!(b.schema, SUPPORTED_SCHEMA);
    // Safe defaults: nothing that depends on undocumented Twitch APIs is on.
    let t = b.twitch_flags(&android());
    assert!(!t.gql_playback && !t.chat_replay && !t.chapters_gql && !t.archive);
    assert_eq!(t.player, PlayerMode::Embed);

    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../config/remote-config.json");
    let example = RemoteConfig::parse(&std::fs::read(example).unwrap()).unwrap();
    assert_eq!(example.serial, b.serial);
    assert_eq!(example.flags, b.flags);
}

#[test]
fn signed_document_is_accepted_cached_and_used() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let body = doc(2, true);
    host.publish(MIRROR_A, &body, &signer.sign(body.as_bytes()));

    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);
    assert_eq!(
        c.refresh_at(1_000).unwrap(),
        RefreshOutcome::Updated { serial: 2 }
    );
    let s = c.snapshot();
    assert_eq!(s.source, ConfigSource::Remote);
    assert_eq!(s.config.serial, 2);
    assert_eq!(s.checked_at, Some(1_000));
    let t = s.config.twitch_flags(&android());
    assert!(t.gql_playback);
    assert_eq!(t.player, PlayerMode::Native);
    assert_eq!(t.gql_client_id.as_deref(), Some("public-web-client-id"));
    assert_eq!(t.persisted_queries["PlaybackAccessToken"], "0123abcd");
}

#[test]
fn bad_signatures_are_rejected() {
    let signer = TestSigner::generate();
    let other = TestSigner::generate();
    let good = doc(2, true);
    let tampered = doc(2, true).replace("0123abcd", "evil0000");
    let cases: Vec<(&str, String, String)> = vec![
        // Bytes changed after signing.
        (
            "tampered body",
            tampered.clone(),
            signer.sign(good.as_bytes()),
        ),
        // Signed by a key the app does not trust.
        ("foreign key", good.clone(), other.sign(good.as_bytes())),
        // Valid signature of another file (e.g. a twitch/… document).
        (
            "other file",
            good.clone(),
            signer.sign_with_comment(good.as_bytes(), "timestamp:1\tfile:other.json\thashed"),
        ),
        // Legacy non-prehashed signature (minisign < 0.6): refused.
        (
            "legacy",
            good.clone(),
            signer.sign_raw(b"Ed", good.as_bytes(), &format!("file:{FILE}")),
        ),
        // Valid signature line, forged trusted comment.
        (
            "forged comment",
            good.clone(),
            signer.sign_with_comment(good.as_bytes(), "file:x").replace(
                "trusted comment: file:x",
                &format!("trusted comment: file:{FILE}"),
            ),
        ),
        ("garbage", good.clone(), "not a signature".into()),
        ("empty", good.clone(), String::new()),
    ];
    for (name, body, signature) in cases {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(MockHost::default());
        host.publish(MIRROR_A, &body, &signature);
        let c = client(Some(signer.trusted_key()), dir.path(), &host);
        let err = c.refresh_at(1_000).unwrap_err();
        assert!(
            matches!(err, Error::BadSignature(_)),
            "{name}: expected BadSignature, got {err:?}"
        );
        let s = c.snapshot();
        assert_eq!(s.source, ConfigSource::BuiltIn, "{name}");
        assert_eq!(s.config.serial, 1, "{name}");
        assert_eq!(s.failed_at, Some(1_000), "{name}");
        assert!(!s.config.twitch_flags(&android()).gql_playback, "{name}");
        // Nothing unverified reaches the disk.
        assert!(
            std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "{name}: cache written"
        );
    }
}

#[test]
fn missing_signature_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    host.put(&format!("{MIRROR_A}/{FILE}"), doc(2, true));
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert_eq!(c.refresh_at(1).unwrap_err(), Error::Http { status: 404 });
    assert_eq!(c.config().serial, 1);
}

#[test]
fn unsigned_bytes_are_never_parsed_but_signed_new_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let c = client(Some(signer.trusted_key()), dir.path(), &host);

    host.publish(MIRROR_A, "{ broken json", "garbage");
    assert!(matches!(c.refresh_at(1), Err(Error::BadSignature(_))));

    let future = r#"{ "schema": 99, "serial": 50 }"#;
    host.publish(MIRROR_A, future, &signer.sign(future.as_bytes()));
    assert_eq!(c.refresh_at(2).unwrap_err(), Error::UnsupportedSchema(99));
    assert_eq!(c.config().serial, 1);
}

#[test]
fn offline_uses_the_last_valid_copy() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v2 = doc(2, true);
    host.publish(MIRROR_A, &v2, &signer.sign(v2.as_bytes()));
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    c.refresh_at(100).unwrap();

    // A newer, badly signed document does not replace the good one.
    let v3 = doc(3, false);
    host.publish(MIRROR_A, &v3, &signer.sign(doc(3, true).as_bytes()));
    assert!(matches!(c.refresh_at(200), Err(Error::BadSignature(_))));
    assert_eq!(c.config().serial, 2);

    // Offline in this run: still the last valid copy.
    host.set_offline(true);
    assert!(matches!(c.refresh_at(300), Err(Error::Offline(_))));
    assert_eq!(c.snapshot().source, ConfigSource::Remote);
    assert_eq!(c.config().serial, 2);
    drop(c);

    // Next start, still offline: the verified disk copy, not the built-in.
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    let s = c.snapshot();
    assert_eq!(s.source, ConfigSource::Cache);
    assert_eq!(s.config.serial, 2);
    assert_eq!(s.checked_at, Some(100));
    assert!(s.config.twitch_flags(&android()).gql_playback);
    assert!(matches!(c.refresh_at(400), Err(Error::Offline(_))));
    assert_eq!(c.config().serial, 2);
}

#[test]
fn tampered_or_foreign_cache_falls_back_to_builtin() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v2 = doc(2, true);
    host.publish(MIRROR_A, &v2, &signer.sign(v2.as_bytes()));
    client(Some(signer.trusted_key()), dir.path(), &host)
        .refresh_at(1)
        .unwrap();
    host.set_offline(true);

    // Another key (e.g. a different build) does not trust this cache.
    let c = client(
        Some(TestSigner::generate().trusted_key()),
        dir.path(),
        &host,
    );
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);

    // Edited on disk.
    let path = crate::cache::DiskCache::new(dir.path(), FILE)
        .path()
        .to_owned();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("0123abcd", "0123abce")).unwrap();
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);
    assert_eq!(c.config().serial, 1);

    // Truncated / not JSON.
    std::fs::write(&path, b"{").unwrap();
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);
}

#[test]
fn etag_is_sent_and_304_keeps_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v2 = doc(2, true);
    host.publish(MIRROR_A, &v2, &signer.sign(v2.as_bytes()));
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    c.refresh_at(10).unwrap();
    let first = host.requests();
    assert_eq!(first.len(), 2, "document + signature");
    assert_eq!(first[0].1, None);

    assert_eq!(c.refresh_at(20).unwrap(), RefreshOutcome::Unchanged);
    let all = host.requests();
    assert_eq!(all.len(), 3, "304: the signature is not downloaded again");
    assert_eq!(all[2].0, format!("{MIRROR_A}/{FILE}"));
    assert_eq!(all[2].1.as_deref(), Some("\"v0\""));
    assert_eq!(c.snapshot().checked_at, Some(20));
    drop(c);

    // The ETag and check time survive a restart.
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert_eq!(c.snapshot().checked_at, Some(20));
    assert_eq!(c.refresh_at(30).unwrap(), RefreshOutcome::Unchanged);
    assert_eq!(host.requests().last().unwrap().1.as_deref(), Some("\"v0\""));

    // A new publication changes the ETag: downloaded and verified again.
    let v3 = doc(3, false);
    host.publish(MIRROR_A, &v3, &signer.sign(v3.as_bytes()));
    assert_eq!(
        c.refresh_at(40).unwrap(),
        RefreshOutcome::Updated { serial: 3 }
    );
    assert!(!c.config().twitch_flags(&android()).gql_playback);
}

#[test]
fn older_serial_is_a_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v3 = doc(3, false);
    host.publish(MIRROR_A, &v3, &signer.sign(v3.as_bytes()));
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    c.refresh_at(1).unwrap();

    // A validly signed but older file (replayed by a mirror or attacker)
    // cannot switch a kill-switch back off.
    let v2 = doc(2, true);
    host.publish(MIRROR_A, &v2, &signer.sign(v2.as_bytes()));
    assert_eq!(
        c.refresh_at(2).unwrap_err(),
        Error::Rollback { got: 2, have: 3 }
    );
    assert_eq!(c.config().serial, 3);
    drop(c);

    // A cache older than a newer app's built-in copy is ignored.
    let mut newer_builtin = builtin();
    newer_builtin.serial = 5;
    let c = RemoteConfigClient::open_with_builtin(
        Some(signer.trusted_key()),
        options(dir.path(), &[MIRROR_A]),
        host.clone(),
        newer_builtin,
    );
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);
    assert_eq!(c.config().serial, 5);
}

#[test]
fn next_mirror_is_tried() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v2 = doc(2, true);
    // Mirror A has a stale signature next to the new file (CDN lag).
    host.publish(MIRROR_A, &v2, &signer.sign(doc(1, false).as_bytes()));
    host.publish(MIRROR_B, &v2, &signer.sign(v2.as_bytes()));
    let c = RemoteConfigClient::open(
        Some(signer.trusted_key()),
        options(dir.path(), &[MIRROR_A, MIRROR_B]),
        host.clone(),
    );
    assert_eq!(
        c.refresh_at(1).unwrap(),
        RefreshOutcome::Updated { serial: 2 }
    );
    // The cached ETag belongs to mirror B: mirror A is asked without one
    // (and still fails), mirror B with it and answers 304.
    assert_eq!(c.refresh_at(2).unwrap(), RefreshOutcome::Unchanged);
    let log = host.requests();
    let a_doc = format!("{MIRROR_A}/{FILE}");
    assert!(
        log.iter()
            .filter(|(u, _)| *u == a_doc)
            .all(|(_, e)| e.is_none())
    );
    let (last_url, last_etag) = log.last().unwrap();
    assert_eq!(*last_url, format!("{MIRROR_B}/{FILE}"));
    assert!(last_etag.is_some());
    assert_eq!(c.config().serial, 2);

    // Every mirror down: the error of the last one, the copy stays.
    host.set_offline(true);
    assert!(matches!(c.refresh_at(3), Err(Error::Offline(_))));
    assert_eq!(c.config().serial, 2);
}

#[test]
fn placeholder_key_means_builtin_only() {
    // Until the owner replaces it, the production key is a placeholder and
    // nothing remote is ever trusted, in debug and release builds alike.
    assert!(production_key_is_placeholder());
    assert_eq!(TrustedKey::production().unwrap_err(), Error::NoTrustedKey);
    assert_eq!(
        TrustedKey::from_base64(PRODUCTION_PUBLIC_KEY).unwrap_err(),
        Error::NoTrustedKey
    );

    // A valid cache written by a client with some key…
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let v2 = doc(2, true);
    host.publish(MIRROR_A, &v2, &signer.sign(v2.as_bytes()));
    client(Some(signer.trusted_key()), dir.path(), &host)
        .refresh_at(1)
        .unwrap();
    let before = host.requests().len();

    // …is ignored without a trusted key, and the network is not touched.
    let c = client(TrustedKey::production().ok(), dir.path(), &host);
    assert!(!c.remote_enabled());
    assert_eq!(c.snapshot().source, ConfigSource::BuiltIn);
    assert!(!c.refresh_due(u64::MAX));
    assert_eq!(c.refresh_at(2).unwrap_err(), Error::NoTrustedKey);
    assert_eq!(c.refresh_if_due().unwrap(), RefreshOutcome::NotDue);
    assert_eq!(host.requests().len(), before);
}

#[test]
fn production_key_constant_is_placeholder_or_valid() {
    // Guards the owner's edit: a real key must decode.
    if !production_key_is_placeholder() {
        TrustedKey::production().expect("PRODUCTION_PUBLIC_KEY must be a minisign public key");
    }
    assert!(TrustedKey::from_base64("RWQ-not-base64!").is_err());
    let signer = TestSigner::generate();
    let pub_file = format!(
        "untrusted comment: minisign public key (test)\n{}\n",
        signer.public_b64()
    );
    let key = TrustedKey::decode(&pub_file).unwrap();
    let body = doc(2, false);
    key.verify(FILE, body.as_bytes(), &signer.sign(body.as_bytes()))
        .unwrap();
}

#[test]
fn refresh_policy_schedules_checks_and_retries() {
    let dir = tempfile::tempdir().unwrap();
    let signer = TestSigner::generate();
    let host = Arc::new(MockHost::default());
    let body =
        r#"{ "schema": 1, "serial": 2, "refresh": { "interval_secs": 3600, "retry_secs": 600 } }"#;
    host.publish(MIRROR_A, body, &signer.sign(body.as_bytes()));
    let c = client(Some(signer.trusted_key()), dir.path(), &host);
    assert!(c.refresh_due(0), "never checked");
    c.refresh_at(1_000).unwrap();
    assert!(!c.refresh_due(1_000 + 3_599));
    assert!(c.refresh_due(1_000 + 3_600));

    host.set_offline(true);
    c.refresh_at(5_000).unwrap_err();
    assert!(!c.refresh_due(5_000 + 599));
    assert!(c.refresh_due(5_000 + 600));

    host.set_offline(false);
    c.refresh_at(6_000).unwrap();
    assert_eq!(c.snapshot().failed_at, None);
    assert!(!c.refresh_due(6_001));
}
