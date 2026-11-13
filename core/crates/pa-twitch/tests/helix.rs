// SPDX-License-Identifier: MPL-2.0
//! Helix client against recorded responses (`tests/fixtures/`). No test
//! touches the network: the fixture transport fails on any unplanned call.

use std::sync::Arc;
use std::time::Duration;

use pa_twitch::testing::{FixtureTransport, ManualClock};
use pa_twitch::{
    AccessToken, ClientId, ClipsQuery, Cursor, Error, HelixClient, HelixConfig, MAX_CACHE_TTL,
    Page, SearchChannelsQuery, StreamsQuery, VideoType, VideosQuery, collect_pages,
};

const START: u64 = 1_800_000_000;
const TOKEN: &str = "test-user-token-not-real";

macro_rules! fx {
    ($name:literal) => {
        include_str!(concat!("fixtures/", $name, ".json"))
    };
}

struct Harness {
    client: HelixClient,
    transport: Arc<FixtureTransport>,
    clock: Arc<ManualClock>,
}

fn harness_with(edit: impl FnOnce(&mut HelixConfig)) -> Harness {
    let transport = Arc::new(FixtureTransport::new());
    let clock = Arc::new(ManualClock::at_unix(START));
    let mut config = HelixConfig::new(ClientId::new("testclientid0000000000000000000").unwrap());
    config.base_url = "https://helix.test/helix".to_owned();
    edit(&mut config);
    let client = HelixClient::with_clock(config, transport.clone(), clock.clone());
    client.set_access_token(AccessToken::new(TOKEN));
    Harness {
        client,
        transport,
        clock,
    }
}

fn harness() -> Harness {
    harness_with(|_| {})
}

impl Harness {
    fn urls(&self) -> Vec<String> {
        self.transport
            .requests()
            .into_iter()
            .map(|r| r.url.replace("https://helix.test/helix/", ""))
            .collect()
    }
}

#[tokio::test]
async fn users_are_batched_sent_with_auth_and_cached() {
    let h = harness();
    h.transport.push_fixture(fx!("users"));

    let users = h
        .client
        .get_users(&["1001", "1002"], &["Nordlicht"])
        .await
        .unwrap();
    assert_eq!(users.len(), 3);
    assert_eq!(users[0].display_name, "Pixel_Fox");
    assert!(users[0].profile_image_url.ends_with("profile_image-300x300.png"));
    assert_eq!(h.urls(), ["users?id=1001&id=1002&login=nordlicht"]);

    let request = &h.transport.requests()[0];
    assert_eq!(
        request.header("client-id"),
        Some("testclientid0000000000000000000")
    );
    assert_eq!(
        request.header("authorization").unwrap(),
        format!("Bearer {TOKEN}")
    );
    assert!(request.header("user-agent").unwrap().starts_with("Kinolune/"));
    assert!(
        !request
            .header("user-agent")
            .unwrap()
            .to_lowercase()
            .contains("twitch")
    );

    // Second lookup (by id and by login) is served from the cache.
    let again = h.client.get_users(&["1003"], &["pixel_fox"]).await.unwrap();
    assert_eq!(again.len(), 2);
    assert_eq!(h.transport.requests().len(), 1);

    // After 24 hours the cache must not answer any more.
    h.clock.advance(MAX_CACHE_TTL);
    let err = h.client.get_users(&["1003"], &[]).await.unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "{err:?}");
    assert_eq!(h.transport.requests().len(), 2);
}

#[tokio::test]
async fn current_user() {
    let h = harness();
    h.transport.push_fixture(fx!("users"));
    let me = h.client.get_current_user().await.unwrap();
    assert_eq!(me.login, "pixel_fox");
    assert_eq!(h.urls(), ["users"]);
}

#[tokio::test]
async fn no_token_means_no_request() {
    let h = harness();
    h.client.sign_out();
    assert!(!h.client.is_signed_in());
    let err = h.client.get_users(&["1001"], &[]).await.unwrap_err();
    assert_eq!(err, Error::MissingToken);
    assert!(h.transport.requests().is_empty());
}

