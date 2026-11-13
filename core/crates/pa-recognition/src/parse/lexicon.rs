// SPDX-License-Identifier: MPL-2.0
//! Dictionaries of release tokens.
//!
//! Written from scratch for project_alpha from public naming conventions of
//! scene, fansub and Russian releases. Lookups are exact matches on the
//! lower-cased token, so they run in linear time.

use super::types::{EpisodeKind, ExtraKind};

/// Meaning of a dictionary token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lex {
    Source,
    VideoCodec,
    AudioCodec,
    Hdr,
    /// ISO 639-2 language code (or `""` for "multi/dual" markers).
    Lang(&'static str),
    Edition,
    Other,
    Extra(ExtraKind),
    Kind(EpisodeKind),
    /// Known release group (mostly fansub groups).
    Group,
    /// Dubbing studio / tracker (Russian releases; implies `rus`).
    Dub,
}

/// A dictionary entry: meaning and whether it is *weak* (a common word that
/// counts only next to other release tokens or inside brackets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub lex: Lex,
    pub weak: bool,
}

const fn strong(lex: Lex) -> Option<Entry> {
    Some(Entry { lex, weak: false })
}

const fn weak(lex: Lex) -> Option<Entry> {
    Some(Entry { lex, weak: true })
}

/// Look up a single lower-cased token.
pub(crate) fn lookup(lower: &str) -> Option<Entry> {
    use Lex::*;
    match lower {
        // Sources.
        "bluray" | "blu-ray" | "bdrip" | "bd-rip" | "brrip" | "br-rip" | "bdremux" | "remux"
        | "web-dl" | "webdl" | "webrip" | "web-rip" | "webdlrip" | "web-dlrip" | "hdtv"
        | "hdtvrip" | "dvdrip" | "dvd-rip" | "dvdscr" | "hdrip" | "tvrip" | "satrip"
        | "camrip" | "hdcam" | "telesync" | "telecine" | "uhdrip" | "iptvrip" | "dvd5"
        | "dvd9" | "hddvd" | "hd-dvd" | "vhsrip" | "ppvrip" | "dsrip" | "pdtv" | "bdmv"
        | "amzn" | "nf" | "dsnp" | "hmax" | "atvp" | "itunes" => {
            if matches!(lower, "amzn" | "nf" | "dsnp" | "hmax" | "atvp" | "itunes") {
                // Streaming service tags: treated as release flags, not source.
                weak(Other)
            } else {
                strong(Source)
            }
        }
        "web" | "bd" | "dvd" | "uhd" | "ts" | "tc" | "cam" | "scr" | "r5" | "hdtc" | "sdtv"
        | "dsr" | "tv" => weak(Source),
        // Video codecs and depth.
        "x264" | "x265" | "h264" | "h265" | "h.264" | "h.265" | "hevc" | "avc" | "xvid"
        | "divx" | "av1" | "vp9" | "mpeg2" | "mpeg-2" | "10bit" | "10-bit" | "hi10p" | "hi10"
        | "8bit" | "8-bit" | "12bit" | "vc-1" | "vc1" | "x264-10bit" | "hevc-10bit" => {
            strong(VideoCodec)
        }
        // Audio.
        "aac" | "ac3" | "eac3" | "e-ac-3" | "e-ac3" | "dts" | "dts-hd" | "dtshd" | "dts-x"
        | "dts-ma" | "truehd" | "atmos" | "flac" | "mp3" | "opus" | "ddp" | "dd+" | "lpcm"
        | "dd" | "2ch" | "6ch" | "8ch" | "aac2" | "dts-es" => strong(AudioCodec),
        "pcm" | "ma" | "5.1" | "7.1" | "2.0" => weak(AudioCodec),
        // HDR.
        "hdr" | "hdr10" | "hdr10+" | "hdr10plus" | "dovi" | "hlg" | "dolbyvision" => strong(Hdr),
        "dv" | "sdr" => weak(Hdr),
        // Languages.
        "rus" | "russian" => strong(Lang("rus")),
        "eng" | "english" => strong(Lang("eng")),
        "jap" | "jpn" | "japanese" => strong(Lang("jpn")),
        "ukr" | "ukrainian" => strong(Lang("ukr")),
        "ger" | "deu" | "german" => strong(Lang("ger")),
        "fre" | "fra" | "french" | "vostfr" | "vff" | "truefrench" => strong(Lang("fre")),
        "spa" | "spanish" | "castellano" => strong(Lang("spa")),
        "ita" | "italian" => strong(Lang("ita")),
        "kor" | "korean" => strong(Lang("kor")),
        "chi" | "chs" | "cht" | "chinese" | "gb" | "big5" => {
            if matches!(lower, "gb" | "big5") {
                weak(Lang("chi"))
            } else {
                strong(Lang("chi"))
            }
        }
        "multi" | "multi-audio" | "multi-subs" | "multisub" | "multisubs" | "dual-audio"
        | "multi-sub" | "dualaudio" => strong(Lang("")),
        "sub" | "subs" | "subbed" | "dub" | "dubbed" | "raw" | "raws" | "hardsub" | "softsub" => {
            weak(Other)
        }
        "mvo" | "dvo" | "avo" | "дубляж" | "многоголосый" | "одноголосый" => strong(Lang("rus")),
        // Editions.
        "extended" | "unrated" | "uncut" | "remastered" | "imax" | "theatrical" | "criterion"
        | "redux" | "restored" | "director's" | "directors" => weak(Edition),
        // Other release flags.
        "proper" | "repack" | "rerip" | "internal" | "readnfo" | "nfofix" | "batch" | "hfr"
        | "60fps" | "uncensored" | "sbs" | "hsbs" | "3d" | "complete" | "limited" | "dubbed-rus" => {
            if matches!(lower, "complete" | "limited" | "3d") {
                weak(Other)
            } else {
                strong(Other)
            }
        }
        // Extras words.
        "trailer" | "trailers" => weak(Extra(ExtraKind::Trailer)),
        "teaser" => weak(Extra(ExtraKind::Teaser)),
        "featurette" | "featurettes" => strong(Extra(ExtraKind::Featurette)),
        "behindthescenes" | "makingof" | "making-of" => strong(Extra(ExtraKind::BehindTheScenes)),
        "deleted" | "deletedscene" | "deletedscenes" => weak(Extra(ExtraKind::DeletedScene)),
        "interview" | "interviews" => weak(Extra(ExtraKind::Interview)),
        "sample" => strong(Extra(ExtraKind::Sample)),
        "menu" => weak(Extra(ExtraKind::Menu)),
        "pv" | "cm" | "preview" => weak(Extra(ExtraKind::Preview)),
        // Episode kinds.
        "ncop" | "nc-op" | "creditless-op" => strong(Kind(EpisodeKind::Ncop)),
        "nced" | "nc-ed" | "creditless-ed" => strong(Kind(EpisodeKind::Nced)),
        "ova" | "oad" => strong(Kind(EpisodeKind::Ova)),
        "ona" => strong(Kind(EpisodeKind::Ona)),
        "sp" | "sps" => weak(Kind(EpisodeKind::Sp)),
        "special" | "specials" => weak(Kind(EpisodeKind::Special)),
        "recap" => weak(Kind(EpisodeKind::Recap)),
        _ => {
            if is_known_group(lower) {
                strong(Group)
            } else if is_dub_studio(lower) {
                strong(Dub)
            } else {
                None
            }
        }
    }
}

