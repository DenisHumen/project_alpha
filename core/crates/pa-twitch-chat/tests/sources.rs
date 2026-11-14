// SPDX-License-Identifier: MPL-2.0
//! Connection loops of both sources on scripted WebSocket connections and
//! recorded Helix responses: handshake, PONG, reconnect with backoff,
//! EventSub session moves, de-duplication, revocation and sending. No test
//! touches the network.

use std::sync::Arc;
use std::time::Duration;

use pa_twitch::testing::{FixtureTransport, ManualClock};
use pa_twitch::{AccessToken, ClientId, HelixClient, HelixConfig};
use pa_twitch_chat::testing::{Script, ScriptedConnector};
use pa_twitch_chat::{
    Backoff, ChatError, ChatEvent, ChatSource, EventSubChatSource, EventSubConfig, IrcChatSource,
    IrcConfig, SourceKind, SourceStatus,
};

const START: u64 = 1_800_000_000;
const NICK: &str = "justinfan48213";
const IRC_URL: &str = "wss://irc.example.invalid";
const ES_URL: &str = "wss://eventsub.example.invalid/ws";
const RECONNECT_URL: &str = "wss://eventsub.example.invalid/ws?id=reconnect-0001";

macro_rules! es {
    ($name:literal) => {
        include_str!(concat!("fixtures/eventsub/", $name, ".json"))
    };
}

macro_rules! helix {
    ($name:literal) => {
        include_str!(concat!("fixtures/helix/", $name, ".json"))
    };
}

fn backoff() -> Backoff {
    Backoff {
        initial: Duration::from_secs(1),
        max: Duration::from_secs(30),
        jitter: false,
    }
}

fn irc_config(max_attempts: u32) -> IrcConfig {
    let mut config = IrcConfig::new(["Kinolune_Demo"]);
    config.url = IRC_URL.to_owned();
    config.nick = Some(NICK.to_owned());
    config.backoff = backoff();
    config.max_attempts = Some(max_attempts);
    config
}

async fn drain(source: &dyn ChatSource) -> Vec<ChatEvent> {
    let mut out = Vec::new();
    while let Some(e) = source.next_event().await {
        out.push(e);
        assert!(out.len() < 1000, "runaway source");
    }
    out
}

fn statuses(events: &[ChatEvent]) -> Vec<SourceStatus> {
    events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::Status(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn message_ids(events: &[ChatEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::Message(m) => Some(m.id.clone()),
            _ => None,
        })
        .collect()
}

// ----- anonymous IRC --------------------------------------------------------

#[tokio::test]
async fn irc_handshake_join_pong_and_idle_reconnect() {
    let connector = ScriptedConnector::new();
    let session = include_str!("fixtures/irc/session.txt").replace('\n', "\r\n");
    connector.push(Script::hanging([session]));
    let clock = Arc::new(ManualClock::at_unix(START));
    let source =
        IrcChatSource::spawn(irc_config(1), Arc::new(connector.clone()), clock).unwrap();
    assert_eq!(source.kind(), SourceKind::AnonymousIrc);
    assert!(!source.can_send());

    let events = drain(&source).await;

    let sent: Vec<String> = connector.sent().into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        sent,
        [
            "CAP REQ :twitch.tv/tags twitch.tv/commands",
            "PASS SCHMOOPIIE",
            "NICK justinfan48213",
            "JOIN #kinolune_demo",
            "PONG :tmi.twitch.tv",
        ]
    );
    assert_eq!(message_ids(&events).len(), 7);
    let st = statuses(&events);
    assert_eq!(st[0], SourceStatus::Connected);
    assert!(matches!(&st[1], SourceStatus::Joined(c) if c.login == "kinolune_demo"));
    assert_eq!(
        st[2],
        SourceStatus::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            reason: "no data from Twitch (idle timeout)".to_owned(),
        }
    );
    // The retry finds no server; max_attempts = 1 stops the source.
    assert!(matches!(st[3], SourceStatus::Stopped { .. }));
    assert_eq!(st.len(), 4);
    assert_eq!(connector.urls(), [IRC_URL, IRC_URL]);
}

