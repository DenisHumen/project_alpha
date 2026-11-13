// SPDX-License-Identifier: MPL-2.0
//! Music tags and stream properties through `lofty` (MIT OR Apache-2.0),
//! and the path fallback for files without tags (MUS-001).
//!
//! Formats: MP3 (ID3v2, ID3v1, APE), FLAC and Ogg Vorbis/Opus/Speex (Vorbis
//! Comments), MP4/M4A with AAC or ALAC (ilst), WAV and AIFF (ID3v2, RIFF
//! INFO), APE, WavPack, Musepack. DSF is read here: lofty does not know
//! it, but its tag is a plain ID3v2 block at an offset in the header.

use std::fmt;
use std::io::{BufReader, Cursor};

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::properties::FileProperties;
use lofty::tag::{Accessor, ItemKey, Tag};
use pa_library::music::{AudioInfo, ReplayGain, TrackTags, album_folder, disc_folder_number};
use pa_vfs::{ByteSource, SourceReader, Vfs, VfsError, VfsPath};

/// Largest DSF ID3v2 block read (covers are usually well below).
const MAX_DSF_TAG: u64 = 32 << 20;

/// Why the tags of a file could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum TagError {
    /// The file could not be opened or read.
    Vfs(VfsError),
    /// The file is not in a supported format or is damaged.
    Parse(String),
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vfs(e) => write!(f, "read: {e}"),
            Self::Parse(m) => write!(f, "tags: {m}"),
        }
    }
}

impl std::error::Error for TagError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Vfs(e) => Some(e),
            Self::Parse(_) => None,
        }
    }
}

impl From<VfsError> for TagError {
    fn from(e: VfsError) -> Self {
        Self::Vfs(e)
    }
}

impl From<std::io::Error> for TagError {
    fn from(e: std::io::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<lofty::error::FileParseError> for TagError {
    fn from(e: lofty::error::FileParseError) -> Self {
        Self::Parse(e.to_string())
    }
}

/// A picture embedded in an audio file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedArt {
    /// MIME type, e.g. `image/jpeg`, if the tag gives one.
    pub mime: Option<String>,
    pub data: Vec<u8>,
}

/// Reads the tags and stream properties of the audio file at `path`.
///
/// Only the tags are returned; the path fallback is [`with_path_fallback`].
/// Blocking: reads the file's headers and tags (a few round trips on a
/// network source).
pub fn read_tags(vfs: &dyn Vfs, path: &VfsPath) -> Result<TrackTags, TagError> {
    let (file, dsf_audio) = probe(vfs, path, true)?;
    let mut tags = TrackTags::default();
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        fill(&mut tags, tag);
    }
    tags.has_embedded_art = file.tags().iter().any(|t| !t.pictures().is_empty());
    tags.audio = dsf_audio.unwrap_or_else(|| audio_info(file.file_type(), file.properties()));
    Ok(tags)
}

/// Reads the embedded cover of an audio file: the front cover, else the
/// first picture.
pub fn read_embedded_art(vfs: &dyn Vfs, path: &VfsPath) -> Result<Option<EmbeddedArt>, TagError> {
    let (file, _) = probe(vfs, path, true)?;
    let pictures = || file.tags().iter().flat_map(|t| t.pictures());
    let picture = pictures()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures().next());
    Ok(picture.map(|p| EmbeddedArt {
        mime: p.mime_type().map(|m| m.as_str().to_owned()),
        data: p.data().to_vec(),
    }))
}

fn options(cover_art: bool, properties: bool) -> ParseOptions {
    ParseOptions::new()
        .parsing_mode(ParsingMode::BestAttempt)
        .read_cover_art(cover_art)
        .read_properties(properties)
}

/// Opens and parses the file; for DSF also returns the stream properties
/// read from its header.
fn probe(
    vfs: &dyn Vfs,
    path: &VfsPath,
    cover_art: bool,
) -> Result<(TaggedFile, Option<AudioInfo>), TagError> {
    let source = vfs.open(path)?;
    let is_dsf = path
        .file_name()
        .and_then(|n| n.rsplit_once('.'))
        .is_some_and(|(_, e)| e.eq_ignore_ascii_case("dsf"));
    if is_dsf {
        return read_dsf(&*source, cover_art).map(|(f, a)| (f, Some(a)));
    }
    let reader = BufReader::with_capacity(64 * 1024, SourceReader::new(source));
    let file = Probe::new(reader)
        .options(options(cover_art, true))
        .guess_file_type()?
        .read()?;
    Ok((file, None))
}

