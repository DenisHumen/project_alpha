// SPDX-License-Identifier: MPL-2.0
//! [`TmdbClient`] as the core's [`Provider`].
//!
//! Neutral requests (`provider = "tmdb"`):
//!
//! | `endpoint` | params | response |
//! |---|---|---|
//! | `search` | `query`, opt. `year`, opt. `kind` (`movie`/`tv`) | normalized: `{"results": [{"kind", "title", "original_title", "year", "popularity", "ids": {"tmdb": "movie:603"}}]}` |
//! | `details` | `kind`, `id` | TMDB JSON of `/movie/{id}` or `/tv/{id}` |
//! | `translations`, `alternative_titles`, `external_ids`, `images` | `kind`, `id` | TMDB JSON |
//! | `collection` | `id` | TMDB JSON |
//! | `season` | `id`, `season` | TMDB JSON |
//! | `episode_groups` | `id` | TMDB JSON |
//! | `episode_group` | `group` | TMDB JSON |
//! | `find` | `external_id`, `source` (`imdb`/`tvdb`/`wikidata`) | TMDB JSON |
//!
//! `search` matches the shape the eval baselines already use
//! (`pa_eval::recognizer::Naive`), so recording this provider with
//! `pa_eval::RecordingProvider` replaces the synthetic baseline tape
//! (META-025). Without `kind` it is a multi-search; `year` then keeps only
//! results of that year.

use pa_recognition::provider::{Provider, ProviderError, ProviderRequest};
use serde_json::{Value, json};

use super::{ExternalSource, MediaKind, PROVIDER, SearchResult, TmdbClient, TmdbError, TtlClass};

impl From<TmdbError> for ProviderError {
    fn from(e: TmdbError) -> Self {
        match e {
            TmdbError::NoKey => Self::Unavailable("no TMDB API key".into()),
            TmdbError::Unauthorized => Self::Unavailable("TMDB rejected the API key".into()),
            TmdbError::Transport(m) => Self::Unavailable(m),
            TmdbError::NotFound(what) => Self::NotFound(what),
            TmdbError::RateLimited { retry_after, .. } => Self::RateLimited {
                retry_after_secs: retry_after.map(|d| d.as_secs()),
            },
            e @ (TmdbError::Http { .. } | TmdbError::Decode(_)) => Self::Failed(e.to_string()),
        }
    }
}

impl Provider for TmdbClient {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        self.answer(request)
    }
}

impl TmdbClient {
    /// Answers a neutral request (the [`Provider`] impl, usable via `&self`).
    pub fn answer(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        if request.provider != PROVIDER {
            return Err(ProviderError::BadRequest(format!(
                "provider {:?} is not tmdb",
                request.provider
            )));
        }
        let p = Params(request);
        match request.endpoint.as_str() {
            "search" => self.neutral_search(&p),
            "details" => {
                let class = match p.kind()? {
                    MediaKind::Movie => TtlClass::Stable,
                    MediaKind::Tv => TtlClass::TvDetails,
                };
                self.raw(
                    &format!("/{}/{}", p.kind()?.as_str(), p.id()?),
                    self.lang(),
                    class,
                )
            }
            sub @ ("translations" | "alternative_titles" | "external_ids") => self.raw(
                &format!("/{}/{}/{sub}", p.kind()?.as_str(), p.id()?),
                Vec::new(),
                TtlClass::Stable,
            ),
            "images" => Ok(serde_json::to_value(self.images(p.kind()?, p.id()?)?)
                .map_err(|e| ProviderError::Failed(e.to_string()))?),
            "collection" => self.raw(
                &format!("/collection/{}", p.id()?),
                self.lang(),
                TtlClass::Stable,
            ),
            "season" => {
                let season: u32 = p.number("season")?;
                self.raw(
                    &format!("/tv/{}/season/{season}", p.id()?),
                    self.lang(),
                    TtlClass::TvParts,
                )
            }
            "episode_groups" => self.raw(
                &format!("/tv/{}/episode_groups", p.id()?),
                Vec::new(),
                TtlClass::TvParts,
            ),
            "episode_group" => Ok(serde_json::to_value(self.episode_group(p.get("group")?)?)
                .map_err(|e| ProviderError::Failed(e.to_string()))?),
            "find" => {
                let source = ExternalSource::parse(p.get("source")?)
                    .ok_or_else(|| ProviderError::BadRequest("unknown find source".into()))?;
                Ok(
                    serde_json::to_value(self.find(p.get("external_id")?, source)?)
                        .map_err(|e| ProviderError::Failed(e.to_string()))?,
                )
            }
            other => Err(ProviderError::BadRequest(format!(
                "unknown tmdb endpoint {other:?}"
            ))),
        }
    }

    fn raw(
        &self,
        path: &str,
        query: Vec<(String, String)>,
        class: TtlClass,
    ) -> Result<Value, ProviderError> {
        let body = self.get_body(path, query, class)?;
        serde_json::from_str(&body)
            .map_err(|e| ProviderError::Failed(format!("unexpected TMDB response: {e}")))
    }

    fn neutral_search(&self, p: &Params<'_>) -> Result<Value, ProviderError> {
        let query = p.get("query")?;
        let year: Option<u32> = p.opt_number("year")?;
        let results = match p.opt("kind") {
            Some(k) => {
                let kind = MediaKind::parse(k)
                    .ok_or_else(|| ProviderError::BadRequest(format!("unknown kind {k:?}")))?;
                match kind {
                    MediaKind::Movie => self.search_movie(query, year)?.results,
                    MediaKind::Tv => self.search_tv(query, year)?.results,
                }
            }
            None => {
                let mut results = self.search_multi(query)?.results;
                if let Some(y) = year {
                    results.retain(|r| r.year() == Some(y));
                }
                results
            }
        };
        Ok(json!({ "results": results.iter().filter_map(normalize).collect::<Vec<_>>() }))
    }
}

fn normalize(r: &SearchResult) -> Option<Value> {
    let kind = r.kind()?;
    Some(json!({
        "kind": match kind { MediaKind::Movie => "movie", MediaKind::Tv => "show" },
        "title": r.title,
        "original_title": r.original_title,
        "original_language": r.original_language,
        "year": r.year(),
        "popularity": r.popularity,
        "ids": { "tmdb": format!("{}:{}", kind.as_str(), r.id) },
    }))
}

struct Params<'a>(&'a ProviderRequest);

impl Params<'_> {
    fn opt(&self, key: &str) -> Option<&str> {
        self.0.params.get(key).map(String::as_str)
    }

    fn get(&self, key: &str) -> Result<&str, ProviderError> {
        self.opt(key)
            .ok_or_else(|| ProviderError::BadRequest(format!("missing param {key:?}")))
    }

    fn opt_number<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, ProviderError> {
        self.opt(key)
            .map(|v| {
                v.parse().map_err(|_| {
                    ProviderError::BadRequest(format!("param {key:?} is not a number"))
                })
            })
            .transpose()
    }

    fn number<T: std::str::FromStr>(&self, key: &str) -> Result<T, ProviderError> {
        self.opt_number(key)?
            .ok_or_else(|| ProviderError::BadRequest(format!("missing param {key:?}")))
    }

    fn id(&self) -> Result<u64, ProviderError> {
        self.number("id")
    }

    fn kind(&self) -> Result<MediaKind, ProviderError> {
        let k = self.get("kind")?;
        MediaKind::parse(k).ok_or_else(|| ProviderError::BadRequest(format!("unknown kind {k:?}")))
    }
}
