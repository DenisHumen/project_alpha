// SPDX-License-Identifier: MPL-2.0
//! Helix response types. Field names follow the Helix reference; fields
//! Twitch may omit or send as `null` are `Option` or defaulted, unknown
//! fields are ignored, so new API fields do not break decoding.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::pagination::HasId;
use crate::util::{fill_image_template, parse_video_duration};

/// `GET /users`: profile with nickname and avatar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// User id.
    pub id: String,
    /// Login (lower-case nickname).
    pub login: String,
    /// Display name (nickname as shown).
    pub display_name: String,
    /// `""`, `affiliate` or `partner`.
    #[serde(default)]
    pub broadcaster_type: String,
    /// Channel description.
    #[serde(default)]
    pub description: String,
    /// Avatar URL.
    #[serde(default)]
    pub profile_image_url: String,
    /// Offline banner URL.
    #[serde(default)]
    pub offline_image_url: String,
    /// RFC 3339 creation time.
    #[serde(default)]
    pub created_at: String,
}

/// `GET /channels/followed`: one followed channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowedChannel {
    /// Channel (broadcaster) id.
    pub broadcaster_id: String,
    /// Channel login.
    pub broadcaster_login: String,
    /// Channel display name.
    pub broadcaster_name: String,
    /// RFC 3339 time the user followed.
    pub followed_at: String,
}

/// `GET /streams`, `GET /streams/followed`: a live stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stream {
    /// Stream id (also the `stream_id` of the resulting VOD).
    pub id: String,
    /// Broadcaster id.
    pub user_id: String,
    /// Broadcaster login.
    pub user_login: String,
    /// Broadcaster display name.
    pub user_name: String,
    /// Category id.
    #[serde(default)]
    pub game_id: String,
    /// Category name.
    #[serde(default)]
    pub game_name: String,
    /// `live`, or `""` on errors.
    #[serde(default, rename = "type")]
    pub kind: String,
    /// Stream title.
    #[serde(default)]
    pub title: String,
    /// Viewers now.
    #[serde(default)]
    pub viewer_count: u64,
    /// RFC 3339 start time.
    #[serde(default)]
    pub started_at: String,
    /// ISO 639-1 language.
    #[serde(default)]
    pub language: String,
    /// Thumbnail template with `{width}`/`{height}`.
    #[serde(default)]
    pub thumbnail_url: String,
    /// Tags.
    #[serde(default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
    /// Mature content flag.
    #[serde(default)]
    pub is_mature: bool,
}

impl Stream {
    /// Thumbnail URL at the given size.
    pub fn thumbnail(&self, width: u32, height: u32) -> String {
        fill_image_template(&self.thumbnail_url, width, height)
    }
}

/// `GET /channels`: channel information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Broadcaster id.
    pub broadcaster_id: String,
    /// Broadcaster login.
    pub broadcaster_login: String,
    /// Broadcaster display name.
    pub broadcaster_name: String,
    /// Broadcast language.
    #[serde(default)]
    pub broadcaster_language: String,
    /// Current (or last) category id.
    #[serde(default)]
    pub game_id: String,
    /// Current (or last) category name.
    #[serde(default)]
    pub game_name: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Stream delay in seconds (only for the owner).
    #[serde(default)]
    pub delay: u64,
    /// Tags.
    #[serde(default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
    /// Content classification labels.
    #[serde(default, deserialize_with = "null_as_default")]
    pub content_classification_labels: Vec<String>,
    /// Branded content flag.
    #[serde(default)]
    pub is_branded_content: bool,
}

/// Kind of a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoType {
    /// Past broadcast (VOD).
    Archive,
    /// Highlight.
    Highlight,
    /// Upload.
    Upload,
    /// Anything Twitch adds later.
    #[serde(other)]
    Other,
}

/// A muted range of a VOD (copyrighted audio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutedSegment {
    /// Length in seconds.
    pub duration: u64,
    /// Start offset in seconds.
    pub offset: u64,
}

/// `GET /videos`: VOD, highlight or upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Video {
    /// Video id.
    pub id: String,
    /// Id of the stream it was recorded from (archives only).
    #[serde(default)]
    pub stream_id: Option<String>,
    /// Owner id.
    pub user_id: String,
    /// Owner login.
    pub user_login: String,
    /// Owner display name.
    pub user_name: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// RFC 3339 creation time.
    #[serde(default)]
    pub created_at: String,
    /// RFC 3339 publish time.
    #[serde(default)]
    pub published_at: String,
    /// Page URL on twitch.tv ("Open on Twitch").
    #[serde(default)]
    pub url: String,
    /// Thumbnail template with `%{width}`/`%{height}`; empty while processing.
    #[serde(default)]
    pub thumbnail_url: String,
    /// `public` (the only value today).
    #[serde(default)]
    pub viewable: String,
    /// Views.
    #[serde(default)]
    pub view_count: u64,
    /// Language.
    #[serde(default)]
    pub language: String,
    /// Kind.
    #[serde(rename = "type")]
    pub kind: VideoType,
    /// Duration as Helix sends it, e.g. `3h8m33s`.
    #[serde(default)]
    pub duration: String,
    /// Muted ranges (only with a user token).
    #[serde(default, deserialize_with = "null_as_default")]
    pub muted_segments: Vec<MutedSegment>,
}