/// Audio tokens with channel layout glued on: `DD5.1`, `AAC2.0`, `DDP5.1`, `TrueHD7.1`.
pub(crate) fn is_audio_with_channels(lower: &str) -> bool {
    const PREFIXES: [&str; 11] = [
        "ddp", "dd+", "dd", "aac", "ac3", "eac3", "dts", "truehd", "flac", "opus", "lpcm",
    ];
    for p in PREFIXES {
        if let Some(rest) = lower.strip_prefix(p) {
            let b = rest.as_bytes();
            if b.len() == 3 && b[0].is_ascii_digit() && b[1] == b'.' && b[2].is_ascii_digit() {
                return true;
            }
            if b.len() == 1 && b[0].is_ascii_digit() {
                return true;
            }
        }
    }
    false
}

/// Fansub and release groups that also appear outside brackets.
fn is_known_group(lower: &str) -> bool {
    matches!(
        lower,
        "subsplease"
            | "erai-raws"
            | "horriblesubs"
            | "judas"
            | "ember"
            | "asw"
            | "yameii"
            | "commie"
            | "gjm"
            | "doki"
            | "coalgirls"
            | "lilith-raws"
            | "ohys-raws"
            | "leopard-raws"
            | "beatrice-raws"
            | "reinforce"
            | "vcb-studio"
            | "moozzi2"
            | "kaleido-subs"
            | "tsundere-raws"
            | "sallysubs"
            | "nc-raws"
            | "anime time"
            | "animetime"
            | "golumpa"
            | "dameda"
            | "cyc"
            | "kawaiika-raws"
            | "dkb"
            | "neohvec"
            | "mtbb"
            | "tenrai-sensei"
    )
}