#[tokio::test]
async fn irc_reconnect_command_moves_without_backoff() {
    let connector = ScriptedConnector::new();
    connector.push(Script::closing([
        ":tmi.twitch.tv 001 justinfan48213 :Welcome, GLHF!\r\n:tmi.twitch.tv RECONNECT\r\n",
    ]));
    connector.push(Script::closing([
        ":tmi.twitch.tv 001 justinfan48213 :Welcome, GLHF!\r\n",
        "@id=after-1;room-id=100000001;tmi-sent-ts=1794650020000 :pixel_otter!pixel_otter@pixel_otter.tmi.twitch.tv PRIVMSG #kinolune_demo :back again\r\n",
    ]));
    let clock = Arc::new(ManualClock::at_unix(START));
    let source =
        IrcChatSource::spawn(irc_config(0), Arc::new(connector.clone()), clock.clone()).unwrap();
    let events = drain(&source).await;

    let st = statuses(&events);
    assert_eq!(st[0], SourceStatus::Connected);
    assert_eq!(
        st[1],
        SourceStatus::Reconnecting {
            attempt: 1,
            delay: Duration::ZERO,
            reason: "server asked to reconnect".to_owned(),
        }
    );
    assert_eq!(st[2], SourceStatus::Connected);
    assert!(matches!(st[3], SourceStatus::Stopped { .. }));
    assert_eq!(message_ids(&events), ["after-1"]);
    // Both connections joined the channel.
    let joins: Vec<usize> = connector
        .sent()
        .into_iter()
        .filter(|(_, s)| s.starts_with("JOIN"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(joins, [0, 1]);
}

#[tokio::test]
async fn irc_backoff_grows_exponentially_until_max_attempts() {
    let connector = ScriptedConnector::new();
    for _ in 0..4 {
        connector.push_connect_error("connection refused");
    }
    let clock = Arc::new(ManualClock::at_unix(START));
    let source =
        IrcChatSource::spawn(irc_config(3), Arc::new(connector.clone()), clock.clone()).unwrap();
    let events = drain(&source).await;
    let delays: Vec<Duration> = statuses(&events)
        .into_iter()
        .filter_map(|s| match s {
            SourceStatus::Reconnecting { delay, .. } => Some(delay),
            _ => None,
        })
        .collect();
    assert_eq!(
        delays,
        [
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4)
        ]
    );
    assert!(matches!(
        statuses(&events).last(),
        Some(SourceStatus::Stopped { reason }) if reason.contains("connection refused")
    ));
    assert_eq!(connector.urls().len(), 4);
}

#[tokio::test]
async fn irc_is_read_only_and_validates_channels() {
    let connector = Arc::new(ScriptedConnector::new());
    let clock = Arc::new(ManualClock::at_unix(START));
    let source = IrcChatSource::spawn(irc_config(0), connector.clone(), clock.clone()).unwrap();
    assert_eq!(
        source.send_message("100000001", "hi", None).await,
        Err(ChatError::ReadOnly)
    );
    source.close();
    assert!(drain(&source).await.len() <= 1);

    let mut bad = irc_config(0);
    bad.channels = vec!["no spaces allowed".to_owned()];
    assert!(IrcChatSource::spawn(bad, connector.clone(), clock.clone()).is_err());
    let mut none = irc_config(0);
    none.channels.clear();
    assert!(IrcChatSource::spawn(none, connector, clock).is_err());
}

#[test]
fn anonymous_nick_shape() {
    let nick = pa_twitch_chat::anonymous_nick();
    let digits = nick.strip_prefix("justinfan").unwrap();
    assert!(!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
}

#[test]
fn backoff_caps_and_jitters() {
    let b = Backoff {
        initial: Duration::from_secs(1),
        max: Duration::from_secs(60),
        jitter: false,
    };
    assert_eq!(b.delay(1), Duration::from_secs(1));
    assert_eq!(b.delay(6), Duration::from_secs(32));
    assert_eq!(b.delay(7), Duration::from_secs(60));
    assert_eq!(b.delay(100), Duration::from_secs(60));
    let j = Backoff { jitter: true, ..b };
    for attempt in 1..10 {
        let d = j.delay(attempt);
        let step = b.delay(attempt);
        assert!(d >= step / 2 && d <= step, "{d:?} outside 50-100% of {step:?}");
    }
}

// ----- EventSub ---------------------------------------------------------------

struct EsHarness {
    connector: ScriptedConnector,
    transport: Arc<FixtureTransport>,
    helix: Arc<HelixClient>,
    clock: Arc<ManualClock>,
}

fn es_harness() -> EsHarness {
    let transport = Arc::new(FixtureTransport::new());
    let clock = Arc::new(ManualClock::at_unix(START));
    let mut config = HelixConfig::new(ClientId::new("testclientid0000000000000000000").unwrap());
    config.base_url = "https://helix.test/helix".to_owned();
    let helix = HelixClient::with_clock(config, transport.clone(), clock.clone());
    helix.set_access_token(AccessToken::new("test-user-token-not-real"));
    EsHarness {
        connector: ScriptedConnector::new(),
        transport,
        helix: Arc::new(helix),
        clock,
    }
}

impl EsHarness {
    fn spawn(&self, max_attempts: u32) -> EventSubChatSource {
        let mut config = EventSubConfig::new("300000001", ["100000001"]);
        config.url = ES_URL.to_owned();
        config.backoff = backoff();
        config.max_attempts = Some(max_attempts);
        EventSubChatSource::spawn(
            config,
            self.helix.clone(),
            Arc::new(self.connector.clone()),
            self.clock.clone(),
        )
        .unwrap()
    }

    fn push_subscriptions(&self) {
        for _ in 0..6 {
            self.transport.push_fixture(helix!("subscription_created"));
        }
    }
}

#[tokio::test]
async fn eventsub_subscribes_dedups_and_follows_session_reconnect() {
    let h = es_harness();
    h.push_subscriptions();
    h.connector.push(Script::hanging([
        es!("welcome"),
        es!("keepalive"),
        es!("chat_message"),
        es!("chat_message"), // duplicate delivery
        es!("message_delete"),
        es!("session_reconnect"),
    ]));
    h.connector
        .push(Script::closing([es!("welcome"), es!("chat_clear")]));
    let source = h.spawn(1);
    assert_eq!(source.kind(), SourceKind::EventSub);
    assert!(source.can_send());

    let events = drain(&source).await;

    // Six subscriptions on the first session only; none after the move.
    let requests = h.transport.requests();
    assert_eq!(requests.len(), 6);
    let mut types = Vec::new();
    for r in &requests {
        assert_eq!(r.url, "https://helix.test/helix/eventsub/subscriptions");
        assert_eq!(r.header("Content-Type"), Some("application/json"));
        assert_eq!(r.header("Client-Id"), Some("testclientid0000000000000000000"));
        let body: serde_json::Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["version"], "1");
        assert_eq!(body["condition"]["broadcaster_user_id"], "100000001");
        assert_eq!(body["condition"]["user_id"], "300000001");
        assert_eq!(body["transport"]["method"], "websocket");
        assert_eq!(
            body["transport"]["session_id"],
            "AgoQHR3s6Mb4T8GFB1l3DlPfiRIGY2VsbC1h"
        );
        types.push(body["type"].as_str().unwrap().to_owned());
    }
    assert_eq!(types, pa_twitch_chat::eventsub::CHAT_SUBSCRIPTIONS);

    assert_eq!(message_ids(&events), ["e5f6a7b8-0003-4a5b-9c1d-000000000003"]);
    assert!(events
        .iter()
        .any(|e| matches!(e, ChatEvent::MessageDeleted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, ChatEvent::ChatCleared { .. })));
    let st = statuses(&events);
    assert_eq!(st[0], SourceStatus::Connected);
    assert_eq!(
        st[1],
        SourceStatus::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            reason: "connection closed by Twitch".to_owned(),
        }
    );
    assert!(matches!(st[2], SourceStatus::Stopped { .. }));
    assert_eq!(h.connector.urls(), [ES_URL, RECONNECT_URL, ES_URL]);
}