impl Video {
    /// Parsed [`Video::duration`].
    pub fn duration(&self) -> Option<Duration> {
        parse_video_duration(&self.duration)
    }

    /// Thumbnail URL at the given size; `None` while Twitch still processes
    /// the video.
    pub fn thumbnail(&self, width: u32, height: u32) -> Option<String> {
        (!self.thumbnail_url.is_empty())
            .then(|| fill_image_template(&self.thumbnail_url, width, height))
    }
}

/// `GET /clips`: a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Clip id (slug).
    pub id: String,
    /// Page URL.
    #[serde(default)]
    pub url: String,
    /// Official embed URL (store builds play through it).
    #[serde(default)]
    pub embed_url: String,
    /// Channel id.
    pub broadcaster_id: String,
    /// Channel display name.
    #[serde(default)]
    pub broadcaster_name: String,
    /// Clipper id.
    #[serde(default)]
    pub creator_id: String,
    /// Clipper display name.
    #[serde(default)]
    pub creator_name: String,
    /// Source VOD id; empty when the VOD is gone.
    #[serde(default)]
    pub video_id: String,
    /// Category id.
    #[serde(default)]
    pub game_id: String,
    /// Language.
    #[serde(default)]
    pub language: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Views.
    #[serde(default)]
    pub view_count: u64,
    /// RFC 3339 creation time.
    #[serde(default)]
    pub created_at: String,
    /// Thumbnail URL.
    #[serde(default)]
    pub thumbnail_url: String,
    /// Length in seconds (fractional).
    #[serde(default)]
    pub duration: f64,
    /// Offset in the source VOD; `null` for a few minutes after creation.
    #[serde(default)]
    pub vod_offset: Option<u64>,
    /// Featured by the broadcaster.
    #[serde(default)]
    pub is_featured: bool,
}

/// `GET /games`, `GET /games/top`, `GET /search/categories`: a category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    /// Category id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Box art template with `{width}`/`{height}`.
    #[serde(default)]
    pub box_art_url: String,
    /// IGDB id (absent in search results), for cover art and genres.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub igdb_id: Option<String>,
}

impl Category {
    /// Box art URL at the given size (Twitch's own ratio is 285×380).
    pub fn box_art(&self, width: u32, height: u32) -> String {
        fill_image_template(&self.box_art_url, width, height)
    }
}

/// `GET /search/channels`: a channel match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSearchResult {
    /// Broadcaster id.
    pub id: String,
    /// Login.
    pub broadcaster_login: String,
    /// Display name.
    pub display_name: String,
    /// Language.
    #[serde(default)]
    pub broadcaster_language: String,
    /// Current (or last) category id.
    #[serde(default)]
    pub game_id: String,
    /// Current (or last) category name.
    #[serde(default)]
    pub game_name: String,
    /// Live now.
    #[serde(default)]
    pub is_live: bool,
    /// Tags.
    #[serde(default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
    /// Avatar URL (despite the name).
    #[serde(default)]
    pub thumbnail_url: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// RFC 3339 start of the current stream; `None` when offline.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub started_at: Option<String>,
}

/// Category reference inside a schedule segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryRef {
    /// Category id.
    pub id: String,
    /// Name.
    pub name: String,
}

/// One planned broadcast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSegment {
    /// Segment id.
    pub id: String,
    /// RFC 3339 start.
    pub start_time: String,
    /// RFC 3339 end (may be absent for open-ended segments).
    #[serde(default)]
    pub end_time: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Set when the broadcaster cancelled this occurrence.
    #[serde(default)]
    pub canceled_until: Option<String>,
    /// Planned category.
    #[serde(default)]
    pub category: Option<CategoryRef>,
    /// Part of a recurring series.
    #[serde(default)]
    pub is_recurring: bool,
}

/// Broadcaster vacation inside a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vacation {
    /// RFC 3339 start.
    pub start_time: String,
    /// RFC 3339 end.
    pub end_time: String,
}

