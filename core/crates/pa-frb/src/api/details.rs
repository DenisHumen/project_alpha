// SPDX-License-Identifier: MPL-2.0
//! What the Detail screen shows besides the item row (UI-009) for Dart. The
//! call is [`CoreLibrary::details`](crate::api::library::CoreLibrary::details).

use pa_catalog as c;

/// Details of a movie or show; episodes get `tech` only.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreItemDetails {
    pub genres: Vec<String>,
    pub runtime_ms: Option<i64>,
    /// Provider logo (https), if one was fetched.
    pub logo_url: Option<String>,
    /// Local clearlogo next to the files.
    pub local_logo: Option<CoreFileRef>,
    pub ratings: Vec<CoreRating>,
    pub external_ids: Vec<CoreExternalId>,
    /// Cast first (billing order), then crew.
    pub credits: Vec<CoreCredit>,
    /// Trailers first, then other extras by name.
    pub extras: Vec<CoreExtra>,
    /// Probe data of the version the item plays.
    pub tech: Option<CoreTechInfo>,
}

/// A file of a source; network files play through the stream proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFileRef {
    pub source_id: i64,
    /// Path relative to the source root.
    pub rel_path: String,
    /// Absolute path for local sources.
    pub local_path: Option<String>,
}

/// An average user rating, 0–10.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreRating {
    /// `tmdb`, …
    pub provider: String,
    pub value: f64,
    pub votes: Option<i64>,
}

/// An id of the work at a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreExternalId {
    pub provider: String,
    /// Entity type inside the provider (`movie`, `tv`); empty if none.
    pub kind: String,
    pub id: String,
}

/// A person of the cast (`character`) or crew (`job`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCredit {
    pub crew: bool,
    pub name: String,
    pub character: Option<String>,
    pub job: Option<String>,
    /// `tmdb:<person id>`.
    pub person_ref: Option<String>,
    pub profile_url: Option<String>,
}

/// A trailer, featurette… next to the main file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreExtra {
    pub file: CoreFileRef,
    /// `trailer`, `teaser`, `featurette`, `behind_the_scenes`, `deleted_scene`,
    /// `interview`, `short`, `scene`, … `other`.
    pub extra_type: String,
    /// File name without the extension.
    pub title: String,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
}

/// What the probe found in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreTechInfo {
    pub file: Option<CoreFileRef>,
    pub container: Option<String>,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
    pub video_codec: Option<String>,
    pub video_profile: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub bit_depth: Option<i64>,
    pub frame_rate: Option<f64>,
    /// `dolby_vision`, `hdr10_plus`, `hdr10`, `hlg`, strongest first.
    pub hdr: Vec<String>,
    /// Audio format slugs (`eac3`, `atmos`, `dts_hd_ma`…).
    pub audio_formats: Vec<String>,
    pub audio_langs: Vec<String>,
    pub sub_langs: Vec<String>,
}

impl From<c::FileRef> for CoreFileRef {
    fn from(f: c::FileRef) -> Self {
        Self {
            source_id: f.source_id,
            rel_path: f.rel_path,
            local_path: f.local_path,
        }
    }
}

impl From<c::ItemDetails> for CoreItemDetails {
    fn from(d: c::ItemDetails) -> Self {
        Self {
            genres: d.genres,
            runtime_ms: d.runtime_ms,
            logo_url: d.logo_url,
            local_logo: d.local_logo.map(Into::into),
            ratings: d
                .ratings
                .into_iter()
                .map(|r| CoreRating {
                    provider: r.provider,
                    value: r.value,
                    votes: r.votes,
                })
                .collect(),
            external_ids: d
                .external_ids
                .into_iter()
                .map(|x| CoreExternalId {
                    provider: x.provider,
                    kind: x.kind,
                    id: x.id,
                })
                .collect(),
            credits: d
                .credits
                .into_iter()
                .map(|p| CoreCredit {
                    crew: p.crew,
                    name: p.name,
                    character: p.character,
                    job: p.job,
                    person_ref: p.person_ref,
                    profile_url: p.profile_url,
                })
                .collect(),
            extras: d
                .extras
                .into_iter()
                .map(|e| CoreExtra {
                    file: e.file.into(),
                    extra_type: e.extra_type,
                    title: e.title,
                    size_bytes: e.size_bytes,
                    duration_ms: e.duration_ms,
                })
                .collect(),
            tech: d.tech.map(|t| CoreTechInfo {
                file: t.file.map(Into::into),
                container: t.container,
                size_bytes: t.size_bytes,
                duration_ms: t.duration_ms,
                video_codec: t.video_codec,
                video_profile: t.video_profile,
                width: t.width,
                height: t.height,
                bit_depth: t.bit_depth,
                frame_rate: t.frame_rate,
                hdr: t.hdr,
                audio_formats: t.audio_formats,
                audio_langs: t.audio_langs,
                sub_langs: t.sub_langs,
            }),
        }
    }
}