#[tokio::test]
async fn eventsub_keepalive_timeout_reconnects_and_resubscribes() {
    let h = es_harness();
    h.push_subscriptions();
    h.push_subscriptions();
    h.connector.push(Script::hanging([es!("welcome")]));
    h.connector
        .push(Script::closing([es!("welcome"), es!("chat_message")]));
    let source = h.spawn(1);
    let events = drain(&source).await;
    let st = statuses(&events);
    assert_eq!(st[0], SourceStatus::Connected);
    assert_eq!(
        st[1],
        SourceStatus::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            reason: "keepalive timeout".to_owned(),
        }
    );
    assert_eq!(st[2], SourceStatus::Connected);
    assert_eq!(message_ids(&events).len(), 1);
    assert_eq!(h.transport.requests().len(), 12, "new session, new subscriptions");
}

#[tokio::test]
async fn eventsub_stops_on_revoked_authorization() {
    let h = es_harness();
    h.push_subscriptions();
    h.connector
        .push(Script::hanging([es!("welcome"), es!("revocation")]));
    let source = h.spawn(5);
    let st = statuses(&drain(&source).await);
    assert_eq!(st.len(), 2);
    assert!(matches!(
        &st[1],
        SourceStatus::Stopped { reason } if reason.contains("authorization_revoked")
    ));
    assert_eq!(h.connector.urls().len(), 1, "no reconnect after revocation");
}

