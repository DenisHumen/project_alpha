// SPDX-License-Identifier: MPL-2.0
//! AniList client against the synthetic cassette
//! `fixtures/anilist/cassette.jsonl`, plus load tests of its rate limit.
//! No network: responses come from the cassette or a generated transport,
//! time from a manual clock.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{ClockedTransport, GetOnly, max_in_window, ok};
use pa_metadata::anilist::{AniListClient, AniListConfig};
use pa_metadata::cache::{MemoryCache, ProviderCache};
use pa_metadata::clock::{Clock, ManualClock};
use pa_metadata::fetch::FetchError;
use pa_metadata::replay::ReplayTransport;
use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::{Value, json};

const CASSETTE: &str = include_str!("fixtures/anilist/cassette.jsonl");
const T0: u64 = 1_800_000_000;
const DAY: u64 = 24 * 3600;
const MINUTE: Duration = Duration::from_secs(60);

struct Fixture {
    client: AniListClient,
    transport: Arc<ReplayTransport>,
    cache: Arc<MemoryCache>,
    clock: Arc<ManualClock>,
}

fn fixture() -> Fixture {
    let transport = Arc::new(ReplayTransport::from_jsonl(CASSETTE).unwrap());
    let cache = Arc::new(MemoryCache::default());
    let clock = Arc::new(ManualClock::at_unix(T0));
    let client = AniListClient::new(
        AniListConfig::default(),
        transport.clone(),
        cache.clone(),
        clock.clone(),
    );
    Fixture {
        client,
        transport,
        cache,
        clock,
    }
}

impl Fixture {
    fn requests(&self) -> usize {
        self.transport.requests().len()
    }
}

fn req(endpoint: &str, params: &[(&str, &str)]) -> ProviderRequest {
    params
        .iter()
        .fold(ProviderRequest::new("anilist", endpoint), |r, (k, v)| {
            r.param(k, *v)
        })
}

// --- Queries -----------------------------------------------------------------

#[test]
fn search_filters_adult_and_is_cached() {
    let f = fixture();
    let found = f.client.search("  Kitsune Relay ").unwrap();
    let ids: Vec<_> = found.iter().map(|m| (m.id, m.id_mal)).collect();
    assert_eq!(
        ids,
        vec![
            (700001, Some(600001)),
            (700003, Some(600003)),
            (700002, Some(600002))
        ],
        "the 18+ OVA is dropped"
    );
    let first = &found[0];
    assert_eq!(first.title.romaji.as_deref(), Some("Kitsune Relay"));
    assert_eq!(first.title.native.as_deref(), Some("キツネリレー"));
    assert_eq!(first.year(), Some(2021));
    assert_eq!(
        first.all_titles(),
        vec!["Kitsune Relay", "Fox Relay", "キツネリレー", "KitsuRe"],
        "titles and synonyms, deduplicated"
    );
    assert!(found[1].is_movie());
    assert_eq!(f.requests(), 1);

    f.client.search("Kitsune Relay").unwrap();
    assert_eq!(f.requests(), 1, "second search comes from the cache");
    f.clock.advance(Duration::from_secs(DAY + 1));
    f.client.search("Kitsune Relay").unwrap();
    assert_eq!(f.requests(), 2, "searches stay fresh for a day");
}

#[test]
fn media_by_id_and_mal_id_with_relations() {
    let f = fixture();
    let m = f.client.media(700001).unwrap();
    assert_eq!(m.id_mal, Some(600001));
    assert_eq!(m.status.as_deref(), Some("FINISHED"));
    assert!(!m.is_ongoing());
    let edges = &m.relations.as_ref().unwrap().edges;
    let rel: Vec<_> = edges
        .iter()
        .map(|e| (e.relation_type.as_deref().unwrap(), e.node.id, e.node.is_anime()))
        .collect();
    assert_eq!(
        rel,
        vec![
            ("SEQUEL", 700002, true),
            ("SIDE_STORY", 700003, true),
            ("SOURCE", 710001, false),
        ],
        "the 18+ spin-off is dropped"
    );

    let by_mal = f.client.media_by_mal(600001).unwrap();
    assert_eq!(by_mal.id, 700001);

    let sequel = f.client.media(700002).unwrap();
    assert!(sequel.is_ongoing());
    assert_eq!(sequel.next_airing_episode.unwrap().episode, 5);
}