/// Russian dubbing studios and trackers (all imply Russian audio).
pub(crate) fn is_dub_studio(lower: &str) -> bool {
    matches!(
        lower,
        "lostfilm"
            | "lostfilm.tv"
            | "newstudio"
            | "baibako"
            | "amedia"
            | "coldfilm"
            | "hdrezka"
            | "jaskier"
            | "ideafilm"
            | "alexfilm"
            | "novafilm"
            | "kerob"
            | "anilibria"
            | "anilibria.tv"
            | "anidub"
            | "animevost"
            | "shiza"
            | "sovetromantica"
            | "studioband"
            | "ozz"
            | "ozz.tv"
            | "rudub"
            | "redheadsound"
            | "kubik"
            | "kurazh-bambey"
            | "кураж-бамбей"
            | "jam"
            | "dreamcast"
            | "tvshows"
            | "gears"
            | "flarrow"
            | "lakefilms"
            | "anistar"
            | "animedia"
    )
}

/// Multi-word dictionary entries, matched on consecutive lower-case tokens.
/// The value is the meaning of the whole phrase.
pub(crate) const PHRASES: &[(&[&str], Lex)] = &[
    (&["director's", "cut"], Lex::Edition),
    (&["directors", "cut"], Lex::Edition),
    (&["extended", "cut"], Lex::Edition),
    (&["extended", "edition"], Lex::Edition),
    (&["special", "edition"], Lex::Edition),
    (&["final", "cut"], Lex::Edition),
    (&["ultimate", "edition"], Lex::Edition),
    (&["collector's", "edition"], Lex::Edition),
    (&["anniversary", "edition"], Lex::Edition),
    (&["theatrical", "cut"], Lex::Edition),
    (&["open", "matte"], Lex::Edition),
    (&["dual", "audio"], Lex::Lang("")),
    (&["multiple", "subtitle"], Lex::Lang("")),
    (&["multiple", "subtitles"], Lex::Lang("")),
    (&["multi", "subs"], Lex::Lang("")),
    (&["dolby", "vision"], Lex::Hdr),
    (&["uhd", "bluray"], Lex::Source),
    (&["uhd", "blu-ray"], Lex::Source),
    (&["web", "dl"], Lex::Source),
    (&["behind", "the", "scenes"], Lex::Extra(ExtraKind::BehindTheScenes)),
    (&["making", "of"], Lex::Extra(ExtraKind::BehindTheScenes)),
    (&["deleted", "scenes"], Lex::Extra(ExtraKind::DeletedScene)),
    (&["deleted", "scene"], Lex::Extra(ExtraKind::DeletedScene)),
    (&["kubik", "v", "kube"], Lex::Dub),
    (&["кубик", "в", "кубе"], Lex::Dub),
    (&["shiza", "project"], Lex::Dub),
    (&["dream", "cast"], Lex::Dub),
    (&["studio", "band"], Lex::Dub),
    (&["red", "head", "sound"], Lex::Dub),
    (&["gears", "media"], Lex::Dub),
    (&["hdrezka", "studio"], Lex::Dub),
    (&["nekomoe", "kissaten"], Lex::Group),
    (&["anime", "time"], Lex::Group),
    (&["creditless", "opening"], Lex::Kind(EpisodeKind::Ncop)),
    (&["creditless", "ending"], Lex::Kind(EpisodeKind::Nced)),
    (&["nc", "op"], Lex::Kind(EpisodeKind::Ncop)),
    (&["nc", "ed"], Lex::Kind(EpisodeKind::Nced)),
];