#[tokio::test]
async fn eventsub_stops_when_token_is_rejected() {
    let h = es_harness();
    h.transport.push_fixture(helix!("unauthorized"));
    h.connector.push(Script::hanging([es!("welcome")]));
    let source = h.spawn(5);
    let st = statuses(&drain(&source).await);
    assert_eq!(st.len(), 1);
    assert!(matches!(&st[0], SourceStatus::Stopped { .. }));
}

#[tokio::test]
async fn eventsub_optional_feed_failure_is_a_notice() {
    let h = es_harness();
    h.transport.push_fixture(helix!("subscription_created")); // chat.message
    h.transport.push_fixture(helix!("subscription_forbidden")); // notification
    for _ in 0..4 {
        h.transport.push_fixture(helix!("subscription_created"));
    }
    h.connector
        .push(Script::closing([es!("welcome"), es!("chat_message")]));
    let source = h.spawn(0);
    let events = drain(&source).await;
    assert!(events.iter().any(|e| matches!(
        e,
        ChatEvent::Notice { notice_id: Some(id), text, .. }
            if id == "subscription_failed" && text.contains("channel.chat.notification")
    )));
    assert_eq!(message_ids(&events).len(), 1);
}

#[tokio::test]
async fn eventsub_sends_through_helix() {
    let h = es_harness();
    // The socket fails at once; sending does not depend on it.
    let source = h.spawn(0);
    h.transport.push_fixture(helix!("message_sent"));
    h.transport.push_fixture(helix!("message_dropped"));

    let sent = source
        .send_message(
            "100000001",
            "  hello chat  ",
            Some("e5f6a7b8-0003-4a5b-9c1d-000000000003"),
        )
        .await
        .unwrap();
    assert!(sent.is_sent);
    assert_eq!(sent.message_id, "e5f6a7b8-0100-4a5b-9c1d-000000000100");

    let dropped = source.send_message("100000001", "hello chat", None).await.unwrap();
    assert!(!dropped.is_sent);
    assert_eq!(dropped.drop_reason.unwrap().code, "msg_duplicate");

    let requests = h.transport.requests();
    assert_eq!(requests[0].url, "https://helix.test/helix/chat/messages");
    let body: serde_json::Value =
        serde_json::from_slice(requests[0].body.as_ref().unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "broadcaster_id": "100000001",
            "sender_id": "300000001",
            "message": "hello chat",
            "reply_parent_message_id": "e5f6a7b8-0003-4a5b-9c1d-000000000003",
        })
    );
    let body: serde_json::Value =
        serde_json::from_slice(requests[1].body.as_ref().unwrap()).unwrap();
    assert!(body.get("reply_parent_message_id").is_none());

    assert!(matches!(
        source.send_message("100000001", "   ", None).await,
        Err(ChatError::InvalidArgument(_))
    ));
    let long = "a".repeat(501);
    assert!(matches!(
        source.send_message("100000001", &long, None).await,
        Err(ChatError::InvalidArgument(_))
    ));
    assert_eq!(h.transport.requests().len(), 2);
}
