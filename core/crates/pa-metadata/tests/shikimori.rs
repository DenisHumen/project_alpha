// SPDX-License-Identifier: MPL-2.0
//! Shikimori client against the synthetic cassette
//! `fixtures/shikimori/cassette.jsonl`, plus a load test of its rate limit.
//! No network: responses come from the cassette or a generated transport,
//! time from a manual clock.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{ClockedTransport, max_in_window, ok};
use pa_metadata::cache::MemoryCache;
use pa_metadata::clock::ManualClock;
use pa_metadata::fetch::FetchError;
use pa_metadata::replay::ReplayTransport;
use pa_metadata::shikimori::{RussianInfo, ShikimoriClient, ShikimoriConfig};
use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::json;

const CASSETTE: &str = include_str!("fixtures/shikimori/cassette.jsonl");
const T0: u64 = 1_800_000_000;
const DAY: u64 = 24 * 3600;

struct Fixture {
    client: ShikimoriClient,
    transport: Arc<ReplayTransport>,
    cache: Arc<MemoryCache>,
    clock: Arc<ManualClock>,
}

fn fixture() -> Fixture {
    let transport = Arc::new(ReplayTransport::from_jsonl(CASSETTE).unwrap());
    let cache = Arc::new(MemoryCache::default());
    let clock = Arc::new(ManualClock::at_unix(T0));
    let client = ShikimoriClient::new(
        ShikimoriConfig::default(),
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
        .fold(ProviderRequest::new("shikimori", endpoint), |r, (k, v)| {
            r.param(k, *v)
        })
}

#[test]
fn search_is_censored_and_identifies_the_app() {
    let f = fixture();
    let found = f.client.search("Kitsune Relay ").unwrap();
    let titles: Vec<_> = found.iter().map(|a| (a.id, a.russian_title())).collect();
    assert_eq!(
        titles,
        vec![
            (600001, Some("Эстафета кицунэ")),
            (600003, Some("Эстафета кицунэ: Ночной забег")),
            (600002, None),
        ]
    );
    assert_eq!(found[0].year(), Some(2021));
    assert!(found[1].is_movie());

    let sent = &f.transport.requests()[0];
    assert_eq!(
        sent.url(),
        "https://shikimori.io/api/animes?search=Kitsune%20Relay&limit=10&censored=true"
    );
    let ua = sent
        .headers
        .iter()
        .find(|(k, _)| k == "user-agent")
        .map(|(_, v)| v.as_str())
        .unwrap();
    assert!(ua.starts_with("Kinolune/"), "{ua}");
    assert!(!ua.contains("Mozilla"), "never a browser UA");

    f.client.search("Kitsune Relay").unwrap();
    assert_eq!(f.requests(), 1, "cached");
}

#[test]
fn russian_title_and_plain_description_by_mal_id() {
    let f = fixture();
    assert_eq!(
        f.client.russian(600001).unwrap(),
        RussianInfo {
            id: 600001,
            title: Some("Эстафета кицунэ".into()),
            license_name: Some("Лисья эстафета".into()),
            description: Some(
                "Мика бежит последний этап эстафеты, а лиса из старого храма знает финал.\n\
                 Синтетическое описание для тестов."
                    .into()
            ),
            original_title: "Kitsune Relay".into(),
            year: Some(2021),
        }
    );
    let sequel = f.client.russian(600002).unwrap();
    assert_eq!(sequel.title, None, "empty Russian title");
    assert_eq!(sequel.description.as_deref(), Some("Продолжение. Сезон 2"));
}

#[test]
fn hidden_missing_and_foreign_entries() {
    let f = fixture();
    assert!(matches!(
        f.client.anime(600009),
        Err(FetchError::NotFound { .. })
    ));
    assert_eq!(
        f.client.anime(600404).unwrap_err(),
        FetchError::NotFound {
            provider: "shikimori",
            what: "/animes/600404".into()
        }
    );
    assert_eq!(f.client.anime(600007).unwrap().myanimelist_id, Some(123));
    assert!(
        matches!(f.client.russian(600007), Err(FetchError::NotFound { .. })),
        "Shikimori id that is not this MAL id"
    );

    let uncensored = ShikimoriClient::new(
        ShikimoriConfig {
            include_adult: true,
            ..ShikimoriConfig::default()
        },
        Arc::new(ReplayTransport::from_jsonl(CASSETTE).unwrap()),
        Arc::new(MemoryCache::default()),
        Arc::new(ManualClock::at_unix(T0)),
    );
    assert!(uncensored.anime(600009).unwrap().adult());
}

#[test]
fn related_keeps_anime_only() {
    let f = fixture();
    let related = f.client.related(600001).unwrap();
    let r: Vec<_> = related
        .iter()
        .map(|r| (r.relation.as_deref().unwrap(), r.anime.as_ref().unwrap().id))
        .collect();
    assert_eq!(r, vec![("Sequel", 600002), ("Side story", 600003)]);
}

#[test]
fn ttl_follows_status() {
    let f = fixture();
    f.client.anime(600001).unwrap();
    f.client.anime(600002).unwrap();
    f.clock.advance(Duration::from_secs(2 * DAY));
    f.client.anime(600001).unwrap();
    assert_eq!(f.requests(), 2, "released: fresh for 14 days");
    f.client.anime(600002).unwrap();
    assert_eq!(f.requests(), 3, "ongoing: fresh for a day");
}

#[test]
fn retries_and_stale_fallback() {
    let f = fixture();
    assert_eq!(f.client.anime(600005).unwrap().base.name, "Slow Fox");
    assert_eq!(f.clock.sleeps()[0], Duration::from_secs(1), "exponential");

    let err = f.client.anime(600006).unwrap_err();
    assert!(
        matches!(err, FetchError::Http { status: 503, .. }),
        "{err:?}"
    );
    assert_eq!(f.requests(), 2 + 5, "first try plus four retries");

    f.client.anime(600001).unwrap();
    f.clock.advance(Duration::from_secs(30 * DAY));
    let offline = ShikimoriClient::new(
        ShikimoriConfig::default(),
        Arc::new(ReplayTransport::default()),
        f.cache.clone(),
        f.clock.clone(),
    );
    assert_eq!(offline.russian(600001).unwrap().year, Some(2021));
    assert_eq!(offline.purge_all(), 2);
}

#[test]
fn load_stays_within_5_per_second_and_90_per_minute() {
    let clock = Arc::new(ManualClock::at_unix(T0));
    let transport = Arc::new(ClockedTransport::new(clock.clone(), |r, _| {
        let id = r.path.trim_start_matches("/animes/");
        ok(
            &format!(r#"{{"id":{id},"name":"Load {id}","status":"released"}}"#),
            &[],
        )
    }));
    let client = ShikimoriClient::new(
        ShikimoriConfig::default(),
        transport.clone(),
        Arc::new(MemoryCache::default()),
        clock,
    );
    for id in 1..=400 {
        assert_eq!(client.anime(id).unwrap().base.id, id);
    }
    let times = transport.times();
    assert_eq!(times.len(), 400);
    let per_second = max_in_window(&times, Duration::from_secs(1));
    let per_minute = max_in_window(&times, Duration::from_secs(60));
    assert!(per_second <= 5, "{per_second} requests in one second");
    assert!(per_minute <= 90, "{per_minute} requests in one minute");
    assert!(
        per_minute >= 85,
        "limiter is not needlessly slow: {per_minute}"
    );
}

#[test]
fn neutral_requests() {
    let mut f = fixture();
    let found = f
        .client
        .call(&req(
            "search",
            &[("query", "Kitsune Relay"), ("kind", "movie")],
        ))
        .unwrap();
    assert_eq!(
        found["results"],
        json!([{
            "kind": "movie",
            "title": "Эстафета кицунэ: Ночной забег",
            "original_title": "Kitsune Relay: Night Run",
            "year": 2022,
            "format": "movie",
            "episodes": 1,
            "ids": {"shikimori": "600003", "mal": "600003"},
        }])
    );
    let y = f
        .client
        .call(&req(
            "search",
            &[("query", "Kitsune Relay"), ("year", "2023")],
        ))
        .unwrap();
    assert_eq!(
        y["results"][0]["title"], "Kitsune Relay 2nd Season",
        "falls back to the romanised title"
    );

    let ru = f
        .client
        .call(&req("russian", &[("mal_id", "600001")]))
        .unwrap();
    assert_eq!(ru["title"], "Эстафета кицунэ");
    assert_eq!(ru["license_name"], "Лисья эстафета");
    assert_eq!(ru["ids"]["mal"], "600001");
    assert!(
        ru["description"]
            .as_str()
            .unwrap()
            .starts_with("Мика бежит")
    );

    let d = f
        .client
        .call(&req("details", &[("mal_id", "600001")]))
        .unwrap();
    assert_eq!(d["russian"], "Эстафета кицунэ");
    assert_eq!(d["myanimelist_id"], 600001);
    let rel = f.client.call(&req("related", &[("id", "600001")])).unwrap();
    assert_eq!(rel.as_array().unwrap().len(), 2);
    assert_eq!(f.requests(), 3, "search once, anime once, related once");

    assert_eq!(
        f.client.call(&req("russian", &[("mal_id", "600404")])),
        Err(ProviderError::NotFound("/animes/600404".into()))
    );
    for bad in [
        req("russian", &[]),
        req("details", &[]),
        req("search", &[("query", "x"), ("kind", "manga")]),
        req("characters", &[("id", "1")]),
        ProviderRequest::new("anilist", "search").param("query", "x"),
    ] {
        assert!(
            matches!(f.client.call(&bad), Err(ProviderError::BadRequest(_))),
            "{bad:?}"
        );
    }
}