/// Known media/subtitle/sidecar extensions (lower-case, without dot).
pub(crate) fn is_known_extension(ext: &str) -> bool {
    matches!(
        ext,
        "mkv"
            | "mp4"
            | "m4v"
            | "avi"
            | "mov"
            | "wmv"
            | "ts"
            | "m2ts"
            | "mts"
            | "vob"
            | "iso"
            | "img"
            | "webm"
            | "flv"
            | "mpg"
            | "mpeg"
            | "ogm"
            | "ogv"
            | "rmvb"
            | "rm"
            | "divx"
            | "3gp"
            | "asf"
            | "srt"
            | "ass"
            | "ssa"
            | "sub"
            | "idx"
            | "sup"
            | "vtt"
            | "nfo"
            | "txt"
            | "jpg"
            | "jpeg"
            | "png"
            | "mka"
            | "mks"
            | "ifo"
            | "bup"
            | "strm"
    )
}

/// Kind of library root folder (`Movies/`, `Аниме/`…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootKind {
    Movies,
    Shows,
    Anime,
    AnimeMovies,
    Generic,
}

pub(crate) fn library_root(lower: &str) -> Option<RootKind> {
    Some(match lower {
        "movies" | "movie" | "films" | "film" | "фильмы" | "кино" | "кинофильмы" => {
            RootKind::Movies
        }
        "tv" | "tv shows" | "tvshows" | "tv series" | "shows" | "series" | "сериалы" | "тв"
        | "телешоу" | "documentaries" | "документальные" => RootKind::Shows,
        "anime" | "аниме" | "anime series" | "anime tv" => RootKind::Anime,
        "anime movies" | "anime films" | "аниме фильмы" => RootKind::AnimeMovies,
        "downloads" | "download" | "загрузки" | "video" | "videos" | "видео" | "media"
        | "медиа" | "мультфильмы" | "cartoons" | "kids" | "torrents" | "complete"
        | "incoming" | "library" | "share" | "public" => RootKind::Generic,
        _ => return None,
    })
}

/// Extras folder (`Extras/`, `Featurettes/`, `NCOP/`…).
pub(crate) fn extras_folder(lower: &str) -> Option<ExtraKind> {
    Some(match lower {
        "extras" | "extra" | "bonus" | "bonuses" | "бонусы" | "доп. материалы"
        | "дополнительные материалы" | "other" | "others" | "misc" => ExtraKind::Other,
        "featurettes" | "featurette" => ExtraKind::Featurette,
        "behind the scenes" | "making of" => ExtraKind::BehindTheScenes,
        "deleted scenes" => ExtraKind::DeletedScene,
        "interviews" => ExtraKind::Interview,
        "scenes" => ExtraKind::Scene,
        "shorts" => ExtraKind::Short,
        "trailers" | "trailer" => ExtraKind::Trailer,
        "samples" | "sample" => ExtraKind::Sample,
        "ncop" | "op" | "openings" => ExtraKind::Opening,
        "nced" | "ed" | "endings" => ExtraKind::Ending,
        "nc" | "ncop&nced" | "ncop & nced" | "creditless" | "nc op ed" => ExtraKind::Other,
        "menu" | "menus" => ExtraKind::Menu,
        "cm" | "pv" | "previews" | "preview" | "cms" | "pvs" => ExtraKind::Preview,
        _ => return None,
    })
}

/// Folders inside a disc structure that never carry a title.
pub(crate) fn is_disc_internal_folder(lower: &str) -> bool {
    matches!(
        lower,
        "stream"
            | "playlist"
            | "clipinf"
            | "certificate"
            | "backup"
            | "meta"
            | "bdjo"
            | "jar"
            | "auxdata"
            | "audio_ts"
    )
}