/// DSF (Sony DSD Stream File): a 28-byte `DSD ` chunk with the offset of the
/// ID3v2 metadata, then a 52-byte `fmt ` chunk with the stream format.
fn read_dsf(src: &dyn ByteSource, cover_art: bool) -> Result<(TaggedFile, AudioInfo), TagError> {
    let mut head = [0u8; 80];
    src.read_exact_at(0, &mut head)?;
    if &head[0..4] != b"DSD " || &head[28..32] != b"fmt " {
        return Err(TagError::Parse("not a DSF file".into()));
    }
    let u32_at = |i: usize| u32::from_le_bytes(head[i..i + 4].try_into().unwrap_or_default());
    let u64_at = |i: usize| u64::from_le_bytes(head[i..i + 8].try_into().unwrap_or_default());
    let (channels, rate, samples) = (u32_at(52), u32_at(56), u64_at(64));
    let audio = AudioInfo {
        codec: Some("dsd".into()),
        lossless: Some(true),
        duration_ms: (rate > 0).then(|| samples.saturating_mul(1000) / u64::from(rate)),
        sample_rate_hz: (rate > 0).then_some(rate),
        bit_depth: Some(1),
        channels: u8::try_from(channels).ok().filter(|c| *c > 0),
        bitrate_kbps: u32::try_from(u64::from(rate) * u64::from(channels) / 1000)
            .ok()
            .filter(|b| *b > 0),
    };

    // The tag block alone is not a format lofty reads, so it is handed over
    // as an MP3 file: the ID3v2 block followed by two silent MPEG frames.
    let mut tag = Vec::new();
    let meta = u64_at(20);
    if meta > 0 && meta.saturating_add(10) <= src.size() {
        let mut id3 = [0u8; 10];
        src.read_exact_at(meta, &mut id3)?;
        if &id3[0..3] == b"ID3" {
            let syncsafe = id3[6..10]
                .iter()
                .fold(0u64, |acc, b| (acc << 7) | u64::from(b & 0x7f));
            let footer = if id3[5] & 0x10 != 0 { 10 } else { 0 };
            let len = (10 + syncsafe + footer)
                .min(MAX_DSF_TAG)
                .min(src.size() - meta);
            tag = vec![0u8; usize::try_from(len).unwrap_or(0)];
            src.read_exact_at(meta, &mut tag)?;
        }
    }
    for _ in 0..2 {
        // MPEG-1 Layer III, 128 kbit/s, 44.1 kHz: 417-byte frames.
        tag.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        tag.resize(tag.len() + 413, 0);
    }
    let file = Probe::new(Cursor::new(tag))
        .set_file_type(FileType::Mpeg)
        .options(options(cover_art, false))
        .read()?;
    Ok((file, audio))
}

fn audio_info(file_type: FileType, p: &FileProperties) -> AudioInfo {
    let (codec, lossless) = match file_type {
        FileType::Mpeg => ("mp3", false),
        FileType::Aac => ("aac", false),
        FileType::Flac => ("flac", true),
        // ALAC (and FLAC in MP4) report a bit depth; AAC does not.
        FileType::Mp4 if p.bit_depth().is_some() => ("alac", true),
        FileType::Mp4 => ("aac", false),
        FileType::Opus => ("opus", false),
        FileType::Vorbis => ("vorbis", false),
        FileType::Speex => ("speex", false),
        FileType::Wav | FileType::Aiff => ("pcm", true),
        FileType::Ape => ("ape", true),
        FileType::WavPack => ("wavpack", true),
        FileType::Mpc => ("musepack", false),
        _ => ("", false),
    };
    let duration = u64::try_from(p.duration().as_millis()).unwrap_or(u64::MAX);
    AudioInfo {
        codec: (!codec.is_empty()).then(|| codec.to_owned()),
        lossless: (!codec.is_empty()).then_some(lossless),
        duration_ms: (duration > 0).then_some(duration),
        sample_rate_hz: p.sample_rate().filter(|r| *r > 0),
        bit_depth: p.bit_depth().filter(|b| *b > 0),
        channels: p.channels().filter(|c| *c > 0),
        bitrate_kbps: p.audio_bitrate().or(p.overall_bitrate()).filter(|b| *b > 0),
    }
}

fn text(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key)
        .map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// All values of a multi-valued field: repeated fields (Vorbis), and
/// values separated by NUL (ID3v2.4) or `;`.
fn values(tag: &Tag, key: ItemKey) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in tag.get_strings(key) {
        for part in v
            .split(['\0', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            if !out.iter().any(|o| o == part) {
                out.push(part.to_owned());
            }
        }
    }
    out
}

/// Leading 4-digit year of a date string (`2001`, `2001-05-04`, `2001/05`).
fn year_of(s: &str) -> Option<u16> {
    let digits = s.trim().get(..4)?;
    digits
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| digits.parse().ok())
        .flatten()
        .filter(|y| (1000..=3000).contains(y))
}

