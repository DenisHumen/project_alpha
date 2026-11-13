// SPDX-License-Identifier: MPL-2.0
//! [`ShikimoriClient`] as the core's [`Provider`].
//!
//! Neutral requests (`provider = "shikimori"`):
//!
//! | `endpoint` | params | response |
//! |---|---|---|
//! | `search` | `query`, opt. `year`, opt. `kind` (`movie`/`show`) | `{"results": [{"kind", "title", "original_title", "year", "ids": {"shikimori": "1", "mal": "1"}}]}` |
//! | `russian` | `mal_id` | `{"title", "license_name", "description", "original_title", "year", "ids"}` — Russian title and plain-text synopsis |
//! | `details` | `id` or `mal_id` | Shikimori JSON of `/animes/{id}` |
//! | `related` | `id` or `mal_id` | Shikimori JSON of `/animes/{id}/related`, anime entries only |
//!
//! In `search`, `title` is the Russian title (falls back to the romanised
//! one) and `original_title` the romanised one. `kind` and `year` filter the
//! results of one cached search.

use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::{Value, json};

use super::{AnimeShort, PROVIDER, ShikimoriClient};
use crate::fetch::Params;

impl Provider for ShikimoriClient {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        self.answer(request)
    }
}

impl ShikimoriClient {
    /// Answers a neutral request (the [`Provider`] impl, usable via `&self`).
    pub fn answer(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        if request.provider != PROVIDER {
            return Err(ProviderError::BadRequest(format!(
                "provider {:?} is not shikimori",
                request.provider
            )));
        }
        let p = Params(request);
        match request.endpoint.as_str() {
            "search" => self.neutral_search(&p),
            "russian" => {
                let info = self.russian(p.number("mal_id")?)?;
                Ok(json!({
                    "title": info.title,
                    "license_name": info.license_name,
                    "description": info.description,
                    "original_title": info.original_title,
                    "year": info.year,
                    "ids": ids(info.id),
                }))
            }
            "details" => to_value(&self.anime(id(&p)?)?),
            "related" => to_value(&self.related(id(&p)?)?),
            other => Err(ProviderError::BadRequest(format!(
                "unknown shikimori endpoint {other:?}"
            ))),
        }
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
            .filter(|a| year.is_none() || a.year() == year)
            .filter(|a| kind.is_none_or(|k| kind_of(a) == k))
            .map(normalize)
            .collect();
        Ok(json!({ "results": results }))
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<Value, ProviderError> {
    serde_json::to_value(v).map_err(|e| ProviderError::Failed(e.to_string()))
}

/// `id` or `mal_id` — the same number for Shikimori anime.
fn id(p: &Params<'_>) -> Result<u64, ProviderError> {
    match p.opt_number::<u64>("id")? {
        Some(id) => Ok(id),
        None => p.opt_number::<u64>("mal_id")?.ok_or_else(|| {
            ProviderError::BadRequest("missing param \"id\" or \"mal_id\"".into())
        }),
    }
}

fn kind_of(a: &AnimeShort) -> &'static str {
    if a.is_movie() { "movie" } else { "show" }
}

fn ids(id: u64) -> Value {
    json!({ "shikimori": id.to_string(), "mal": id.to_string() })
}

fn normalize(a: &AnimeShort) -> Value {
    json!({
        "kind": kind_of(a),
        "title": a.russian_title().unwrap_or(&a.name),
        "original_title": a.name,
        "year": a.year(),
        "format": a.kind,
        "episodes": a.episodes,
        "ids": ids(a.id),
    })
}
