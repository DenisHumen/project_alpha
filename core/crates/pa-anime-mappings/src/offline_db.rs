// SPDX-License-Identifier: MPL-2.0
//! anime-offline-database (ODbL-1.0 + DbCL-1.0): upstream JSONL parser and the
//! derived database we bundle with the app.
//!
//! The derived file is itself a database under **ODbL-1.0** (share-alike):
//! it is plain JSONL (no encryption, ODbL §4.7), its header carries the
//! attribution, and `data/anime-mappings/build.sh` reproduces it from the
//! pinned upstream release (ODbL §4.6). It contains **only** ODbL data —
//! nothing from Anime-Lists or Fribb (`docs/legal/anime-mappings.md` §2 п.4).

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AnimeIds, Error, Result};

/// Format tag of the derived bundle header.
pub const BUNDLE_FORMAT: &str = "pa-anime-offline-ids/1";
/// Licence of the derived bundle (share-alike from upstream).
pub const BUNDLE_LICENSE: &str = "ODbL-1.0 (database) + DbCL-1.0 (contents)";

/// One anime from anime-offline-database, reduced to what mapping needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineEntry {
    #[serde(flatten)]
    pub ids: AnimeIds,
    pub title: String,
    /// Upstream `type`: TV, MOVIE, OVA, ONA, SPECIAL, UNKNOWN.
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episodes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Alternative titles in scripts that appear in file names (Latin,
    /// Cyrillic, Japanese and Chinese): romaji, English, Russian, native.
    /// Used by the title index (META-007); see [`keep_synonym`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
}

/// Whether a synonym is worth bundling: every letter is Latin, Cyrillic,
/// kana or CJK. Titles in other scripts (Arabic, Thai, Hangul…) do not occur
/// in release names we parse and would only grow the bundle.
pub fn keep_synonym(s: &str) -> bool {
    let mut letters = false;
    for c in s.chars().filter(|c| c.is_alphabetic()) {
        letters = true;
        let ok = matches!(c as u32,
            0x0041..=0x024F        // Latin, Latin-1, Latin Extended-A/B
            | 0x1E00..=0x1EFF      // Latin Extended Additional
            | 0x0400..=0x04FF      // Cyrillic
            | 0x3005..=0x3007      // 々 〆 〇
            | 0x3040..=0x30FF      // Hiragana, Katakana
            | 0x31F0..=0x31FF      // Katakana extensions
            | 0x3400..=0x4DBF      // CJK Extension A
            | 0x4E00..=0x9FFF      // CJK Unified Ideographs
            | 0xFF00..=0xFFEF      // Half-width and full-width forms
            | 0x2150..=0x218F      // Number forms (Ⅲ)
        );
        if !ok {
            return false;
        }
    }
    letters
}

/// First line of the derived bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleHeader {
    pub format: String,
    pub license: String,
    pub attribution: String,
    pub upstream_repository: String,
    /// Upstream `lastUpdate` (YYYY-MM-DD).
    #[serde(default)]
    pub upstream_last_update: String,
    /// Release tag the bundle was built from, e.g. `2026-45`.
    #[serde(default)]
    pub upstream_tag: String,
    /// SHA-256 of the upstream JSONL the bundle was built from.
    #[serde(default)]
    pub upstream_sha256: String,
}

/// Metadata line of the upstream JSONL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpstreamMeta {
    pub repository: String,
    pub license_name: String,
    pub last_update: String,
}