/// `-6.54 dB`, `+1.2 dB`, `-6,54` → dB.
fn gain_db(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s
        .strip_suffix("dB")
        .or_else(|| s.strip_suffix("db"))
        .unwrap_or(s);
    s.trim()
        .replace(',', ".")
        .parse()
        .ok()
        .filter(|g: &f64| g.is_finite())
}

/// Opus `R128_*_GAIN`: Q7.8 dB relative to -23 LUFS; ReplayGain uses -18 LUFS.
fn r128_to_replay_gain(s: &str) -> Option<f64> {
    let q: i32 = s.trim().parse().ok()?;
    Some(f64::from(q) / 256.0 + 5.0)
}

fn flag(s: Option<String>) -> bool {
    s.is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn fill(t: &mut TrackTags, tag: &Tag) {
    t.title = tag
        .title()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    t.artists = values(tag, ItemKey::TrackArtists);
    if t.artists.is_empty() {
        t.artists = values(tag, ItemKey::TrackArtist);
    }
    t.album_artists = values(tag, ItemKey::AlbumArtist);
    t.album = tag
        .album()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    t.composers = values(tag, ItemKey::Composer);
    t.genres = values(tag, ItemKey::Genre);
    t.track_number = tag.track();
    t.track_total = tag.track_total();
    t.disc_number = tag.disk();
    t.disc_total = tag.disk_total();
    t.year = tag
        .date()
        .map(|d| d.year)
        .filter(|y| (1000..=3000).contains(y));
    for key in [
        ItemKey::RecordingDate,
        ItemKey::Year,
        ItemKey::ReleaseDate,
        ItemKey::OriginalReleaseDate,
    ] {
        if t.year.is_none() {
            t.year = text(tag, key).as_deref().and_then(year_of);
        }
    }
    t.compilation = flag(text(tag, ItemKey::FlagCompilation));
    t.artist_sort_name = text(tag, ItemKey::TrackArtistSortOrder);
    t.artist_mbid = text(tag, ItemKey::MusicBrainzArtistId);
    t.recording_mbid = text(tag, ItemKey::MusicBrainzRecordingId);
    t.release_mbid = text(tag, ItemKey::MusicBrainzReleaseId);
    t.release_group_mbid = text(tag, ItemKey::MusicBrainzReleaseGroupId);

    let num = |key| text(tag, key).as_deref().and_then(gain_db);
    t.replay_gain = ReplayGain {
        track_gain_db: num(ItemKey::ReplayGainTrackGain).or_else(|| {
            text(tag, ItemKey::R128TrackGain)
                .as_deref()
                .and_then(r128_to_replay_gain)
        }),
        track_peak: num(ItemKey::ReplayGainTrackPeak),
        album_gain_db: num(ItemKey::ReplayGainAlbumGain).or_else(|| {
            text(tag, ItemKey::R128AlbumGain)
                .as_deref()
                .and_then(r128_to_replay_gain)
        }),
        album_peak: num(ItemKey::ReplayGainAlbumPeak),
    };
}

/// Track information guessed from a path (release-0.1 rules,
/// `pa_library` Dart `trackInfoFromPath`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathInfo {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
}

/// Folder names that never name an album or an artist (lowercase).
const GENERIC_FOLDERS: &[&str] = &[
    "music",
    "audio",
    "mp3",
    "flac",
    "media",
    "downloads",
    "songs",
    "tracks",
    "misc",
    "unsorted",
    "unknown",
    "music library",
    "my music",
    "музыка",
];

fn usable_folder(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    !lower.is_empty() && !GENERIC_FOLDERS.contains(&lower.as_str())
}

/// Splits a `NN - `, `NN. `, `D-NN ` track-number prefix off a file stem.
fn split_track_prefix(stem: &str) -> (Option<u32>, Option<u32>, &str) {
    let b = stem.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = 0;
    let mut disc = None;
    let n = digits(0);
    if (1..=2).contains(&n) && matches!(b.get(n), Some(b'-' | b'.')) && digits(n + 1) > 0 {
        disc = stem[..n].parse().ok();
        i = n + 1;
    }
    let n = digits(i);
    if !(1..=3).contains(&n) {
        return (None, None, stem);
    }
    let number = stem[i..i + n].parse().ok();
    let rest = &stem[i + n..];
    let trimmed = rest.trim_start_matches([' ', '.', '_', '-']);
    if trimmed.len() == rest.len() || trimmed.is_empty() {
        return (None, None, stem);
    }
    (number, disc, trimmed)
}

