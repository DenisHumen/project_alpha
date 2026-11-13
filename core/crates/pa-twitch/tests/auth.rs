// SPDX-License-Identifier: MPL-2.0
//! Device Code Flow, refresh, validation and revocation against recorded
//! responses (`tests/fixtures/auth/`). No network: the fixture transports
//! fail on any unplanned call. All tokens here are fake.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pa_secrets::{MemoryStore, SecretKey, SecretStore, SecretString, redact};
use pa_twitch::testing::{FixtureTransport, ManualClock, fixture};
use pa_twitch::{
    AuthConfig, AuthError, AuthorizedHelix, BoxFuture, ClientId, HelixClient, HelixConfig,
    HttpRequest, HttpResponse, Transport, TransportError, TwitchAuth,
};

const START: u64 = 1_800_000_000;
const CLIENT_ID: &str = "testclientid0000000000000000000";
const USER_ID: &str = "424242";
const ACCESS_1: &str = "fakeaccess1000000000000000000";
const REFRESH_1: &str = "fakerefresh1000000000000000000000000000000000000";
const ACCESS_2: &str = "fakeaccess2000000000000000000";
const REFRESH_2: &str = "fakerefresh2000000000000000000000000000000000000";
const DEVICE_CODE: &str = "fakeDeviceCode0000000000000000000000000";

macro_rules! fx {
    ($name:literal) => {
        include_str!(concat!("fixtures/auth/", $name, ".json"))
    };
}

/// Yields a few times before answering, so concurrent callers interleave.
struct Yielding(Arc<FixtureTransport>);

impl Transport for Yielding {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        let answer = self.0.send(request);
        Box::pin(async move {
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
            answer.await
        })
    }
}

struct Harness {
    auth: Arc<TwitchAuth>,
    transport: Arc<FixtureTransport>,
    clock: Arc<ManualClock>,
    store: Arc<MemoryStore>,
}

fn harness() -> Harness {
    let transport = Arc::new(FixtureTransport::new());
    let clock = Arc::new(ManualClock::at_unix(START));
    let store = Arc::new(MemoryStore::new());
    let mut config = AuthConfig::new(ClientId::new(CLIENT_ID).unwrap());
    config.id_base_url = "https://id.test/oauth2".to_owned();
    let auth = TwitchAuth::with_clock(
        config,
        Arc::new(Yielding(transport.clone())),
        clock.clone(),
        store.clone(),
    );
    Harness {
        auth: Arc::new(auth),
        transport,
        clock,
        store,
    }
}

fn key() -> SecretKey {
    SecretKey::twitch_tokens(USER_ID).unwrap()
}

impl Harness {
    /// Tokens stored by an earlier run, valid until `START + 4 h`.
    async fn restored(&self) {
        let doc = serde_json::json!({
            "v": 1,
            "access_token": ACCESS_1,
            "refresh_token": REFRESH_1,
            "expires_at": START + 14_400,
            "scopes": ["user:read:follows"],
            "user_id": USER_ID,
            "login": "test_viewer",
        });
        self.store
            .set(&key(), &SecretString::new(doc.to_string()))
            .unwrap();
        let account = self.auth.restore(USER_ID).await.unwrap().unwrap();
        assert_eq!(account.login, "test_viewer");
    }

    fn stored(&self) -> Option<serde_json::Value> {
        self.store
            .get(&key())
            .unwrap()
            .map(|v| serde_json::from_str(v.expose()).unwrap())
    }

    fn bodies(&self) -> Vec<String> {
        self.transport
            .requests()
            .iter()
            .map(|r| String::from_utf8(r.body.clone().unwrap_or_default()).unwrap())
            .collect()
    }
}

#[tokio::test]
async fn device_code_flow_signs_in_and_stores_tokens_per_device() {
    let h = harness();
    h.transport
        .push_fixture(fx!("device"))
        .push_fixture(fx!("pending"))
        .push_fixture(fx!("slow_down"))
        .push_fixture(fx!("token"))
        .push_fixture(fx!("validate"));

    let device = h.auth.start_device_login().await.unwrap();
    assert_eq!(device.user_code, "TESTCODE");
    assert!(
        device
            .verification_uri
            .starts_with("https://www.twitch.tv/activate")
    );
    assert_eq!(device.interval, Duration::from_secs(5));

    let account = h.auth.complete_device_login(device).await.unwrap();
    assert_eq!(account.user_id, USER_ID);
    assert_eq!(account.login, "test_viewer");
    assert_eq!(account.scopes, ["user:read:chat", "user:read:follows"]);
    // Interval honoured, and grown by 5 s after `slow_down`.
    assert_eq!(
        h.clock.sleeps(),
        [5, 5, 10].map(Duration::from_secs).to_vec()
    );

    let requests = h.transport.requests();
    let urls: Vec<&str> = requests.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://id.test/oauth2/device",
            "https://id.test/oauth2/token",
            "https://id.test/oauth2/token",
            "https://id.test/oauth2/token",
            "https://id.test/oauth2/validate",
        ]
    );
    let bodies = h.bodies();
    // Public client: id and minimal scopes, never a secret.
    assert_eq!(
        bodies[0],
        format!("client_id={CLIENT_ID}&scopes=user%3Aread%3Afollows%20user%3Aread%3Achat")
    );
    assert!(bodies[1].contains(&format!("device_code={DEVICE_CODE}")));
    assert!(
        bodies[1].contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
    );
    assert!(bodies.iter().all(|b| !b.contains("secret")));
    assert_eq!(
        requests[4].header("authorization"),
        Some(format!("OAuth {ACCESS_1}").as_str())
    );

    // Stored under this device's key only.
    assert_eq!(h.store.keys(), [key()]);
    let stored = h.stored().unwrap();
    assert_eq!(stored["access_token"], ACCESS_1);
    assert_eq!(stored["refresh_token"], REFRESH_1);
    assert_eq!(stored["expires_at"], START + 20 + 14_400);
}