#[test]
fn adult_and_missing_media_are_not_found() {
    let f = fixture();
    assert!(matches!(
        f.client.media(700009),
        Err(FetchError::NotFound { .. })
    ));
    match f.client.media(799999) {
        Err(FetchError::NotFound { provider, what }) => {
            assert_eq!(provider, "anilist");
            assert_eq!(what, "Not Found.");
        }
        other => panic!("{other:?}"),
    }

    let adult_ok = AniListClient::new(
        AniListConfig {
            include_adult: true,
            ..AniListConfig::default()
        },
        Arc::new(ReplayTransport::from_jsonl(CASSETTE).unwrap()),
        Arc::new(MemoryCache::default()),
        Arc::new(ManualClock::at_unix(T0)),
    );
    assert!(adult_ok.media(700009).unwrap().adult());
    assert_eq!(
        adult_ok.media(700001).unwrap().relations.unwrap().edges.len(),
        4
    );
}

#[test]
fn ttl_depends_on_airing_status() {
    let f = fixture();
    f.client.media(700001).unwrap();
    f.client.media(700002).unwrap();
    assert_eq!(f.requests(), 2);
    f.clock.advance(Duration::from_secs(2 * DAY));
    f.client.media(700001).unwrap();
    assert_eq!(f.requests(), 2, "finished: fresh for 14 days");
    f.client.media(700002).unwrap();
    assert_eq!(f.requests(), 3, "airing: fresh for one day");
    f.clock.advance(Duration::from_secs(13 * DAY));
    f.client.media(700001).unwrap();
    assert_eq!(f.requests(), 4);
}

#[test]
fn stale_entry_is_served_offline_and_purged() {
    let f = fixture();
    f.client.media(700001).unwrap();
    f.clock.advance(Duration::from_secs(20 * DAY));
    let offline = AniListClient::new(
        AniListConfig::default(),
        Arc::new(ReplayTransport::default()),
        f.cache.clone(),
        f.clock.clone(),
    );
    assert_eq!(offline.media(700001).unwrap().id, 700001, "stale fallback");
    assert!(matches!(
        offline.media(700002),
        Err(FetchError::Transport(_))
    ));
    assert_eq!(
        ProviderError::from(offline.media(700002).unwrap_err()),
        ProviderError::Unavailable("not in cassette: / graphql:Media{\"id\":700002}".into())
    );

    f.clock.advance(Duration::from_secs(200 * DAY));
    assert!(offline.media(700001).is_err(), "nothing past six months");
    assert_eq!(offline.purge_expired(), 1);
    f.client.search("Kitsune Relay").unwrap();
    assert_eq!(f.client.purge_all(), 1);
    assert!(f.cache.is_empty());
}

// --- Errors and retries ------------------------------------------------------

#[test]
fn retry_after_of_a_minute_is_honoured() {
    let f = fixture();
    let found = f.client.search("Rate Limited").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(f.requests(), 2);
    let sleeps = f.clock.sleeps();
    assert_eq!(sleeps[0], MINUTE, "not capped at 30 s: {sleeps:?}");
}

#[test]
fn persistent_429_becomes_rate_limited() {
    let f = fixture();
    let err = f.client.search("Always Busy").unwrap_err();
    assert_eq!(
        err,
        FetchError::RateLimited {
            provider: "anilist",
            attempts: 4,
            retry_after: Some(MINUTE),
        }
    );
    assert_eq!(f.requests(), 4);
    assert_eq!(
        ProviderError::from(err),
        ProviderError::RateLimited {
            retry_after_secs: Some(60)
        }
    );
    assert!(f.cache.is_empty(), "errors are not cached");
}