#[tokio::test]
async fn followed_overview_joins_follows_profiles_and_live_streams() {
    let h = harness();
    h.transport
        .push_fixture(fx!("channels_followed_p1"))
        .push_fixture(fx!("channels_followed_p2"))
        .push_fixture(fx!("streams_followed"))
        .push_fixture(fx!("users"));

    let list = h.client.followed_overview("1002", 500).await.unwrap();

    let names: Vec<&str> = list.iter().map(|s| s.display_name()).collect();
    // Live first by viewers, then offline; the duplicate from page 2 is gone.
    assert_eq!(names, ["Pixel_Fox", "Nordlicht", "Quiet_Owl"]);
    assert!(list[0].is_live() && list[1].is_live() && !list[2].is_live());
    assert_eq!(list[0].stream.as_ref().unwrap().game_name, "Super Mario 64");
    assert_eq!(
        list[0].stream.as_ref().unwrap().thumbnail(320, 180),
        "https://static-cdn.jtvnw.net/previews-ttv/live_user_pixel_fox-320x180.jpg"
    );
    assert!(list[2].avatar_url().unwrap().contains("quiet_owl"));
    assert!(list[1].stream.as_ref().unwrap().tags.is_empty()); // `null` tags

    assert_eq!(
        h.urls(),
        [
            "channels/followed?user_id=1002&first=100",
            "channels/followed?user_id=1002&first=100&after=eyJiIjpudWxsLCJhIjp7Ik9mZnNldCI6Mn19",
            "streams/followed?user_id=1002&first=100",
            "users?id=1003&id=1002&id=1001",
        ]
    );
}

#[tokio::test]
async fn followed_channels_page_reports_total() {
    let h = harness();
    h.transport.push_fixture(fx!("channels_followed_p1"));
    let page = h
        .client
        .get_followed_channels("1002", Some(500), None)
        .await
        .unwrap();
    assert_eq!(page.total, Some(3));
    assert_eq!(page.items.len(), 2);
    assert!(page.next.is_some());
    // `first` is clamped to Helix's maximum.
    assert_eq!(h.urls(), ["channels/followed?user_id=1002&first=100"]);
}

#[tokio::test]
async fn http_429_waits_for_reset_and_retries() {
    let h = harness();
    h.transport
        .push_fixture(fx!("rate_limited"))
        .push_fixture(fx!("users"));

    let users = h.client.get_users(&["1001"], &[]).await.unwrap();
    assert_eq!(users[0].id, "1001");
    // Ratelimit-Reset is 5 s after "now".
    assert_eq!(h.clock.sleeps(), [Duration::from_secs(5)]);
    assert_eq!(h.transport.requests().len(), 2);
    assert_eq!(h.client.rate_limit().remaining, Some(799));
}

#[tokio::test]
async fn http_429_gives_up_after_max_retries() {
    let h = harness_with(|c| c.max_retries = 2);
    for _ in 0..3 {
        h.transport.push_fixture(fx!("rate_limited"));
    }
    let err = h.client.get_users(&["1001"], &[]).await.unwrap_err();
    assert!(matches!(err, Error::RateLimited { .. }), "{err:?}");
    assert_eq!(h.transport.requests().len(), 3);
    assert_eq!(h.transport.remaining(), 0);
}

#[tokio::test]
async fn http_429_with_far_reset_fails_fast() {
    let h = harness_with(|c| c.max_wait = Duration::from_secs(2));
    h.transport.push_fixture(fx!("rate_limited"));
    let err = h.client.get_users(&["1001"], &[]).await.unwrap_err();
    assert_eq!(
        err,
        Error::RateLimited {
            retry_after: Duration::from_secs(5)
        }
    );
    assert!(h.clock.sleeps().is_empty());
}

#[tokio::test]
async fn empty_bucket_delays_the_next_request() {
    let h = harness();
    h.transport
        .push_fixture(fx!("bucket_empty_users"))
        .push_fixture(fx!("users"));

    h.client.get_users(&["1002"], &[]).await.unwrap();
    assert!(h.clock.sleeps().is_empty());
    assert_eq!(h.client.rate_limit().remaining, Some(0));

    // Remaining = 0 and reset in 30 s: wait, then send (no 429 provoked).
    h.client.get_users(&["1001"], &[]).await.unwrap();
    assert_eq!(h.clock.sleeps(), [Duration::from_secs(30)]);
    assert_eq!(h.transport.requests().len(), 2);
}

