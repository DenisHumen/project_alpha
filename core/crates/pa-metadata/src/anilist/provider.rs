// SPDX-License-Identifier: MPL-2.0
//! [`AniListClient`] as the core's [`Provider`].
//!
//! Neutral requests (`provider = "anilist"`):
//!
//! | `endpoint` | params | response |
//! |---|---|---|
//! | `search` | `query`, opt. `year`, opt. `kind` (`movie`/`show`) | `{"results": [{"kind", "title", "original_title", "titles", "year", "format", "episodes", "popularity", "ids": {"anilist": "21", "mal": "21"}}]}` |
//! | `details` | `id` or `mal_id` | the `Media` object (camelCase AniList fields, with `relations`) |
//! | `relations` | `id` or `mal_id` | `{"relations": [{"relation", "kind", "title", "year", "format", "status", "ids"}]}`, anime only |
//!
//! In `search`, `title` is the romaji title (what anime releases are named
//! after), `original_title` the native one, and `titles` every title and
//! synonym for the title index. `kind` and `year` filter the results of one
//! cached search, so each query costs a single request of the scarce
//! AniList quota.

use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::{Value, json};

use super::{AniListClient, Media, PROVIDER};
use crate::fetch::Params;

impl Provider for AniListClient {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        self.answer(request)
    }
}

impl AniListClient {
    /// Answers a neutral request (the [`Provider`] impl, usable via `&self`).
    pub fn answer(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        if request.provider != PROVIDER {
            return Err(ProviderError::BadRequest(format!(
                "provider {:?} is not anilist",
                request.provider
            )));
        }
        let p = Params(request);
        match request.endpoint.as_str() {
            "search" => self.neutral_search(&p),
            "details" => serde_json::to_value(self.lookup(&p)?)
                .map_err(|e| ProviderError::Failed(e.to_string())),
            "relations" => {
                let media = self.lookup(&p)?;
                let edges = media.relations.map(|r| r.edges).unwrap_or_default();
                let relations: Vec<Value> = edges
                    .iter()
                    .filter(|e| e.node.is_anime())
                    .map(|e| {
                        let n = &e.node;
                        json!({
                            "relation": e.relation_type,
                            "kind": kind_of(n.format.as_deref()),
                            "title": n.title.romaji.as_ref().or(n.title.english.as_ref()),
                            "year": n.year(),
                            "format": n.format,
                            "status": n.status,
                            "ids": ids(n.id, n.id_mal),
                        })
                    })
                    .collect();
                Ok(json!({ "relations": relations }))
            }
            other => Err(ProviderError::BadRequest(format!(
                "unknown anilist endpoint {other:?}"
            ))),
        }
    }

    fn lookup(&self, p: &Params<'_>) -> Result<Media, ProviderError> {
        if let Some(id) = p.opt_number::<u64>("id")? {
            return Ok(self.media(id)?);
        }
        if let Some(mal) = p.opt_number::<u64>("mal_id")? {
            return Ok(self.media_by_mal(mal)?);
        }
        Err(ProviderError::BadRequest(
            "missing param \"id\" or \"mal_id\"".into(),
        ))
    }

    fn neutral_search(&self, p: &Params<'_>) -> Result<Value, ProviderError> {
        let query = p.get("query")?;
        let year: Option<u32> = p.opt_number("year")?;
        let kind = match p.opt("kind") {
            None => None,
            Some(k @ ("movie" | "show")) => Some(k),
            Some(k) => return Err(ProviderError::BadRequest(format!("unknown kind {k:?}"))),
        };
        let results: Vec<Value> = self
            .search(query)?
            .iter()
            .filter(|m| year.is_none() || m.year() == year)
            .filter(|m| kind.is_none_or(|k| kind_of(m.format.as_deref()) == k))
            .map(normalize)
            .collect();
        Ok(json!({ "results": results }))
    }
}

fn kind_of(format: Option<&str>) -> &'static str {
    if format == Some("MOVIE") {
        "movie"
    } else {
        "show"
    }
}

fn ids(id: u64, mal: Option<u64>) -> Value {
    let mut ids = json!({ "anilist": id.to_string() });
    if let Some(mal) = mal {
        ids["mal"] = json!(mal.to_string());
    }
    ids
}

fn normalize(m: &Media) -> Value {
    json!({
        "kind": kind_of(m.format.as_deref()),
        "title": m.title.romaji.as_ref().or(m.title.english.as_ref()),
        "original_title": m.title.native,
        "titles": m.all_titles(),
        "year": m.year(),
        "format": m.format,
        "episodes": m.episodes,
        "popularity": m.popularity,
        "ids": ids(m.id, m.id_mal),
    })
}