#[tokio::test]
async fn device_code_expiry_and_refusal() {
    let h = harness();
    h.transport.push_fixture(fx!("device"));
    let mut device = h.auth.start_device_login().await.unwrap();
    h.clock.advance(Duration::from_secs(1800));
    assert_eq!(
        h.auth.poll_device_login(&mut device).await,
        Err(AuthError::DeviceCodeExpired)
    );
    assert_eq!(h.transport.requests().len(), 1, "no poll after expiry");

    let h = harness();
    h.transport
        .push_fixture(fx!("device"))
        .push_fixture(fx!("invalid_device_code"))
        .push_fixture(fx!("device"))
        .push_fixture(fx!("access_denied"));
    let device = h.auth.start_device_login().await.unwrap();
    assert_eq!(
        h.auth.complete_device_login(device).await,
        Err(AuthError::DeviceCodeExpired)
    );
    let device = h.auth.start_device_login().await.unwrap();
    assert_eq!(
        h.auth.complete_device_login(device).await,
        Err(AuthError::AccessDenied)
    );
    assert!(h.auth.account().is_none());
    assert!(h.store.keys().is_empty());
}

#[tokio::test]
async fn refreshes_ten_minutes_before_expiry() {
    let h = harness();
    h.restored().await;

    h.clock.advance(Duration::from_secs(14_400 - 601));
    assert_eq!(h.auth.access_token().await.unwrap().secret(), ACCESS_1);
    assert!(h.transport.requests().is_empty());

    h.clock.advance(Duration::from_secs(1));
    h.transport.push_fixture(fx!("token_refreshed"));
    assert_eq!(h.auth.access_token().await.unwrap().secret(), ACCESS_2);
    assert_eq!(
        h.bodies(),
        [format!(
            "client_id={CLIENT_ID}&grant_type=refresh_token&refresh_token={REFRESH_1}"
        )]
    );
    // The single-use refresh token is replaced in the store.
    let stored = h.stored().unwrap();
    assert_eq!(stored["access_token"], ACCESS_2);
    assert_eq!(stored["refresh_token"], REFRESH_2);
    assert_eq!(stored["user_id"], USER_ID);
}

#[tokio::test]
async fn parallel_callers_share_one_refresh() {
    let h = harness();
    h.restored().await;
    h.clock.advance(Duration::from_secs(14_400 - 60));
    h.transport.push_fixture(fx!("token_refreshed"));

    let a = &h.auth;
    let tokens = tokio::join!(
        a.access_token(),
        a.access_token(),
        a.access_token(),
        a.access_token(),
        a.access_token(),
        a.access_token(),
        a.access_token(),
        a.access_token(),
    );
    for token in [
        tokens.0, tokens.1, tokens.2, tokens.3, tokens.4, tokens.5, tokens.6, tokens.7,
    ] {
        assert_eq!(token.unwrap().secret(), ACCESS_2);
    }
    // A second refresh would have spent the new refresh token and failed.
    assert_eq!(h.transport.requests().len(), 1);
    assert_eq!(h.stored().unwrap()["refresh_token"], REFRESH_2);
}

/// Helix stand-in: `401` for the old token, the user for the new one.
#[derive(Default)]
struct TokenAwareHelix {
    calls: AtomicUsize,
}

impl Transport for TokenAwareHelix {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let response = match request.header("authorization") {
            Some(v) if v == format!("Bearer {ACCESS_2}") => {
                fixture(include_str!("fixtures/users.json"))
            }
            _ => fixture(include_str!("fixtures/unauthorized.json")),
        };
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(response)
        })
    }
}

fn authorized(h: &Harness, helix_transport: Arc<TokenAwareHelix>) -> AuthorizedHelix {
    let mut config = HelixConfig::new(ClientId::new(CLIENT_ID).unwrap());
    config.base_url = "https://helix.test/helix".to_owned();
    let helix = HelixClient::with_clock(config, helix_transport, h.clock.clone());
    AuthorizedHelix::new(h.auth.clone(), Arc::new(helix))
}