#[tokio::test]
async fn unauthorized_is_reported_for_refresh() {
    let h = harness();
    h.transport.push_fixture(fx!("unauthorized"));
    let err = h.client.get_users(&["1001"], &[]).await.unwrap_err();
    assert_eq!(
        err,
        Error::Unauthorized {
            message: "Invalid OAuth token".to_owned()
        }
    );
    assert_eq!(h.transport.requests().len(), 1);
}

#[tokio::test]
async fn transient_5xx_is_retried_with_backoff() {
    let h = harness();
    h.transport
        .push_fixture(fx!("server_error"))
        .push_fixture(fx!("server_error"))
        .push_fixture(fx!("users"));
    h.client.get_users(&["1001"], &[]).await.unwrap();
    assert_eq!(
        h.clock.sleeps(),
        [Duration::from_secs(1), Duration::from_secs(2)]
    );
}

#[tokio::test]
async fn network_failure_is_a_transport_error() {
    let h = harness();
    h.transport.push_error("connection reset");
    let err = h.client.get_users(&["1001"], &[]).await.unwrap_err();
    assert_eq!(err, Error::Transport("connection reset".to_owned()));
}

#[tokio::test]
async fn streams_by_category() {
    let h = harness();
    h.transport.push_fixture(fx!("streams"));
    let page = h
        .client
        .get_streams(&StreamsQuery {
            game_ids: vec!["1229".to_owned()],
            languages: vec!["en".to_owned()],
            first: Some(20),
            ..StreamsQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].viewer_count, 4120);
    assert_eq!(page.items[0].kind, "live");
    assert_eq!(
        page.next,
        Some(Cursor("eyJiIjp7IkN1cnNvciI6IjEifX0".to_owned()))
    );
    assert_eq!(h.urls(), ["streams?game_id=1229&language=en&first=20"]);
}

#[tokio::test]
async fn channel_info() {
    let h = harness();
    h.transport.push_fixture(fx!("channels"));
    let channels = h.client.get_channels(&["1001"]).await.unwrap();
    assert_eq!(channels[0].game_name, "Super Mario 64");
    assert_eq!(channels[0].tags, ["English", "Speedrun"]);
    assert_eq!(h.urls(), ["channels?broadcaster_id=1001"]);
}

#[tokio::test]
async fn vods_of_a_channel() {
    let h = harness();
    h.transport.push_fixture(fx!("videos"));
    let page = h
        .client
        .get_videos(&VideosQuery {
            video_type: Some(VideoType::Archive),
            sort: Some("time".to_owned()),
            first: Some(2),
            ..VideosQuery::by_user("1001")
        })
        .await
        .unwrap();
    assert_eq!(
        h.urls(),
        ["videos?user_id=1001&type=archive&sort=time&first=2"]
    );

    let vod = &page.items[0];
    assert_eq!(vod.kind, VideoType::Archive);
    assert_eq!(vod.stream_id.as_deref(), Some("40952000001"));
    assert_eq!(vod.duration(), Some(Duration::from_secs(3 * 3600 + 8 * 60 + 33)));
    assert_eq!(vod.muted_segments[0].offset, 120);
    assert!(vod.thumbnail(320, 180).unwrap().ends_with("thumb0-320x180.jpg"));

    let highlight = &page.items[1];
    assert_eq!(highlight.kind, VideoType::Highlight);
    assert_eq!(highlight.stream_id, None);
    assert!(highlight.muted_segments.is_empty());
    assert_eq!(highlight.thumbnail(320, 180), None); // still processing
    assert!(page.next.is_some());
}

#[tokio::test]
async fn videos_need_exactly_one_selector() {
    let h = harness();
    let none = h.client.get_videos(&VideosQuery::default()).await;
    assert!(matches!(none, Err(Error::InvalidArgument(_))));
    let two = h
        .client
        .get_videos(&VideosQuery {
            game_id: Some("1229".to_owned()),
            ..VideosQuery::by_user("1001")
        })
        .await;
    assert!(matches!(two, Err(Error::InvalidArgument(_))));
    assert!(h.transport.requests().is_empty());
}