/// `GET /schedule`: upcoming broadcasts ("Soon live" row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// Segments of this page.
    #[serde(default, deserialize_with = "null_as_default")]
    pub segments: Vec<ScheduleSegment>,
    /// Broadcaster id.
    pub broadcaster_id: String,
    /// Broadcaster display name.
    #[serde(default)]
    pub broadcaster_name: String,
    /// Broadcaster login.
    #[serde(default)]
    pub broadcaster_login: String,
    /// Current vacation, if any.
    #[serde(default)]
    pub vacation: Option<Vacation>,
}

/// Static image URLs of an emote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmoteImages {
    /// 1× image.
    #[serde(default)]
    pub url_1x: String,
    /// 2× image.
    #[serde(default)]
    pub url_2x: String,
    /// 4× image.
    #[serde(default)]
    pub url_4x: String,
}

/// `GET /chat/emotes*`: an emote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emote {
    /// Emote id.
    pub id: String,
    /// Text code.
    pub name: String,
    /// Static images.
    pub images: EmoteImages,
    /// Subscriber tier (`1000`, `2000`, `3000`) for channel emotes.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub tier: Option<String>,
    /// `subscriptions`, `follower`, `bitstier`, `globals`, …
    #[serde(default)]
    pub emote_type: Option<String>,
    /// Emote set id.
    #[serde(default)]
    pub emote_set_id: Option<String>,
    /// Owner (for `chat/emotes/set`).
    #[serde(default)]
    pub owner_id: Option<String>,
    /// `static`, `animated`.
    #[serde(default)]
    pub format: Vec<String>,
    /// `1.0`, `2.0`, `3.0`.
    #[serde(default)]
    pub scale: Vec<String>,
    /// `light`, `dark`.
    #[serde(default)]
    pub theme_mode: Vec<String>,
}

/// Emotes plus the CDN URL template (`{{id}}`, `{{format}}`,
/// `{{theme_mode}}`, `{{scale}}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmoteList {
    /// Emotes.
    pub emotes: Vec<Emote>,
    /// URL template.
    pub template: String,
}

impl EmoteList {
    /// CDN URL of an emote; prefers `animated` when the emote has it.
    pub fn url(&self, emote: &Emote, dark: bool, scale: &str) -> String {
        let format = if emote.format.iter().any(|f| f == "animated") {
            "animated"
        } else {
            "static"
        };
        self.template
            .replace("{{id}}", &emote.id)
            .replace("{{format}}", format)
            .replace("{{theme_mode}}", if dark { "dark" } else { "light" })
            .replace("{{scale}}", scale)
    }
}

/// One version of a chat badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BadgeVersion {
    /// Version id (e.g. months for `subscriber`).
    pub id: String,
    /// 18 px image.
    #[serde(default)]
    pub image_url_1x: String,
    /// 36 px image.
    #[serde(default)]
    pub image_url_2x: String,
    /// 72 px image.
    #[serde(default)]
    pub image_url_4x: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
}

/// `GET /chat/badges*`: a badge set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BadgeSet {
    /// Set id (`subscriber`, `moderator`, …).
    pub set_id: String,
    /// Versions.
    pub versions: Vec<BadgeVersion>,
}

/// `GET /chat/color`: a user's name colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserChatColor {
    /// User id.
    pub user_id: String,
    /// Login.
    #[serde(default)]
    pub user_login: String,
    /// Display name.
    #[serde(default)]
    pub user_name: String,
    /// `#RRGGBB`; `None` when the user never chose one.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub color: Option<String>,
}

/// `GET /chat/settings`: chat modes shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSettings {
    /// Broadcaster id.
    pub broadcaster_id: String,
    /// Emote-only mode.
    #[serde(default)]
    pub emote_mode: bool,
    /// Followers-only mode.
    #[serde(default)]
    pub follower_mode: bool,
    /// Minutes a user must follow; `None` when off.
    #[serde(default)]
    pub follower_mode_duration: Option<u64>,
    /// Slow mode.
    #[serde(default)]
    pub slow_mode: bool,
    /// Seconds between messages; `None` when off.
    #[serde(default)]
    pub slow_mode_wait_time: Option<u64>,
    /// Subscribers-only mode.
    #[serde(default)]
    pub subscriber_mode: bool,
    /// Unique-chat (R9K) mode.
    #[serde(default)]
    pub unique_chat_mode: bool,
}

impl HasId for User {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for FollowedChannel {
    fn id(&self) -> &str {
        &self.broadcaster_id
    }
}
impl HasId for Stream {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Video {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Clip {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Category {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for ChannelSearchResult {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for ScheduleSegment {
    fn id(&self) -> &str {
        &self.id
    }
}

fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

fn empty_as_none<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(de)?.filter(|s| !s.is_empty()))
}
