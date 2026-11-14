// SPDX-License-Identifier: MPL-2.0
//! Metadata run (META-013) end to end: a scanned folder, synthetic provider
//! responses (no network, no real TMDB data), the library afterwards.

use std::path::Path;
use std::sync::{Arc, Mutex};

use pa_catalog::pa_metadata::anilist::{AniListClient, AniListConfig};
use pa_catalog::pa_metadata::cache::MemoryCache;
use pa_catalog::pa_metadata::clock::ManualClock;
use pa_catalog::pa_metadata::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use pa_catalog::pa_metadata::key::Keys;
use pa_catalog::pa_metadata::shikimori::{ShikimoriClient, ShikimoriConfig};
use pa_catalog::pa_metadata::tmdb::{TmdbClient, TmdbConfig};
use pa_catalog::{Catalog, MetadataOptions, MetadataProviders};
use pa_library::rusqlite::Connection;
use pa_scanner::CancelToken;

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

/// Answers by path (and the `query` parameter); records every request.
#[derive(Default)]
struct FakeProviders {
    calls: Mutex<Vec<String>>,
    offline: Mutex<bool>,
    unauthorized: Mutex<bool>,
}

fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: body.to_owned(),
    }
}

fn status(code: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status: code,
        headers: vec![],
        body: body.to_owned(),
    }
}

fn param<'a>(r: &'a HttpRequest, key: &str) -> Option<&'a str> {
    r.query
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