#[test]
fn server_errors_are_retried_and_graphql_errors_reported() {
    let f = fixture();
    assert_eq!(f.client.search("Server Hiccup").unwrap()[0].id, 700003);
    assert_eq!(f.clock.sleeps()[0], Duration::from_secs(2));

    match f.client.search("Bad Query").unwrap_err() {
        FetchError::Http {
            status, message, ..
        } => {
            assert_eq!(status, 400);
            assert!(message.starts_with("Validation error"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let n = f.requests();
    match f.client.search("Partial Error").unwrap_err() {
        FetchError::Http {
            status, message, ..
        } => assert_eq!((status, message.as_str()), (500, "Internal error in a resolver")),
        other => panic!("{other:?}"),
    }
    assert_eq!(f.requests(), n + 1, "a 200 with errors is not retried");
    f.client.search("Partial Error").unwrap_err();
    assert_eq!(f.requests(), n + 2, "… and not cached");
}

#[test]
fn transport_without_post_is_unavailable() {
    let client = AniListClient::new(
        AniListConfig::default(),
        Arc::new(GetOnly),
        Arc::new(MemoryCache::default()),
        Arc::new(ManualClock::at_unix(T0)),
    );
    let err = client.answer(&req("search", &[("query", "x")])).unwrap_err();
    assert!(
        matches!(&err, ProviderError::Unavailable(m) if m.contains("POST is not supported")),
        "{err:?}"
    );
}

// --- Rate limit --------------------------------------------------------------

#[test]
fn quota_headers_adapt_the_limit_and_pause() {
    let f = fixture();
    assert_eq!(f.client.rate_per_minute(), 30);
    f.client.search("Quota Ninety").unwrap();
    assert_eq!(f.client.rate_per_minute(), 90);
    f.client.search("Quota Drained").unwrap();
    assert_eq!(f.client.rate_per_minute(), 30, "back to degraded mode");
    assert!(f.clock.sleeps().is_empty());
    f.client.search("Kitsune Relay").unwrap();
    assert_eq!(
        f.clock.sleeps()[0],
        Duration::from_secs(40),
        "waits for X-RateLimit-Reset"
    );
}

fn load_client(
    limit_header: Option<&'static str>,
    config: AniListConfig,
) -> (AniListClient, Arc<ClockedTransport>, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::at_unix(T0));
    let transport = Arc::new(ClockedTransport::new(clock.clone(), move |_, _| {
        let mut headers = vec![];
        if let Some(l) = limit_header {
            headers.push(("x-ratelimit-limit", l));
        }
        ok(r#"{"data":{"Page":{"media":[]}}}"#, &headers)
    }));
    let client = AniListClient::new(
        config,
        transport.clone(),
        Arc::new(MemoryCache::default()),
        clock.clone(),
    );
    (client, transport, clock)
}

#[test]
fn load_stays_within_30_per_minute() {
    let (client, transport, _) = load_client(Some("30"), AniListConfig::default());
    for i in 0..150 {
        client.search(&format!("load {i}")).unwrap();
    }
    let times = transport.times();
    assert_eq!(times.len(), 150);
    let peak = max_in_window(&times, MINUTE);
    assert!(peak <= 30, "{peak} requests in one minute");
    assert!(peak >= 28, "limiter is not needlessly slow: {peak}");
}

#[test]
fn load_adapts_to_90_per_minute_and_is_capped() {
    let (client, transport, clock) = load_client(Some("90"), AniListConfig::default());
    for i in 0..300 {
        client.search(&format!("load {i}")).unwrap();
    }
    assert_eq!(client.rate_per_minute(), 90);
    let peak = max_in_window(&transport.times(), MINUTE);
    assert!((85..=90).contains(&peak), "{peak}");
    assert!(clock.monotonic() < Duration::from_secs(300), "faster than 30/min");

    // AniList announcing more than the configured ceiling changes nothing.
    let (client, transport, _) = load_client(Some("120"), AniListConfig::default());
    for i in 0..200 {
        client.search(&format!("load {i}")).unwrap();
    }
    assert_eq!(client.rate_per_minute(), 90);
    assert!(max_in_window(&transport.times(), MINUTE) <= 90);

    // Without adaptation the starting limit holds.
    let (client, transport, _) = load_client(
        Some("90"),
        AniListConfig {
            adaptive: false,
            ..AniListConfig::default()
        },
    );
    for i in 0..100 {
        client.search(&format!("load {i}")).unwrap();
    }
    assert_eq!(client.rate_per_minute(), 30);
    assert!(max_in_window(&transport.times(), MINUTE) <= 30);
}

#[test]
fn sends_a_graphql_query_with_variables() {
    let (client, transport, _) = load_client(None, AniListConfig::default());
    client.search("Kitsune").unwrap();
    client.media_by_mal(600001).unwrap_err();
    let calls = transport.calls.lock().unwrap();
    let (_, request, body) = &calls[0];
    assert_eq!(request.url(), "https://graphql.anilist.co/");
    let body: Value = serde_json::from_str(body.as_deref().unwrap()).unwrap();
    assert!(body["query"].as_str().unwrap().starts_with("query Search("));
    assert!(body["query"].as_str().unwrap().contains("type: ANIME"));
    assert_eq!(
        body["variables"],
        json!({"search": "Kitsune", "perPage": 10, "isAdult": false})
    );
    let body: Value = serde_json::from_str(calls[1].2.as_deref().unwrap()).unwrap();
    assert_eq!(body["variables"], json!({"idMal": 600001}));
    assert!(body["query"].as_str().unwrap().contains("relations"));
}

// --- Provider ----------------------------------------------------------------

#[test]
fn neutral_search_normalizes_and_filters() {
    let mut f = fixture();
    let all = f
        .client
        .call(&req("search", &[("query", "Kitsune Relay")]))
        .unwrap();
    let results = all["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(
        results[0],
        json!({
            "kind": "show",
            "title": "Kitsune Relay",
            "original_title": "キツネリレー",
            "titles": ["Kitsune Relay", "Fox Relay", "キツネリレー", "KitsuRe"],
            "year": 2021,
            "format": "TV",
            "episodes": 12,
            "popularity": 5400,
            "ids": {"anilist": "700001", "mal": "600001"},
        })
    );

    let movies = f
        .client
        .call(&req("search", &[("query", "Kitsune Relay"), ("kind", "movie")]))
        .unwrap();
    assert_eq!(movies["results"][0]["ids"]["anilist"], "700003");
    assert_eq!(movies["results"].as_array().unwrap().len(), 1);
    let y2023 = f
        .client
        .call(&req("search", &[("query", "Kitsune Relay"), ("year", "2023")]))
        .unwrap();
    assert_eq!(y2023["results"][0]["ids"]["mal"], "600002");
    assert_eq!(f.requests(), 1, "filters reuse one cached search");
}

#[test]
fn neutral_details_and_relations() {
    let f = fixture();
    let d = f.client.answer(&req("details", &[("mal_id", "600001")])).unwrap();
    assert_eq!(d["id"], 700001);
    assert_eq!(d["title"]["english"], "Fox Relay");
    assert_eq!(d["startDate"]["year"], 2021);

    let r = f.client.answer(&req("relations", &[("id", "700001")])).unwrap();
    assert_eq!(
        r["relations"],
        json!([
            {"relation": "SEQUEL", "kind": "show", "title": "Kitsune Relay 2nd Season",
             "year": 2023, "format": "TV", "status": "RELEASING",
             "ids": {"anilist": "700002", "mal": "600002"}},
            {"relation": "SIDE_STORY", "kind": "movie", "title": "Kitsune Relay: Night Run",
             "year": 2022, "format": "MOVIE", "status": "FINISHED",
             "ids": {"anilist": "700003", "mal": "600003"}},
        ]),
        "anime only: no manga, no 18+"
    );
    assert_eq!(f.requests(), 2, "relations reuse the cached Media(id)");
}

#[test]
fn neutral_errors() {
    let f = fixture();
    let bad = |r: ProviderRequest| matches!(f.client.answer(&r), Err(ProviderError::BadRequest(_)));
    assert!(bad(ProviderRequest::new("tmdb", "search").param("query", "x")));
    assert!(bad(req("episodes", &[("id", "1")])));
    assert!(bad(req("details", &[])));
    assert!(bad(req("details", &[("id", "abc")])));
    assert!(bad(req("search", &[])));
    assert!(bad(req("search", &[("query", "x"), ("kind", "music")])));
    assert_eq!(
        f.client.answer(&req("details", &[("id", "799999")])),
        Err(ProviderError::NotFound("Not Found.".into()))
    );
}

#[test]
fn cache_keys_carry_the_provider_and_no_query_text() {
    let f = fixture();
    f.client.search("Kitsune Relay").unwrap();
    let key = r#"anilist:/ graphql:Search{"isAdult":false,"perPage":10,"search":"Kitsune Relay"}"#;
    assert!(f.cache.get(key).unwrap().is_some());
}