#[tokio::test]
async fn unauthorized_from_helix_refreshes_once_and_retries() {
    let h = harness();
    h.restored().await;
    let helix_transport = Arc::new(TokenAwareHelix::default());
    let api = authorized(&h, helix_transport.clone());
    h.transport.push_fixture(fx!("token_refreshed"));

    let (a, b) = tokio::join!(
        api.call(|c| async move { c.get_current_user().await }),
        api.call(|c| async move { c.get_current_user().await }),
    );
    assert_eq!(a.unwrap().id, "1001");
    assert_eq!(b.unwrap().id, "1001");
    // Both saw 401 with the old token; only one refresh went out.
    assert_eq!(h.transport.requests().len(), 1);
    assert_eq!(helix_transport.calls.load(Ordering::SeqCst), 4);
    assert_eq!(h.stored().unwrap()["access_token"], ACCESS_2);
}

#[tokio::test]
async fn dead_refresh_token_requires_sign_in_again() {
    let h = harness();
    h.restored().await;
    let api = authorized(&h, Arc::new(TokenAwareHelix::default()));
    // 30 days without use: Twitch no longer accepts the refresh token.
    h.clock.advance(Duration::from_secs(30 * 24 * 3600));
    h.transport.push_fixture(fx!("refresh_invalid"));

    let result = api
        .call(|c| async move { c.get_current_user().await })
        .await;
    assert_eq!(result.unwrap_err(), AuthError::ReauthRequired);
    assert!(h.auth.account().is_none());
    assert!(h.store.keys().is_empty(), "dead tokens are deleted");
    assert!(!api.helix().is_signed_in());
    assert_eq!(
        h.auth.access_token().await.unwrap_err(),
        AuthError::NotSignedIn
    );
}

#[tokio::test]
async fn validates_hourly_and_refreshes_a_rejected_token() {
    let h = harness();
    h.restored().await;
    h.transport
        .push_fixture(fx!("validate_unauthorized"))
        .push_fixture(fx!("token_refreshed"))
        .push_fixture(fx!("validate"));
    let v = h.auth.validate_if_due().await.unwrap().unwrap();
    assert_eq!(v.user_id, USER_ID);
    assert_eq!(h.transport.requests().len(), 3);
    assert_eq!(
        h.transport.requests()[2].header("authorization"),
        Some(format!("OAuth {ACCESS_2}").as_str())
    );

    h.clock.advance(Duration::from_secs(59 * 60));
    assert_eq!(h.auth.validate_if_due().await.unwrap(), None);
    assert_eq!(h.transport.requests().len(), 3);

    h.clock.advance(Duration::from_secs(60));
    h.transport.push_fixture(fx!("validate"));
    assert!(h.auth.validate_if_due().await.unwrap().is_some());
}

#[tokio::test]
async fn sign_out_revokes_and_deletes_even_offline() {
    let h = harness();
    h.restored().await;
    h.transport.push_fixture(fx!("revoke"));
    h.auth.sign_out().await.unwrap();
    assert_eq!(
        h.transport.requests()[0].url,
        "https://id.test/oauth2/revoke"
    );
    assert_eq!(
        h.bodies(),
        [format!("client_id={CLIENT_ID}&token={ACCESS_1}")]
    );
    assert!(h.auth.account().is_none());
    assert!(h.store.keys().is_empty());

    let h = harness();
    h.restored().await;
    h.transport.push_error("offline");
    h.auth.sign_out().await.unwrap();
    assert!(h.auth.account().is_none());
    assert!(h.store.keys().is_empty());
    assert_eq!(h.auth.restore(USER_ID).await.unwrap(), None);
}

#[tokio::test]
async fn tokens_never_show_up_in_debug_output_or_logs() {
    let h = harness();
    h.transport
        .push_fixture(fx!("device"))
        .push_fixture(fx!("token"))
        .push_fixture(fx!("validate"));
    let device = h.auth.start_device_login().await.unwrap();
    let device_debug = format!("{device:?}");
    h.auth.complete_device_login(device).await.unwrap();

    let secrets = [ACCESS_1, REFRESH_1, DEVICE_CODE];
    let mut dumps = vec![
        device_debug,
        format!("{:?}", h.auth),
        format!("{:?}", h.auth.account()),
        format!("{:?}", h.auth.access_token().await),
        format!("{:?}", h.store),
    ];
    dumps.extend(h.transport.requests().iter().map(|r| format!("{r:?}")));
    for dump in &dumps {
        for secret in secrets {
            assert!(!dump.contains(secret), "{secret} leaked in {dump}");
        }
    }
    // The logger runs every line through `redact`; the live tokens are
    // registered with it.
    let line = format!("helix call with {ACCESS_1} after refresh {REFRESH_1}");
    let masked = redact(&line);
    assert!(
        !masked.contains(ACCESS_1) && !masked.contains(REFRESH_1),
        "{masked}"
    );
}

#[tokio::test]
async fn restore_without_stored_tokens_is_signed_out() {
    let h = harness();
    assert_eq!(h.auth.restore(USER_ID).await.unwrap(), None);
    assert_eq!(h.auth.account(), None);
    assert_eq!(h.auth.refresh().await.unwrap_err(), AuthError::NotSignedIn);
    assert!(h.transport.requests().is_empty());
}
