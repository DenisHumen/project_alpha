// SPDX-License-Identifier: MPL-2.0
//! TMDB client against the synthetic cassette `fixtures/tmdb/cassette.jsonl`.
//! No network: every response comes from the cassette, time from a manual
//! clock.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pa_metadata::cache::{CacheEntry, CacheError, MAX_CACHE_AGE, MemoryCache, ProviderCache};
use pa_metadata::clock::ManualClock;
use pa_metadata::http::HttpResponse;
use pa_metadata::key::{KeySource, Keys};
use pa_metadata::replay::ReplayTransport;
use pa_metadata::tmdb::{
    EpisodeGroupType, ExternalSource, MediaKind, TmdbClient, TmdbConfig, TmdbError,
};
use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::{Value, json};

const CASSETTE: &str = include_str!("fixtures/tmdb/cassette.jsonl");
const T0: u64 = 1_800_000_000;
const DAY: u64 = 24 * 3600;
const USER_KEY: &str = "user-key-1";
const PROJECT_KEY: &str = "project-key-1";

/// Cache that remembers the keys it was asked to store.
#[derive(Default)]
struct SpyCache {
    inner: MemoryCache,
    keys: Mutex<Vec<String>>,
}

impl ProviderCache for SpyCache {
    fn get(&self, key: &str) -> Result<Option<CacheEntry>, CacheError> {
        self.inner.get(key)
    }
    fn put(&self, provider: &str, key: &str, entry: &CacheEntry) -> Result<(), CacheError> {
        self.keys.lock().unwrap().push(key.to_owned());
        self.inner.put(provider, key, entry)
    }
    fn purge_older_than(&self, cutoff: u64) -> Result<usize, CacheError> {
        self.inner.purge_older_than(cutoff)
    }
    fn purge_provider(&self, provider: &str) -> Result<usize, CacheError> {
        self.inner.purge_provider(provider)
    }
}

struct Fixture {
    client: TmdbClient,
    transport: Arc<ReplayTransport>,
    cache: Arc<SpyCache>,
    clock: Arc<ManualClock>,
}

