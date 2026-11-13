// SPDX-License-Identifier: MPL-2.0
//! Offline mapping tests on hand-written fixtures (no network, no upstream data).

use pa_anime_mappings::anime_lists::{self, Target};
use pa_anime_mappings::{EpisodeRef, IdRef, MappingIndex, fribb, offline_db};

const OFFLINE: &str = include_str!("fixtures/anime-offline-database.jsonl");
const XML: &str = include_str!("fixtures/anime-list-master.xml");
const FRIBB: &str = include_str!("fixtures/anime-list-full.json");

fn s(season: u32, episode: u32) -> Target {
    Target::Season { season, episode }
}

fn ep(anidb: u32, season: u32, episode: u32) -> EpisodeRef {
    EpisodeRef {
        anidb,
        season,
        episode,
    }
}

fn index() -> MappingIndex {
    let (_, offline) = offline_db::parse_upstream_jsonl(OFFLINE.as_bytes()).unwrap();
    let mut idx = MappingIndex::new();
    idx.add_offline(offline);
    idx.add_anime_lists(anime_lists::parse(XML).unwrap());
    idx.add_fribb(fribb::parse(FRIBB.as_bytes()).unwrap());
    idx
}

#[test]
fn parses_fixtures() {
    let (meta, offline) = offline_db::parse_upstream_jsonl(OFFLINE.as_bytes()).unwrap();
    assert_eq!(meta.last_update, "2026-11-08");
    assert_eq!(
        offline.len(),
        5,
        "entry with unknown sources only is skipped"
    );
    assert_eq!(offline[4].episodes, None, "0 episodes means unknown");

    let al = anime_lists::parse(XML).unwrap();
    assert_eq!(al.len(), 6);
    assert_eq!(al[4].name, "Absolute & Order");
    assert_eq!(al[3].tvdb_kind.as_deref(), Some("movie"));
    assert_eq!(al[3].tmdb_movie, vec![300004]);
    assert_eq!(al[5].anidb, 9007);
    assert_eq!(al[2].mappings.len(), 4);
    assert_eq!(
        al[2].mappings[0].explicit,
        vec![(1, vec![3]), (2, vec![4, 5]), (3, vec![])]
    );

    assert_eq!(fribb::parse(FRIBB.as_bytes()).unwrap().len(), 3);
}

#[test]
fn cross_ids() {
    let idx = index();
    let r = idx.find(IdRef::Mal(5001)).unwrap();
    assert_eq!(r.ids.anidb, Some(9001));
    assert_eq!(r.ids.tvdb, Some(100001));
    assert_eq!(r.ids.tmdb_tv, Some(200001));
    assert_eq!(r.ids.imdb, vec!["tt0000001".to_string()]);
    assert_eq!(r.title.as_deref(), Some("Example Saga"));

    // One TVDB series, two AniDB entries (split cour).
    assert_eq!(idx.get(IdRef::Tvdb(100001)).count(), 2);

    let movie = idx.find(IdRef::AniDb(9004)).unwrap();
    assert_eq!(movie.ids.tmdb_movie, Some(300004));
    assert_eq!(movie.ids.imdb, vec!["tt0000004".to_string()]);

    // Entry without AniDB ID merged by MAL ID across layers.
    let web = idx.find(IdRef::Kitsu(7005)).unwrap();
    assert_eq!(web.ids.anilist, Some(6005));
    assert_eq!(web.title.as_deref(), Some("Web Only"));
    assert_eq!(idx.len(), 8);
}

#[test]
fn anidb_tvdb_tmdb_split_cour() {
    let idx = index();
    assert_eq!(idx.anidb_to_tvdb(ep(9001, 1, 5)), vec![s(1, 5)]);
    assert_eq!(idx.anidb_to_tvdb(ep(9002, 1, 3)), vec![s(1, 15)]);
    assert_eq!(idx.anidb_to_tmdb(9002, 3), vec![s(2, 3)]);

    assert_eq!(idx.tvdb_to_anidb(100001, s(1, 15)), Some(ep(9002, 1, 3)));
    assert_eq!(idx.tvdb_to_anidb(100001, s(1, 5)), Some(ep(9001, 1, 5)));
    assert_eq!(
        idx.tvdb_to_anidb(100001, s(1, 30)),
        None,
        "beyond both cours"
    );
    assert_eq!(idx.tmdb_to_anidb(200001, s(2, 3)), Some(ep(9002, 1, 3)));
    assert_eq!(idx.tmdb_to_anidb(200001, s(1, 12)), Some(ep(9001, 1, 12)));
}

#[test]
fn mapping_list_rules() {
    let idx = index();
    // Explicit pairs win over ranges and default.
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 1, 9)), vec![s(0, 1)]);
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 1, 18)), vec![s(0, 2)]);
    // Ranges with offsets.
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 1, 12)), vec![s(1, 11)]);
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 1, 20)), vec![s(1, 18)]);
    // Default season.
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 1, 5)), vec![s(1, 5)]);
    // Specials: one AniDB special → two TVDB specials; `-0` = no counterpart;
    // unmapped special → nothing.
    assert_eq!(idx.anidb_to_tvdb(ep(9003, 0, 2)), vec![s(0, 4), s(0, 5)]);
    assert!(idx.anidb_to_tvdb(ep(9003, 0, 3)).is_empty());
    assert!(idx.anidb_to_tvdb(ep(9003, 0, 7)).is_empty());

    // Reverse.
    assert_eq!(idx.tvdb_to_anidb(100003, s(1, 11)), Some(ep(9003, 1, 12)));
    assert_eq!(idx.tvdb_to_anidb(100003, s(0, 1)), Some(ep(9003, 1, 9)));
    assert_eq!(idx.tvdb_to_anidb(100003, s(0, 5)), Some(ep(9003, 0, 2)));
    assert_eq!(idx.tvdb_to_anidb(100003, s(1, 5)), Some(ep(9003, 1, 5)));
    // TVDB S1E9 would be AniDB 9 by default, but AniDB 9 maps to S0E1; S1E9 is AniDB 10.
    assert_eq!(idx.tvdb_to_anidb(100003, s(1, 9)), Some(ep(9003, 1, 10)));

    // Absolute order on TMDB.
    assert_eq!(
        idx.anidb_to_tmdb(9003, 14),
        vec![Target::Absolute { episode: 14 }]
    );
}

