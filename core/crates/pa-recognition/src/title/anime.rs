// SPDX-License-Identifier: MPL-2.0
//! Title records from the bundled anime-offline-database (`pa-anime-mappings`,
//! ODbL): main title plus synonyms (romaji, English, Japanese, Russian…),
//! anime-site ids, type, year and episode count.

use pa_anime_mappings::AnimeIds;
use pa_anime_mappings::offline_db::OfflineEntry;

use super::index::{TitleRecord, WorkKind};

/// Ids of an anime entry in the corpus/record notation.
pub fn ids(ids: &AnimeIds) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut put = |ns: &str, v: Option<String>| {
        if let Some(v) = v {
            out.insert(ns.to_owned(), v);
        }
    };
    put("anidb", ids.anidb.map(|v| v.to_string()));
    put("anilist", ids.anilist.map(|v| v.to_string()));
    put("mal", ids.mal.map(|v| v.to_string()));
    put("kitsu", ids.kitsu.map(|v| v.to_string()));
    put("tvdb", ids.tvdb.map(|v| format!("series:{v}")));
    put(
        "tmdb",
        ids.tmdb_tv
            .map(|v| format!("tv:{v}"))
            .or(ids.tmdb_movie.map(|v| format!("movie:{v}"))),
    );
    put("imdb", ids.imdb.first().cloned());
    out
}

/// anime-offline-database `type` → [`WorkKind`].
pub fn kind(upstream_type: &str) -> WorkKind {
    match upstream_type {
        "TV" | "ONA" => WorkKind::Series,
        "MOVIE" => WorkKind::Movie,
        "OVA" | "SPECIAL" => WorkKind::Special,
        _ => WorkKind::Unknown,
    }
}

/// One anime-offline-database entry as a title record.
pub fn record(entry: &OfflineEntry) -> TitleRecord {
    let mut titles = Vec::with_capacity(1 + entry.synonyms.len());
    titles.push(entry.title.clone());
    titles.extend(entry.synonyms.iter().cloned());
    TitleRecord {
        ids: ids(&entry.ids),
        kind: kind(&entry.kind),
        anime: true,
        year: entry.year,
        episodes: entry.episodes,
        titles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_entry() {
        let e = OfflineEntry {
            ids: AnimeIds {
                anidb: Some(9980),
                mal: Some(22043),
                ..Default::default()
            },
            title: "Fairy Tail (2014)".into(),
            kind: "TV".into(),
            episodes: Some(102),
            year: Some(2014),
            synonyms: vec!["Fairy Tail Season 2".into(), "Фейри Тейл".into()],
        };
        let r = record(&e);
        assert_eq!(r.ids["anidb"], "9980");
        assert_eq!(r.ids["mal"], "22043");
        assert_eq!(r.kind, WorkKind::Series);
        assert!(r.anime);
        assert_eq!(r.titles.len(), 3);
        assert_eq!(kind("MOVIE"), WorkKind::Movie);
        assert_eq!(kind("OVA"), WorkKind::Special);
    }
}