/// `Artist/Album/NN - Title.ext`, `Artist - Album (Year)/NN. Title.ext`,
/// `Artist - Title.ext`; `CD1` subfolders give the disc number.
#[must_use]
pub fn path_info(path: &str) -> PathInfo {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name
        .rsplit_once('.')
        .map_or(name, |(s, _)| s)
        .replace('_', " ");
    let (track_number, mut disc_number, rest) = split_track_prefix(stem.trim());
    let mut info = PathInfo {
        title: rest.trim().to_owned(),
        track_number,
        ..PathInfo::default()
    };
    let stem_title = info.title.clone();
    if let Some((artist, title)) = stem_title.split_once(" - ")
        && !artist.trim().is_empty()
        && !title.trim().is_empty()
    {
        info.artist = Some(artist.trim().to_owned());
        info.title = title.trim().to_owned();
    }

    let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
    let parent_name = parent.rsplit('/').next().unwrap_or("");
    if let Some(n) = disc_folder_number(parent_name) {
        disc_number = Some(n);
    }
    info.disc_number = disc_number;

    let album_dir = album_folder(path);
    if album_dir != "/" {
        let (above, folder) = album_dir.rsplit_once('/').unwrap_or(("", album_dir));
        if usable_folder(folder) {
            let mut a = strip_year(folder);
            if let Some((artist, album)) = a.clone().split_once(" - ") {
                info.artist.get_or_insert_with(|| artist.trim().to_owned());
                a = album.trim().to_owned();
            } else if info.artist.is_none() {
                let artist = above.rsplit('/').next().unwrap_or("");
                if usable_folder(artist) {
                    info.artist = Some(artist.to_owned());
                }
            }
            info.album = (!a.is_empty()).then_some(a);
        }
    }
    info
}

/// Removes a `(1999)` / `[1999]` part and a `1999 - ` prefix.
fn strip_year(folder: &str) -> String {
    let is_year = |s: &str| s.len() == 4 && (s.starts_with("19") || s.starts_with("20"));
    let mut out = folder.trim().to_owned();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some(start) = out.find(open)
            && let Some(len) = out[start..].find(close)
            && is_year(&out[start + 1..start + len])
        {
            out = format!("{} {}", &out[..start], &out[start + len + 1..]);
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    match out
        .get(..4)
        .filter(|y| is_year(y) && y.bytes().all(|b| b.is_ascii_digit()))
    {
        Some(_) => {
            let rest = out[4..].trim_start();
            match rest.strip_prefix(['-', '.']) {
                Some(r) if !r.trim().is_empty() => r.trim().to_owned(),
                _ => out,
            }
        }
        None => out,
    }
}

/// Fills what the tags lack from [`path_info`].
#[must_use]
pub fn with_path_fallback(mut tags: TrackTags, path: &str) -> TrackTags {
    let p = path_info(path);
    if tags.title.is_none() && !p.title.is_empty() {
        tags.title = Some(p.title);
    }
    if tags.artists.is_empty() {
        tags.artists.extend(p.artist);
    }
    if tags.album.is_none() {
        tags.album = p.album;
    }
    tags.track_number = tags.track_number.or(p.track_number);
    tags.disc_number = tags.disc_number.or(p.disc_number);
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_info_reads_common_layouts() {
        let p = path_info("/Pink Floyd/The Wall (1979)/CD2/03 - Hey You.flac");
        assert_eq!(p.title, "Hey You");
        assert_eq!(p.track_number, Some(3));
        assert_eq!(p.disc_number, Some(2));
        assert_eq!(p.album.as_deref(), Some("The Wall"));
        assert_eq!(p.artist.as_deref(), Some("Pink Floyd"));

        let p = path_info("/Music/Daft Punk - Discovery [2001]/1-02. Aerodynamic.mp3");
        assert_eq!(
            (p.disc_number, p.track_number, p.title.as_str()),
            (Some(1), Some(2), "Aerodynamic")
        );
        assert_eq!(p.album.as_deref(), Some("Discovery"));
        assert_eq!(p.artist.as_deref(), Some("Daft Punk"));

        let p = path_info("/Artist - Song.ogg");
        assert_eq!(p.artist.as_deref(), Some("Artist"));
        assert_eq!(p.title, "Song");
        assert_eq!(p.album, None);

        let p = path_info("/Music/1999.mp3");
        assert_eq!((p.title.as_str(), p.track_number), ("1999", None));
        assert_eq!(strip_year("2001 - Discovery"), "Discovery");
    }

    #[test]
    fn gains_and_years_parse() {
        assert_eq!(gain_db("-6.54 dB"), Some(-6.54));
        assert_eq!(gain_db("+1,5 dB"), Some(1.5));
        assert_eq!(gain_db("loud"), None);
        assert_eq!(r128_to_replay_gain("-512"), Some(3.0));
        assert_eq!(year_of("2001-05-04"), Some(2001));
        assert_eq!(year_of("01"), None);
    }
}