#[test]
fn absolute_and_fallbacks() {
    let idx = index();
    let abs = Target::Absolute { episode: 7 };
    assert_eq!(idx.anidb_to_tvdb(ep(9006, 1, 7)), vec![abs]);
    assert_eq!(idx.tvdb_to_anidb(100006, abs), Some(ep(9006, 1, 7)));

    // Not in Anime-Lists: Fribb seasons are the fallback.
    assert_eq!(idx.anidb_to_tvdb(ep(9008, 1, 4)), vec![s(2, 4)]);
    assert_eq!(idx.anidb_to_tmdb(9008, 4), vec![s(2, 4)]);

    // Movie and unknown IDs map to no episodes.
    assert!(idx.anidb_to_tvdb(ep(9004, 1, 1)).is_empty());
    assert!(idx.anidb_to_tvdb(ep(1, 1, 1)).is_empty());
}

#[test]
fn offline_layer_alone_has_no_tvdb() {
    // Bundled ODbL data only: cross-ID between anime sites works, TVDB does not.
    let (_, offline) = offline_db::parse_upstream_jsonl(OFFLINE.as_bytes()).unwrap();
    let mut idx = MappingIndex::new();
    idx.add_offline(offline);
    assert_eq!(
        idx.find(IdRef::AniList(6002)).unwrap().ids.anidb,
        Some(9002)
    );
    assert!(idx.anidb_to_tvdb(ep(9002, 1, 1)).is_empty());
}

#[test]
fn bundle_roundtrip_is_deterministic() {
    let (meta, entries) = offline_db::parse_upstream_jsonl(OFFLINE.as_bytes()).unwrap();
    let header = offline_db::bundle_header(&meta, "2026-45", "abc");
    let mut a = Vec::new();
    offline_db::write_bundle(&mut a, &header, &entries).unwrap();
    let mut rev = entries.clone();
    rev.reverse();
    let mut b = Vec::new();
    offline_db::write_bundle(&mut b, &header, &rev).unwrap();
    assert_eq!(a, b);

    let (h, back) = offline_db::read_bundle(a.as_slice()).unwrap();
    assert_eq!(h, header);
    assert!(h.license.starts_with("ODbL-1.0"));
    assert!(h.attribution.contains("anime-offline-database"));
    assert!(h.attribution.contains("manami-project"));
    assert_eq!(back.len(), entries.len());
    for e in &entries {
        assert!(back.contains(e));
    }
}

#[test]
fn rejects_non_odbl_upstream() {
    let bad = OFFLINE.replacen("Open Data Commons Open Database License (ODbL)", "Other", 1);
    assert!(offline_db::parse_upstream_jsonl(bad.as_bytes()).is_err());
}

/// Opt-in check against real upstream files the developer downloaded
/// locally (never committed): set `PA_ANIME_MAPPINGS_REAL_DIR` to a directory
/// with `anime-list-master.xml`, `anime-list-full.json` and the bundle
/// `anime-offline-ids.jsonl` from `data/anime-mappings/build.sh`, then run
/// `cargo test -p pa-anime-mappings -- --ignored`.
#[test]
#[ignore = "needs locally downloaded upstream files"]
fn real_upstream_files() {
    use pa_anime_mappings::update::Source;
    let dir = std::path::PathBuf::from(
        std::env::var("PA_ANIME_MAPPINGS_REAL_DIR").expect("PA_ANIME_MAPPINGS_REAL_DIR"),
    );
    let xml = std::fs::read(dir.join("anime-list-master.xml")).unwrap();
    let json = std::fs::read(dir.join("anime-list-full.json")).unwrap();
    let bundle = std::fs::read(dir.join("anime-offline-ids.jsonl")).unwrap();
    let al = Source::AnimeLists.spec();
    let fr = Source::Fribb.spec();
    assert!((al.validate)(&xml).unwrap() >= al.min_entries);
    assert!((fr.validate)(&json).unwrap() >= fr.min_entries);

    let (_, offline) = offline_db::read_bundle(bundle.as_slice()).unwrap();
    let mut idx = MappingIndex::new();
    idx.add_offline(offline);
    idx.add_anime_lists(anime_lists::parse(std::str::from_utf8(&xml).unwrap()).unwrap());
    idx.add_fribb(fribb::parse(&json).unwrap());
    // AniDB 1 is TVDB 72025 season 1 in Anime-Lists.
    assert_eq!(idx.anidb_to_tvdb(ep(1, 1, 3)), vec![s(1, 3)]);
    assert_eq!(idx.tvdb_to_anidb(72025, s(1, 3)), Some(ep(1, 1, 3)));
    assert_eq!(idx.find(IdRef::Mal(290)).unwrap().ids.anidb, Some(1));
}