#[tokio::test]
async fn videos_by_id_send_no_paging() {
    let h = harness();
    h.transport.push_fixture(fx!("videos"));
    h.client
        .get_videos(&VideosQuery {
            first: Some(10),
            ..VideosQuery::by_ids(["2200000001", "2200000002"])
        })
        .await
        .unwrap();
    assert_eq!(h.urls(), ["videos?id=2200000001&id=2200000002"]);
}

#[tokio::test]
async fn clips_of_a_channel() {
    let h = harness();
    h.transport.push_fixture(fx!("clips"));
    let page = h
        .client
        .get_clips(&ClipsQuery {
            broadcaster_id: Some("1001".to_owned()),
            started_at: Some("2026-11-13T00:00:00Z".to_owned()),
            is_featured: Some(true),
            ..ClipsQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(
        h.urls(),
        ["clips?broadcaster_id=1001&started_at=2026-11-13T00%3A00%3A00Z&is_featured=true"]
    );
    assert_eq!(page.items[0].vod_offset, Some(5400));
    assert!((page.items[0].duration - 28.4).abs() < f64::EPSILON);
    assert_eq!(page.items[1].vod_offset, None);
    assert!(page.items[1].embed_url.starts_with("https://clips.twitch.tv/embed"));
    assert_eq!(page.next, None);
}

#[tokio::test]
async fn categories_top_lookup_and_search() {
    let h = harness();
    h.transport
        .push_fixture(fx!("games_top"))
        .push_fixture(fx!("search_categories"));

    let top = h.client.get_top_categories(Some(2), None).await.unwrap();
    assert_eq!(top.items[0].name, "Just Chatting");
    assert_eq!(top.items[0].igdb_id, None); // "" from Helix
    assert_eq!(top.items[1].igdb_id.as_deref(), Some("1074"));
    assert_eq!(
        top.items[1].box_art(285, 380),
        "https://static-cdn.jtvnw.net/ttv-boxart/1229-285x380.jpg"
    );

    // Lookup by id is answered by the cache filled from games/top.
    let cached = h.client.get_categories(&["1229"], &[]).await.unwrap();
    assert_eq!(cached[0].name, "Super Mario 64");

    let found = h
        .client
        .search_categories("mario 64", None, None)
        .await
        .unwrap();
    assert_eq!(found.items[0].id, "1229");
    assert_eq!(found.items[0].igdb_id, None);

    assert_eq!(
        h.urls(),
        ["games/top?first=2", "search/categories?query=mario%2064"]
    );
}

#[tokio::test]
async fn categories_by_name() {
    let h = harness();
    h.transport.push_fixture(fx!("games"));
    let games = h
        .client
        .get_categories(&[], &["Super Mario 64"])
        .await
        .unwrap();
    assert_eq!(games[0].id, "1229");
    assert_eq!(h.urls(), ["games?name=Super%20Mario%2064"]);
}

#[tokio::test]
async fn channel_search() {
    let h = harness();
    h.transport.push_fixture(fx!("search_channels"));
    let page = h
        .client
        .search_channels(&SearchChannelsQuery {
            query: "pixel fox".to_owned(),
            live_only: false,
            first: Some(5),
            after: None,
        })
        .await
        .unwrap();
    assert!(page.items[0].is_live);
    assert!(page.items[0].started_at.is_some());
    assert!(!page.items[1].is_live);
    assert_eq!(page.items[1].started_at, None); // "" when offline
    assert_eq!(h.urls(), ["search/channels?query=pixel%20fox&first=5"]);

    let empty = h
        .client
        .search_channels(&SearchChannelsQuery::default())
        .await;
    assert!(matches!(empty, Err(Error::InvalidArgument(_))));
}

#[tokio::test]
async fn schedule_and_missing_schedule() {
    let h = harness();
    h.transport
        .push_fixture(fx!("schedule"))
        .push_fixture(fx!("schedule_not_found"));

    let page = h
        .client
        .get_schedule("1001", None, None)
        .await
        .unwrap()
        .unwrap();
    let segment = &page.schedule.segments[0];
    assert_eq!(segment.title, "Sunday any%");
    assert_eq!(segment.category.as_ref().unwrap().name, "Super Mario 64");
    assert!(segment.is_recurring);
    assert_eq!(page.schedule.vacation, None);
    assert_eq!(page.next, None);

    assert_eq!(h.client.get_schedule("1002", None, None).await.unwrap(), None);
}

#[tokio::test]
async fn emotes_badges_colours_and_chat_settings() {
    let h = harness();
    h.transport
        .push_fixture(fx!("emotes_channel"))
        .push_fixture(fx!("badges_global"))
        .push_fixture(fx!("chat_color"))
        .push_fixture(fx!("chat_settings"));

    let emotes = h.client.get_channel_emotes("1001").await.unwrap();
    assert_eq!(emotes.emotes.len(), 2);
    assert_eq!(emotes.emotes[0].tier.as_deref(), Some("1000"));
    assert_eq!(emotes.emotes[1].tier, None);
    assert_eq!(
        emotes.url(&emotes.emotes[0], true, "3.0"),
        "https://static-cdn.jtvnw.net/emoticons/v2/304456832/animated/dark/3.0"
    );
    assert_eq!(
        emotes.url(&emotes.emotes[1], false, "1.0"),
        "https://static-cdn.jtvnw.net/emoticons/v2/emotesv2_1a2b3c/static/light/1.0"
    );

    let badges = h.client.get_global_badges().await.unwrap();
    assert_eq!(badges[0].set_id, "moderator");
    assert_eq!(badges[0].versions[0].title, "Moderator");

    let colours = h
        .client
        .get_user_chat_colors(&["1001", "1002"])
        .await
        .unwrap();
    assert_eq!(colours[0].color.as_deref(), Some("#9146FF"));
    assert_eq!(colours[1].color, None);

    let settings = h.client.get_chat_settings("1001").await.unwrap();
    assert!(settings.slow_mode);
    assert_eq!(settings.slow_mode_wait_time, Some(30));
    assert!(settings.follower_mode);
    assert!(!settings.subscriber_mode);

    assert_eq!(
        h.urls(),
        [
            "chat/emotes?broadcaster_id=1001",
            "chat/badges/global",
            "chat/color?user_id=1001&user_id=1002",
            "chat/settings?broadcaster_id=1001",
        ]
    );
}

#[tokio::test]
async fn sign_out_forgets_cached_twitch_data() {
    let h = harness();
    h.transport.push_fixture(fx!("users"));
    h.client.get_users(&["1001"], &[]).await.unwrap();
    h.client.sign_out();
    h.client.set_access_token(AccessToken::new(TOKEN));
    // Not answered from the cache any more: needs a new (absent) response.
    assert!(h.client.get_users(&["1001"], &[]).await.is_err());
    assert_eq!(h.transport.requests().len(), 2);
}

#[tokio::test]
async fn tokens_never_show_in_debug_output() {
    let h = harness();
    h.transport.push_fixture(fx!("users"));
    h.client.get_users(&["1001"], &[]).await.unwrap();
    let request = format!("{:?}", h.transport.requests()[0]);
    assert!(!request.contains(TOKEN), "{request}");
    assert!(request.contains("<redacted>"));
    assert!(!format!("{:?}", AccessToken::new(TOKEN)).contains(TOKEN));
    assert!(!format!("{:?}", h.client).contains(TOKEN));
}

#[test]
fn client_id_is_validated() {
    assert!(ClientId::new("").is_err());
    assert!(ClientId::new("abc def").is_err());
    assert_eq!(ClientId::new(" abc123 ").unwrap().as_str(), "abc123");
}

#[tokio::test]
async fn collect_pages_stops_on_a_repeated_cursor() {
    let mut calls = 0;
    let items = collect_pages(100, |_cursor| {
        calls += 1;
        let page: Page<pa_twitch::Category> = Page {
            items: vec![pa_twitch::Category {
                id: format!("{calls}"),
                name: String::new(),
                box_art_url: String::new(),
                igdb_id: None,
            }],
            next: Some(Cursor("same".to_owned())),
            total: None,
        };
        async move { Ok(page) }
    })
    .await
    .unwrap();
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn collect_pages_respects_limit() {
    let h = harness();
    h.transport.push_fixture(fx!("channels_followed_p1"));
    let items = h.client.all_followed_channels("1002", 1).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(h.transport.requests().len(), 1);
}