impl FakeProviders {
    fn respond(&self, r: &HttpRequest, body: Option<&str>) -> Result<HttpResponse, TransportError> {
        let query = param(r, "query").unwrap_or("");
        self.calls
            .lock()
            .unwrap()
            .push(format!("{} {}", r.path, query));
        if *self.offline.lock().unwrap() {
            return Err(TransportError("offline".into()));
        }
        if *self.unauthorized.lock().unwrap() && r.path.starts_with("/search") {
            return Ok(status(
                401,
                r#"{"status_code":7,"status_message":"Invalid API key"}"#,
            ));
        }
        let empty = r#"{"page":1,"results":[],"total_pages":0,"total_results":0}"#;
        Ok(match r.path.as_str() {
            "/search/movie" if query == "Dune" => ok(r#"{"page":1,"results":[
                {"id":841,"title":"Дюна","original_title":"Dune","release_date":"1984-12-14","popularity":20},
                {"id":438631,"title":"Дюна","original_title":"Dune","release_date":"2021-09-15","popularity":90}
            ]}"#),
            "/search/movie" => ok(empty),
            "/find/tt0113277" => ok(r#"{"movie_results":[
                {"id":949,"title":"Схватка","original_title":"Heat","release_date":"1995-12-15"}
            ],"tv_results":[]}"#),
            "/movie/949" => ok(r#"{"id":949,"title":"Схватка","original_title":"Heat",
                "original_language":"en","overview":"Ограбление.","release_date":"1995-12-15",
                "status":"Released","runtime":170,"imdb_id":"tt0113277"}"#),
            "/movie/438631" => ok(r#"{"id":438631,"title":"Дюна","original_title":"Dune",
                "original_language":"en","overview":"Пустынная планета.","release_date":"2021-09-15",
                "status":"Released","runtime":155,"poster_path":"/dune-ru.jpg",
                "backdrop_path":"/dune-bg.jpg","imdb_id":"tt0000001","vote_average":7.8,
                "vote_count":12000,"genres":[{"id":878,"name":"фантастика"}],
                "belongs_to_collection":{"id":726871,"name":"Дюна: Коллекция"}}"#),
            "/movie/438631/credits" => ok(r#"{"id":438631,"cast":[
                {"id":2,"name":"Ребекка Фергюсон","character":"Джессика","order":1},
                {"id":1,"name":"Тимоти Шаламе","character":"Пол Атрейдес","order":0,
                 "profile_path":"/tc.jpg"},
                {"id":3,"name":" ","character":"","order":2}
            ],"crew":[
                {"id":10,"name":"Ханс Циммер","job":"Original Music Composer","department":"Sound"},
                {"id":11,"name":"Дени Вильнёв","job":"Screenplay","department":"Writing"},
                {"id":11,"name":"Дени Вильнёв","job":"Director","department":"Directing"},
                {"id":12,"name":"Someone","job":"Gaffer","department":"Lighting"}
            ]}"#),
            "/movie/438631/translations" => ok(r#"{"id":438631,"translations":[
                {"iso_3166_1":"US","iso_639_1":"en","data":{"title":"Dune","overview":"Desert planet."}},
                {"iso_3166_1":"RU","iso_639_1":"ru","data":{"title":"Дюна","overview":"Пустынная планета."}}
            ]}"#),
            "/search/tv" if query == "Dark" || query == "Тьма" => ok(r#"{"page":1,"results":[
                {"id":70523,"name":"Тьма","original_name":"Dark","first_air_date":"2017-12-01"}
            ]}"#),
            "/search/tv" => ok(empty),
            "/tv/70523" => ok(r#"{"id":70523,"name":"Тьма","original_name":"Dark",
                "original_language":"de","overview":"Пропажа детей.","first_air_date":"2017-12-01",
                "status":"Ended","episode_run_time":[60],"poster_path":"/dark.jpg",
                "seasons":[{"season_number":1,"episode_count":10}]}"#),
            "/tv/70523/translations" => ok(r#"{"id":70523,"translations":[
                {"iso_3166_1":"US","iso_639_1":"en","data":{"name":"Dark"}}
            ]}"#),
            "/tv/70523/season/1" => ok(r#"{"id":1,"season_number":1,"episodes":[
                {"id":1001,"season_number":1,"episode_number":1,"name":"Секреты","overview":"Первая серия.",
                 "air_date":"2017-12-01","runtime":51,"still_path":"/e1.jpg"},
                {"id":1002,"season_number":1,"episode_number":2,"name":"Ложь","air_date":"2017-12-01"},
                {"id":1003,"season_number":1,"episode_number":3,"name":"Прошлое и настоящее"}
            ]}"#),
            "/animes/52991" => ok(r#"{"id":52991,"name":"Sousou no Frieren",
                "russian":"Провожающая в последний путь Фрирен","kind":"tv","status":"released",
                "aired_on":"2023-09-29","description":"Эльфийка-[b]маг[/b].","myanimelist_id":52991,
                "rating":"pg_13","english":["Frieren: Beyond Journey's End"],"japanese":[],"synonyms":[]}"#),
            "" | "/" if body.is_some_and(|b| b.contains("Frieren")) => {
                ok(r#"{"data":{"Page":{"media":[
                {"id":154587,"idMal":52991,"format":"TV","status":"FINISHED","episodes":28,"duration":24,
                 "title":{"romaji":"Sousou no Frieren","english":"Frieren: Beyond Journey's End",
                          "native":"葬送のフリーレン"},
                 "synonyms":[],"startDate":{"year":2023,"month":9,"day":29},
                 "description":"An elf mage.<br><br>After the journey.",
                 "coverImage":{"large":"https://img.example/frieren.jpg"}}
            ]}}}"#)
            }
            "" | "/" => ok(r#"{"data":{"Page":{"media":[]}}}"#),
            other => status(
                404,
                &format!(r#"{{"status_message":"no fixture for {other}"}}"#),
            ),
        })
    }
}

struct Transport(Arc<FakeProviders>);

impl HttpTransport for Transport {
    fn get(&self, r: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.0.respond(r, None)
    }

    fn post_json(&self, r: &HttpRequest, body: &str) -> Result<HttpResponse, TransportError> {
        self.0.respond(r, Some(body))
    }
}

fn providers(fake: &Arc<FakeProviders>, tmdb_key: bool) -> MetadataProviders {
    let transport: Arc<dyn HttpTransport> = Arc::new(Transport(fake.clone()));
    let cache = Arc::new(MemoryCache::default());
    let clock = Arc::new(ManualClock::at_unix(1_800_000_000));
    let keys = if tmdb_key {
        Keys::none().with_user_key(Some("test-key"))
    } else {
        Keys::none()
    };
    let mut tmdb_config = TmdbConfig::default();
    tmdb_config.retry.max_retries = 0;
    let mut anilist_config = AniListConfig::default();
    anilist_config.retry.max_retries = 0;
    let mut shiki_config = ShikimoriConfig::default();
    shiki_config.retry.max_retries = 0;
    MetadataProviders {
        tmdb: Some(Arc::new(TmdbClient::new(
            tmdb_config,
            keys,
            transport.clone(),
            cache.clone(),
            clock.clone(),
        ))),
        anilist: Some(Arc::new(AniListClient::new(
            anilist_config,
            transport.clone(),
            cache.clone(),
            clock.clone(),
        ))),
        shikimori: Some(Arc::new(ShikimoriClient::new(
            shiki_config,
            transport,
            cache,
            clock,
        ))),
    }
}

fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv", 1);
    media(&root, "Movies/Zzyzx Qwerty 2003.mkv", 2);
    media(
        &root,
        "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv",
        3,
    );
    media(&root, "Shows/Dark/Season 1/Dark.S01E02.1080p.mkv", 4);
    media(&root, "Serialy/Тьма/Тьма.S01E03.1080p.mkv", 5);
    media(
        &root,
        "Anime/[SubsPlease] Sousou no Frieren - 01 (1080p) [ABCD1234].mkv",
        6,
    );
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    (dir, cat)
}

fn run(cat: &Catalog, p: &MetadataProviders) -> pa_catalog::MetadataReport {
    let mut progress = Vec::new();
    let report = cat
        .fetch_metadata(
            p,
            &MetadataOptions::default(),
            &CancelToken::new(),
            &mut |d, t| progress.push((d, t)),
        )
        .unwrap();
    if let (false, Some(&(d, t))) = (report.stopped, progress.last()) {
        assert_eq!(d, t);
    }
    report
}

fn db(dir: &tempfile::TempDir) -> Connection {
    Connection::open(dir.path().join("library.db")).unwrap()
}

#[test]
fn movies_shows_and_anime_get_provider_metadata() {
    let (dir, cat) = setup();
    assert_eq!(cat.shows().unwrap().len(), 3, "Dark, Тьма, Frieren before");
    let fake = Arc::new(FakeProviders::default());
    let report = run(&cat, &providers(&fake, true));
    assert_eq!(report.works, 5, "{report:?}");
    assert_eq!(report.matched, 4, "{report:?}");
    assert_eq!(report.not_found, 1, "{report:?}");
    assert_eq!(report.merged, 1, "{report:?}");
    assert!(report.error.is_none());

    // Movie: the 2021 film, not the 1984 one.
    let movies = cat.movies().unwrap();
    let dune = movies.iter().find(|m| m.tmdb_id.is_some()).unwrap();
    assert_eq!(dune.tmdb_id, Some(438_631));
    assert_eq!(dune.title, "Дюна");
    assert_eq!(dune.original_title.as_deref(), Some("Dune"));
    assert_eq!(dune.overview.as_deref(), Some("Пустынная планета."));
    assert_eq!(
        dune.poster_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w500/dune-ru.jpg")
    );
    assert_eq!(
        dune.backdrop_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w1280/dune-bg.jpg")
    );
    let unknown = movies.iter().find(|m| m.tmdb_id.is_none()).unwrap();
    assert!(unknown.poster_url.is_none());

    // UI-009: cast, crew, rating and genres for the Detail screen.
    let details = cat.details(dune.id).unwrap();
    assert_eq!(details.genres, ["фантастика"]);
    assert_eq!(details.runtime_ms, Some(155 * 60_000));
    assert_eq!(details.ratings.len(), 1);
    assert_eq!(details.ratings[0].provider, "tmdb");
    assert!((details.ratings[0].value - 7.8).abs() < 1e-9);
    assert_eq!(details.ratings[0].votes, Some(12_000));
    let people: Vec<_> = details
        .credits
        .iter()
        .map(|c| {
            (
                c.crew,
                c.name.as_str(),
                c.character.as_deref().or(c.job.as_deref()).unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        people,
        [
            (false, "Тимоти Шаламе", "Пол Атрейдес"),
            (false, "Ребекка Фергюсон", "Джессика"),
            (true, "Дени Вильнёв", "Director"),
            (true, "Дени Вильнёв", "Screenplay"),
            (true, "Ханс Циммер", "Original Music Composer"),
        ]
    );
    assert_eq!(details.credits[0].person_ref.as_deref(), Some("tmdb:1"));
    assert_eq!(
        details.credits[0].profile_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w185/tc.jpg")
    );
    assert!(details.external_ids.iter().any(|x| x.provider == "imdb" && x.id == "tt0000001"));
    // Heat has no credits fixture (404): metadata still applies, no people.
    let heat = movies.iter().find(|m| m.tmdb_id == Some(949));
    if let Some(heat) = heat {
        assert!(cat.details(heat.id).unwrap().credits.is_empty());
    }

    // LIB-010: the TMDB collection of the movie.
    let collections = cat.collections().unwrap();
    assert_eq!(collections.len(), 1, "{collections:?}");
    assert_eq!(collections[0].name, "Дюна: Коллекция");
    assert_eq!(
        collections[0].external_ref.as_deref(),
        Some("tmdb:collection:726871")
    );
    assert_eq!(collections[0].kind, pa_catalog::CollectionKind::External);
    let in_collection = cat.collection_items(collections[0].id).unwrap();
    assert_eq!(in_collection.len(), 1);
    assert_eq!(in_collection[0].id, dune.id);
    assert_eq!(cat.item_collections(dune.id).unwrap(), collections);

    // Show: two folders of one TMDB series became one show.
    let shows = cat.shows().unwrap();
    assert_eq!(shows.len(), 2, "{shows:?}");
    let dark = shows.iter().find(|s| s.tmdb_id == Some(70_523)).unwrap();
    assert_eq!(dark.title, "Тьма");
    assert_eq!(dark.overview.as_deref(), Some("Пропажа детей."));
    let eps = cat.episodes(dark.id).unwrap();
    let names: Vec<_> = eps.iter().map(|e| (e.episode, e.title.as_str())).collect();
    assert_eq!(
        names,
        [
            (Some(1), "Секреты"),
            (Some(2), "Ложь"),
            (Some(3), "Прошлое и настоящее")
        ]
    );
    assert_eq!(eps[0].overview.as_deref(), Some("Первая серия."));
    assert_eq!(
        eps[0].backdrop_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w300/e1.jpg")
    );
    assert_eq!(
        eps[1].poster_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w500/dark.jpg")
    );

    // Anime: TMDB did not know it; AniList and Shikimori did.
    let frieren = shows.iter().find(|s| s.tmdb_id.is_none()).unwrap();
    assert_eq!(frieren.title, "Провожающая в последний путь Фрирен");
    assert_eq!(frieren.original_title.as_deref(), Some("葬送のフリーレン"));
    assert_eq!(frieren.overview.as_deref(), Some("Эльфийка-маг."));
    assert_eq!(
        frieren.poster_url.as_deref(),
        Some("https://img.example/frieren.jpg")
    );

    let conn = db(&dir);
    let ids: Vec<(String, String)> = conn
        .prepare("SELECT provider, ext_id FROM external_ids ORDER BY provider, ext_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let ids: Vec<(&str, &str)> = ids.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(
        ids,
        [
            ("anilist", "154587"),
            ("imdb", "tt0000001"),
            ("mal", "52991"),
            ("shikimori", "52991"),
            ("tmdb", "438631"),
            ("tmdb", "70523")
        ]
    );
    // Titles in several languages; local spellings kept as synonyms.
    let titles: Vec<(String, String, String)> = conn
        .prepare("SELECT kind, lang, text FROM titles WHERE work_id = ?1 ORDER BY kind, lang, text")
        .unwrap()
        .query_map([dark.id.to_string()[1..].parse::<i64>().unwrap()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let has = |k: &str, l: &str, t: &str| titles.iter().any(|x| x.0 == k && x.1 == l && x.2 == t);
    assert!(has("main", "ru", "Тьма"), "{titles:?}");
    assert!(has("official", "de", "Dark"), "{titles:?}");
    assert!(has("official", "en", "Dark"), "{titles:?}");
    assert!(has("synonym", "", "Dark"), "{titles:?}");
    assert!(has("synonym", "", "Тьма"), "{titles:?}");
    // Parser decisions are now scorer decisions.
    let decided: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT DISTINCT decided_by, state, model_version FROM matches ORDER BY 1, 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        decided
            .iter()
            .any(|d| d.0 == "scorer" && d.1 == "auto" && d.2.as_deref() == Some("scorer_v0")),
        "{decided:?}"
    );
    assert!(
        decided.iter().any(|d| d.0 == "rule"),
        "unknown movie keeps the parser decision"
    );

    // Search finds the provider titles.
    let hits = cat.search("Dune", 10).unwrap();
    assert!(hits.iter().any(|h| h.tmdb_id == Some(438_631)), "{hits:?}");
    let hits = cat.search("Frieren Beyond", 10).unwrap();
    assert!(!hits.is_empty());

    // A second run has nothing to do and makes no request.
    let before = fake.calls.lock().unwrap().len();
    let again = run(&cat, &providers(&fake, true));
    assert_eq!(again.works, 0, "{again:?}");
    assert_eq!(fake.calls.lock().unwrap().len(), before);
}

#[test]
fn user_fix_by_tmdb_id_is_read_by_id_and_kept() {
    let (_dir, cat) = setup();
    let unknown = cat
        .movies()
        .unwrap()
        .into_iter()
        .find(|m| m.title.starts_with("Zzyzx"))
        .unwrap();
    cat.fix_match(unknown.id, None, None, Some(438_631))
        .unwrap();
    let fake = Arc::new(FakeProviders::default());
    let report = run(&cat, &providers(&fake, true));
    assert_eq!(report.not_found, 0, "{report:?}");
    // The fixed movie and the Dune folder are the same TMDB movie now.
    let movies = cat.movies().unwrap();
    assert_eq!(movies.len(), 1, "{movies:?}");
    assert_eq!(movies[0].title, "Дюна");
    let calls = fake.calls.lock().unwrap().clone();
    assert!(
        !calls.iter().any(|c| c.starts_with("/search/movie Zzyzx")),
        "a fixed id is not searched: {calls:?}"
    );
}

#[test]
fn rejected_key_stops_the_run_and_offline_marks_works_failed() {
    let (dir, cat) = setup();
    let fake = Arc::new(FakeProviders::default());
    *fake.unauthorized.lock().unwrap() = true;
    let report = run(&cat, &providers(&fake, true));
    assert!(report.stopped);
    assert_eq!(report.error.as_deref(), Some("TMDB rejected the API key"));
    assert_eq!(report.matched, 0);

    *fake.unauthorized.lock().unwrap() = false;
    *fake.offline.lock().unwrap() = true;
    let report = run(&cat, &providers(&fake, true));
    assert!(report.failed > 0, "{report:?}");
    assert_eq!(report.matched, 0);
    let failed: i64 = db(&dir)
        .query_row(
            "SELECT count(*) FROM work_metadata WHERE outcome = 'failed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(failed > 0);
    // Titles stay the parsed ones.
    assert!(cat.movies().unwrap().iter().all(|m| m.poster_url.is_none()));

    // Failed works are retried once the network is back (here: right
    // away, with a zero retry delay).
    *fake.offline.lock().unwrap() = false;
    let options = MetadataOptions {
        retry_failed_after_ms: -1,
        ..Default::default()
    };
    let report = cat
        .fetch_metadata(
            &providers(&fake, true),
            &options,
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(report.matched, 4, "{report:?}");
}

#[test]
fn without_a_tmdb_key_only_anime_is_asked() {
    let (_dir, cat) = setup();
    let fake = Arc::new(FakeProviders::default());
    let report = run(&cat, &providers(&fake, false));
    assert_eq!(report.works, 1, "{report:?}");
    assert_eq!(report.matched, 1);
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.iter().all(|c| !c.starts_with("/search")), "{calls:?}");
}

#[test]
fn an_nfo_imdb_id_is_looked_up_instead_of_searched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Movies/Heat (1995)/Heat (1995).mkv", 1);
    std::fs::write(
        root.join("Movies/Heat (1995)/Heat (1995).nfo"),
        "https://www.imdb.com/title/tt0113277/",
    )
    .unwrap();
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    let fake = Arc::new(FakeProviders::default());
    let report = run(&cat, &providers(&fake, true));
    assert_eq!((report.works, report.matched), (1, 1), "{report:?}");
    let calls = fake.calls.lock().unwrap().clone();
    assert!(
        calls.iter().all(|c| !c.starts_with("/search")),
        "no title search: {calls:?}"
    );
    let movies = cat.movies().unwrap();
    assert_eq!(movies[0].tmdb_id, Some(949));
    assert_eq!(movies[0].title, "Схватка");
    let decided: String = db(&dir)
        .query_row("SELECT decided_by FROM matches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(decided, "nfo", "a known id keeps the NFO decision");
}