/// Parse the upstream `anime-offline-database.jsonl`. Entries without any
/// recognised provider ID are skipped.
pub fn parse_upstream_jsonl(input: impl BufRead) -> Result<(UpstreamMeta, Vec<OfflineEntry>)> {
    let mut meta = None;
    let mut entries = Vec::new();
    for (n, line) in input.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line)
            .map_err(|e| Error::Parse(format!("line {}: {e}", n + 1)))?;
        if meta.is_none() {
            if v.get("sources").is_some() {
                return Err(Error::Parse("missing metadata line".into()));
            }
            meta = Some(UpstreamMeta {
                repository: str_of(&v, "repository"),
                license_name: v
                    .get("license")
                    .map(|l| str_of(l, "name"))
                    .unwrap_or_default(),
                last_update: str_of(&v, "lastUpdate"),
            });
            continue;
        }
        let mut e = OfflineEntry {
            title: str_of(&v, "title"),
            kind: str_of(&v, "type"),
            episodes: v
                .get("episodes")
                .and_then(Value::as_u64)
                .and_then(|x| u32::try_from(x).ok())
                .filter(|&x| x > 0),
            year: v
                .pointer("/animeSeason/year")
                .and_then(Value::as_u64)
                .and_then(|x| u16::try_from(x).ok()),
            ..Default::default()
        };
        for syn in v
            .get("synonyms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let syn = syn.trim();
            if syn != e.title && keep_synonym(syn) && !e.synonyms.iter().any(|x| x == syn) {
                e.synonyms.push(syn.to_owned());
            }
        }
        for s in v
            .get("sources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(s) = s.as_str() {
                e.ids.set_from_source_url(s);
            }
        }
        if e.ids != AnimeIds::default() {
            entries.push(e);
        }
    }
    let meta = meta.ok_or_else(|| Error::Parse("empty input".into()))?;
    if !meta.license_name.contains("ODbL") {
        return Err(Error::Invalid(format!(
            "unexpected upstream licence {:?}; re-check docs/legal/anime-mappings.md",
            meta.license_name
        )));
    }
    Ok((meta, entries))
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Header for a bundle derived from `meta`.
pub fn bundle_header(meta: &UpstreamMeta, tag: &str, sha256: &str) -> BundleHeader {
    BundleHeader {
        format: BUNDLE_FORMAT.into(),
        license: BUNDLE_LICENSE.into(),
        attribution: crate::ATTRIBUTION[0].0.into(),
        upstream_repository: meta.repository.clone(),
        upstream_last_update: meta.last_update.clone(),
        upstream_tag: tag.into(),
        upstream_sha256: sha256.into(),
    }
}

/// Write the derived bundle: header line, then one entry per line, sorted
/// for reproducible output.
pub fn write_bundle(
    out: &mut impl Write,
    header: &BundleHeader,
    entries: &[OfflineEntry],
) -> Result<()> {
    let mut sorted: Vec<&OfflineEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        (
            a.ids.anidb.is_none(),
            a.ids.anidb,
            a.ids.mal,
            a.ids.anilist,
            &a.title,
        )
            .cmp(&(
                b.ids.anidb.is_none(),
                b.ids.anidb,
                b.ids.mal,
                b.ids.anilist,
                &b.title,
            ))
    });
    serde_json::to_writer(&mut *out, header)?;
    out.write_all(b"\n")?;
    for e in sorted {
        serde_json::to_writer(&mut *out, e)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Read a derived bundle written by [`write_bundle`].
pub fn read_bundle(input: impl BufRead) -> Result<(BundleHeader, Vec<OfflineEntry>)> {
    let mut lines = input.lines();
    let first = lines
        .next()
        .ok_or_else(|| Error::Parse("empty bundle".into()))??;
    let header: BundleHeader = serde_json::from_str(&first)?;
    if header.format != BUNDLE_FORMAT {
        return Err(Error::Invalid(format!(
            "unsupported bundle format {:?}",
            header.format
        )));
    }
    let mut entries = Vec::new();
    for line in lines {
        let line = line?;
        if !line.trim().is_empty() {
            entries.push(serde_json::from_str(&line)?);
        }
    }
    Ok((header, entries))
}

/// Count entries of an upstream JSONL (validation for runtime updates).
pub fn validate_upstream(bytes: &[u8]) -> Result<usize> {
    parse_upstream_jsonl(bytes).map(|(_, e)| e.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synonym_scripts() {
        for keep in ["Ex Saga", "Атака титанов", "進撃の巨人", "モブサイコ100 Ⅲ", "Pokémon", "時々"] {
            assert!(keep_synonym(keep), "{keep}");
        }
        for drop in ["진격의 거인", "ผ่าพิภพไททัน", "الهجوم على العمالقة", "123", ""] {
            assert!(!keep_synonym(drop), "{drop}");
        }
    }

    #[test]
    fn synonyms_survive_the_bundle() {
        let upstream = concat!(
            r#"{"license":{"name":"ODbL v1.0"},"repository":"r","lastUpdate":"2026-11-08"}"#,
            "\n",
            r#"{"sources":["https://anidb.net/anime/1"],"title":"A","synonyms":["A","B","B","진격","Б"]}"#,
            "\n"
        );
        let (meta, entries) = parse_upstream_jsonl(upstream.as_bytes()).unwrap();
        assert_eq!(entries[0].synonyms, ["B", "Б"]);
        let mut out = Vec::new();
        write_bundle(&mut out, &bundle_header(&meta, "t", "x"), &entries).unwrap();
        let (_, back) = read_bundle(out.as_slice()).unwrap();
        assert_eq!(back, entries);
    }
}