fn fixture_with(keys: Keys) -> Fixture {
    let transport = Arc::new(ReplayTransport::from_jsonl(CASSETTE).unwrap());
    let cache = Arc::new(SpyCache::default());
    let clock = Arc::new(ManualClock::at_unix(T0));
    let client = TmdbClient::new(
        TmdbConfig::default(),
        keys,
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

fn fixture() -> Fixture {
    fixture_with(Keys::none().with_user_key(Some(USER_KEY)))
}

impl Fixture {
    fn requests(&self) -> usize {
        self.transport.requests().len()
    }
}

// --- Endpoints ---------------------------------------------------------------

#[test]
fn search_movie_with_and_without_year() {
    let f = fixture();
    let page = f.client.search_movie("Lunar Harbor", Some(2019)).unwrap();
    assert_eq!(page.results.len(), 1);
    let top = &page.results[0];
    assert_eq!(top.id, 900001);
    assert_eq!(top.kind(), Some(MediaKind::Movie));
    assert_eq!(top.title.as_deref(), Some("Лунная гавань"));
    assert_eq!(top.original_title.as_deref(), Some("Lunar Harbor"));
    assert_eq!(top.year(), Some(2019));

    let page = f.client.search_movie("  Lunar Harbor ", None).unwrap();
    let years: Vec<_> = page.results.iter().map(|r| r.year()).collect();
    assert_eq!(years, vec![Some(2019), Some(1987)]);
}

#[test]
fn search_tv_and_multi() {
    let f = fixture();
    let tv = f.client.search_tv("Kitsune Relay", Some(2021)).unwrap();
    assert_eq!(tv.results[0].kind(), Some(MediaKind::Tv));
    assert_eq!(
        tv.results[0].original_title.as_deref(),
        Some("Kitsune Relay")
    );
    assert_eq!(tv.results[0].year(), Some(2021));

    let multi = f.client.search_multi("Lunar Harbor").unwrap();
    let kinds: Vec<_> = multi.results.iter().map(|r| (r.kind(), r.id)).collect();
    assert_eq!(
        kinds,
        vec![
            (Some(MediaKind::Movie), 900001),
            (Some(MediaKind::Tv), 800003),
            (Some(MediaKind::Movie), 900003),
        ],
        "people are dropped"
    );
}

#[test]
fn movie_details_translations_titles_ids_images_collection() {
    let f = fixture();
    let d = f.client.details(MediaKind::Movie, 900001).unwrap();
    assert_eq!(d.title.as_deref(), Some("Лунная гавань"));
    assert_eq!(d.year(), Some(2019));
    assert_eq!(d.runtime, Some(118));
    assert_eq!(d.imdb_id.as_deref(), Some("tt9000001"));
    assert_eq!(d.tagline, Some(String::new()));
    let collection_id = d.belongs_to_collection.unwrap().id;

    let tr = f.client.translations(MediaKind::Movie, 900001).unwrap();
    let ru = tr.find("ru-RU").unwrap();
    assert_eq!(ru.data.title.as_deref(), Some("Лунная гавань"));
    assert_eq!(
        tr.find("en").unwrap().data.title,
        None,
        "empty = untranslated"
    );
    assert!(tr.find("de-DE").is_none());

    let alt = f
        .client
        .alternative_titles(MediaKind::Movie, 900001)
        .unwrap();
    assert_eq!(alt.titles.len(), 2);
    assert_eq!(alt.titles[1].kind.as_deref(), Some("katakana"));
    assert_eq!(alt.titles[0].kind, None);

    let ids = f.client.external_ids(MediaKind::Movie, 900001).unwrap();
    assert_eq!(ids.imdb_id.as_deref(), Some("tt9000001"));
    assert_eq!(ids.tvdb_id, None);

    let img = f.client.images(MediaKind::Movie, 900001).unwrap();
    assert_eq!(img.posters[0].iso_639_1.as_deref(), Some("ru"));
    assert_eq!(img.backdrops[0].iso_639_1, None);
    assert_eq!(
        pa_metadata::tmdb::image_url("w342", &img.posters[0].file_path),
        "https://image.tmdb.org/t/p/w342/synthetic-poster-900001.jpg"
    );

    let c = f.client.collection(collection_id).unwrap();
    let parts: Vec<_> = c.parts.iter().map(|p| (p.id, p.year())).collect();
    assert_eq!(parts, vec![(900001, Some(2019)), (900002, Some(2022))]);
    assert!(c.parts.iter().all(|p| p.kind() == Some(MediaKind::Movie)));
}

#[test]
fn tv_details_seasons_groups_and_find() {
    let f = fixture();
    let d = f.client.details(MediaKind::Tv, 800002).unwrap();
    assert_eq!(d.title.as_deref(), Some("Эстафета кицунэ"));
    assert_eq!(d.original_title.as_deref(), Some("Kitsune Relay"));
    assert_eq!(d.year(), Some(2021));
    assert!(d.is_ongoing());
    assert_eq!(d.number_of_episodes, Some(25));
    assert_eq!(d.seasons[0].season_number, 0);

    let alt = f.client.alternative_titles(MediaKind::Tv, 800002).unwrap();
    assert_eq!(alt.titles[0].title, "Kitsune no Relay", "TV uses `results`");

    let ids = f.client.external_ids(MediaKind::Tv, 800002).unwrap();
    assert_eq!(ids.tvdb_id, Some(390001));
    assert_eq!(ids.imdb_id, None, "empty string = none");

    let groups = f.client.episode_groups(800002).unwrap();
    let absolute = groups
        .results
        .iter()
        .find(|g| g.group_type == EpisodeGroupType::Absolute)
        .unwrap();
    let group = f.client.episode_group(&absolute.id).unwrap();
    let eps = &group.groups[0].episodes;
    assert_eq!(eps.len(), 24);
    let e13 = &eps[12];
    assert_eq!(
        (e13.order, e13.season_number, e13.episode_number),
        (12, 2, 1)
    );
    assert_eq!(groups.results[1].group_type, EpisodeGroupType::StoryArc);

    let season = f.client.season(800002, 1).unwrap();
    assert_eq!(season.episodes.len(), 12);
    assert_eq!(season.episodes[0].runtime, Some(24));

    let found = f.client.find("tt9000001", ExternalSource::Imdb).unwrap();
    assert_eq!(found.movie_results[0].id, 900001);
    assert_eq!(found.movie_results[0].kind(), Some(MediaKind::Movie));
    assert!(found.tv_results.is_empty());

    assert!(matches!(
        f.client.episode_group("../x"),
        Err(TmdbError::NotFound(_))
    ));
}

// --- Keys --------------------------------------------------------------------

#[test]
fn v3_key_goes_in_query_and_never_into_cache_keys() {
    let f = fixture();
    f.client.details(MediaKind::Movie, 900001).unwrap();
    let sent = &f.transport.requests()[0];
    assert!(sent.query.contains(&("api_key".into(), USER_KEY.into())));
    assert!(sent.headers.iter().all(|(k, _)| k != "authorization"));
    let keys = f.cache.keys.lock().unwrap().clone();
    assert_eq!(keys, vec!["tmdb:/movie/900001?language=ru-RU".to_owned()]);
}

#[test]
fn v4_token_goes_in_bearer_header() {
    let token = "eyJx.eyJy.z";
    let f = fixture_with(Keys::none().with_user_key(Some(token)));
    f.client.details(MediaKind::Movie, 900001).unwrap();
    let sent = &f.transport.requests()[0];
    assert!(sent.query.iter().all(|(k, _)| k != "api_key"));
    assert!(
        sent.headers
            .contains(&("authorization".into(), format!("Bearer {token}")))
    );
}

#[test]
fn user_key_overrides_project_key_at_runtime() {
    let f = fixture_with(Keys::none().with_project_key(Some(PROJECT_KEY)));
    assert_eq!(f.client.key_source(), Some(KeySource::Project));
    f.client.search_movie("Lunar Harbor", None).unwrap();
    f.client.set_user_key(Some(USER_KEY));
    assert_eq!(f.client.key_source(), Some(KeySource::User));
    f.client.search_movie("Lunar Harbor", Some(2019)).unwrap();
    f.client.set_user_key(None);
    assert_eq!(f.client.key_source(), Some(KeySource::Project));

    let used: Vec<String> = f
        .transport
        .requests()
        .iter()
        .map(|r| {
            r.query
                .iter()
                .find(|(k, _)| k == "api_key")
                .unwrap()
                .1
                .clone()
        })
        .collect();
    assert_eq!(used, vec![PROJECT_KEY, USER_KEY]);
}

#[test]
fn works_without_key() {
    let f = fixture_with(Keys::none());
    assert_eq!(f.client.key_source(), None);
    assert_eq!(
        f.client.search_movie("Lunar Harbor", None),
        Err(TmdbError::NoKey)
    );
    assert_eq!(f.requests(), 0, "no request without a key");

    // Cached data stays available without a key.
    let cached = r#"{"page":1,"results":[{"id":1,"title":"Cached","media_type":"movie"}]}"#;
    f.cache
        .put(
            "tmdb",
            "tmdb:/search/movie?include_adult=false&language=ru-RU&query=Lunar%20Harbor",
            &CacheEntry {
                body: cached.into(),
                fetched_at: T0 - 30 * DAY,
                expires_at: T0 - 29 * DAY,
            },
        )
        .unwrap();
    let page = f.client.search_movie("Lunar Harbor", None).unwrap();
    assert_eq!(page.results[0].title.as_deref(), Some("Cached"));
}

#[test]
fn unauthorized_is_reported() {
    let f = fixture();
    assert_eq!(
        f.client.details(MediaKind::Movie, 900404),
        Err(TmdbError::Unauthorized)
    );
    assert_eq!(f.requests(), 1, "401 is not retried");
}

// --- Rate limit and backoff --------------------------------------------------

#[test]
fn http_429_is_retried_with_backoff() {
    let f = fixture();
    let page = f.client.search_movie("Busy Title", None).unwrap();
    assert_eq!(page.results[0].id, 900001);
    assert_eq!(f.requests(), 3);
    let sleeps = f.clock.sleeps();
    // 1st 429: Retry-After: 2. 2nd 429 without header: exponential, retry #1 = 2 s.
    // Each pause is followed by a short token wait (slow restart).
    let long: Vec<_> = sleeps
        .iter()
        .copied()
        .filter(|d| *d >= Duration::from_secs(1))
        .collect();
    assert_eq!(long, vec![Duration::from_secs(2), Duration::from_secs(2)]);
}

#[test]
fn persistent_429_gives_up_with_rate_limited() {
    let f = fixture();
    let err = f.client.search_movie("Always Busy", None).unwrap_err();
    assert_eq!(
        err,
        TmdbError::RateLimited {
            attempts: 5,
            retry_after: None
        }
    );
    assert_eq!(f.requests(), 5);
    let long: Vec<u64> = f
        .clock
        .sleeps()
        .iter()
        .filter(|d| **d >= Duration::from_secs(1))
        .map(Duration::as_secs)
        .collect();
    assert_eq!(long, vec![1, 2, 4, 8]);
    assert_eq!(
        ProviderError::from(err),
        ProviderError::RateLimited {
            retry_after_secs: None
        }
    );
}

#[test]
fn server_error_is_retried_once_then_succeeds() {
    let f = fixture();
    let d = f.client.details(MediaKind::Movie, 900503).unwrap();
    assert_eq!(d.original_title.as_deref(), Some("After the Outage"));
    assert_eq!(f.requests(), 2);
}

#[test]
fn not_found_is_not_retried() {
    let f = fixture();
    assert!(matches!(
        f.client.details(MediaKind::Movie, 999999),
        Err(TmdbError::NotFound(_))
    ));
    assert_eq!(f.requests(), 1);
}

#[test]
fn client_side_rate_limit_under_load() {
    // 100 requests at 20 req/s with a burst of 20: at least 80 / 20 = 4 s
    // of waiting, spread over the requests beyond the burst.
    let f = fixture();
    for i in 0..100u64 {
        f.transport.push(
            &format!("/movie/{}?language=ru-RU", 1_000 + i),
            HttpResponse {
                status: 200,
                headers: vec![],
                body: format!(r#"{{"id":{}}}"#, 1_000 + i),
            },
        );
    }
    for i in 0..100u64 {
        f.client.details(MediaKind::Movie, 1_000 + i).unwrap();
    }
    let waited: Duration = f.clock.sleeps().iter().sum();
    assert!(
        (waited.as_secs_f64() - 4.0).abs() < 1e-6,
        "waited {waited:?}"
    );
}

// --- Cache -------------------------------------------------------------------

#[test]
fn cache_hit_refresh_stale_fallback_and_hard_limit() {
    let f = fixture();
    f.client.details(MediaKind::Movie, 900001).unwrap();
    f.client.details(MediaKind::Movie, 900001).unwrap();
    assert_eq!(f.requests(), 1, "second call is a cache hit");

    // Movies stay fresh for 14 days.
    f.clock.advance(Duration::from_secs(13 * DAY));
    f.client.details(MediaKind::Movie, 900001).unwrap();
    assert_eq!(f.requests(), 1);
    f.clock.advance(Duration::from_secs(2 * DAY));
    f.client.details(MediaKind::Movie, 900001).unwrap();
    assert_eq!(f.requests(), 2, "expired entry is refreshed");

    // Offline: the transport has no answer, a stale entry is served.
    f.clock.advance(Duration::from_secs(20 * DAY));
    let client = TmdbClient::new(
        TmdbConfig::default(),
        Keys::none().with_user_key(Some(USER_KEY)),
        Arc::new(ReplayTransport::default()),
        f.cache.clone(),
        f.clock.clone(),
    );
    assert_eq!(client.details(MediaKind::Movie, 900001).unwrap().id, 900001);

    // Past six months nothing is served, and purge removes it.
    f.clock.advance(MAX_CACHE_AGE);
    assert!(matches!(
        client.details(MediaKind::Movie, 900001),
        Err(TmdbError::Transport(_))
    ));
    assert_eq!(client.purge_expired(), 1);
    assert!(f.cache.inner.is_empty());
}

#[test]
fn ongoing_series_refresh_daily_ended_ones_biweekly() {
    let f = fixture();
    f.client.details(MediaKind::Tv, 800002).unwrap(); // Returning Series
    f.client.details(MediaKind::Tv, 800001).unwrap(); // Ended
    f.clock.advance(Duration::from_secs(DAY + 1));
    f.client.details(MediaKind::Tv, 800002).unwrap();
    f.client.details(MediaKind::Tv, 800001).unwrap();
    let paths: Vec<String> = f
        .transport
        .requests()
        .iter()
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(paths, vec!["/tv/800002", "/tv/800001", "/tv/800002"]);
}

#[test]
fn purge_all_drops_every_tmdb_entry() {
    let f = fixture();
    f.client.details(MediaKind::Movie, 900001).unwrap();
    f.client.search_movie("Lunar Harbor", None).unwrap();
    assert_eq!(f.client.purge_all(), 2);
    assert!(f.cache.inner.is_empty());
}

// --- Provider (neutral requests) ---------------------------------------------

fn call(
    client: &mut TmdbClient,
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<Value, ProviderError> {
    let mut req = ProviderRequest::new("tmdb", endpoint);
    for (k, v) in params {
        req = req.param(k, *v);
    }
    client.call(&req)
}

#[test]
fn neutral_search_matches_eval_tape_shape() {
    let mut f = fixture();
    let v = call(
        &mut f.client,
        "search",
        &[("query", "Lunar Harbor"), ("year", "2019")],
    )
    .unwrap();
    // Multi-search filtered by year: only the 2019 movie.
    assert_eq!(
        v,
        json!({"results": [{
            "kind": "movie", "title": "Лунная гавань", "original_title": "Lunar Harbor",
            "original_language": "en", "year": 2019, "popularity": 30.5,
            "ids": {"tmdb": "movie:900001"}
        }]})
    );

    let v = call(
        &mut f.client,
        "search",
        &[("query", "Kitsune Relay"), ("kind", "show")],
    )
    .unwrap();
    assert_eq!(v["results"][0]["kind"], "show");
    assert_eq!(v["results"][0]["ids"]["tmdb"], "tv:800002");
}

#[test]
fn neutral_raw_endpoints() {
    let mut f = fixture();
    let d = call(
        &mut f.client,
        "details",
        &[("kind", "tv"), ("id", "800002")],
    )
    .unwrap();
    assert_eq!(d["status"], "Returning Series");
    let ids = call(
        &mut f.client,
        "external_ids",
        &[("kind", "tv"), ("id", "800002")],
    )
    .unwrap();
    assert_eq!(ids["tvdb_id"], 390001);
    let s = call(
        &mut f.client,
        "season",
        &[("id", "800002"), ("season", "1")],
    )
    .unwrap();
    assert_eq!(s["episodes"].as_array().unwrap().len(), 12);
    let g = call(
        &mut f.client,
        "episode_group",
        &[("group", "5f00000000000000000a0001")],
    )
    .unwrap();
    assert_eq!(g["type"], 2);
    let found = call(
        &mut f.client,
        "find",
        &[("external_id", "tt9000001"), ("source", "imdb")],
    )
    .unwrap();
    assert_eq!(found["movie_results"][0]["id"], 900001);
    let c = call(&mut f.client, "collection", &[("id", "910001")]).unwrap();
    assert_eq!(c["parts"].as_array().unwrap().len(), 2);
}

#[test]
fn neutral_errors() {
    let mut f = fixture();
    assert!(matches!(
        call(&mut f.client, "nope", &[]),
        Err(ProviderError::BadRequest(_))
    ));
    assert!(matches!(
        call(&mut f.client, "details", &[("kind", "movie")]),
        Err(ProviderError::BadRequest(_))
    ));
    assert!(matches!(
        call(
            &mut f.client,
            "details",
            &[("kind", "movie"), ("id", "999999")]
        ),
        Err(ProviderError::NotFound(_))
    ));
    let mut other = ProviderRequest::new("anilist", "search");
    other = other.param("query", "x");
    assert!(matches!(
        f.client.call(&other),
        Err(ProviderError::BadRequest(_))
    ));

    let mut keyless = fixture_with(Keys::none());
    assert!(matches!(
        call(&mut keyless.client, "search", &[("query", "Lunar Harbor")]),
        Err(ProviderError::Unavailable(_))
    ));
}

#[test]
fn attribution_is_exposed() {
    let a = pa_metadata::attribution::TMDB;
    assert_eq!(
        a.notice,
        "This product uses the TMDB API but is not endorsed or certified by TMDB."
    );
    assert!(pa_metadata::attribution::ALL.contains(&a));
}
