// SPDX-License-Identifier: MPL-2.0
//! Structural folder analysis (LIB-007, `docs/research/03` §4.2, stage 1).
//!
//! [`analyze`] turns a listing of a source (paths, sizes and, when
//! `pa-probe` already ran, durations) into [`Group`]s, without network and
//! without opening files:
//!
//! - **disc structures** — a folder containing `BDMV/` or `VIDEO_TS/` is one
//!   item named by that folder (A12); its main title is the longest
//!   `BDMV/STREAM/*.m2ts`, or the VOBs of the largest DVD title set as
//!   parts. An `.iso` file is one item;
//! - **parts** — `cd1`/`cd2`, `part1`, `pt1`, `disc1`, `disk1` of one name
//!   in one folder are joined into one item (A13), numbered 1..N without
//!   gaps;
//! - **extras** — Plex-style suffixes (`-trailer`, `-featurette`…), extras
//!   folders (`Featurettes/`, `Extras/`, `Trailers/`…), anime bonus tokens
//!   (`NCOP`, `NCED`, `Menu01`, `PV`, `CM`, `Preview`) and samples (A10);
//!   optionally a file shorter than [`StructureOptions::short_ratio`] of the
//!   longest video of its folder is an extra (or, inside a part stack, an
//!   ignored leftover). Files without a known duration are never judged by
//!   duration;
//! - **sidecar subtitles** — `movie.ru.forced.srt`, `Subs/2_English.srt`,
//!   with language and forced / SDH / default flags;
//! - **NFO files** — `movie.nfo`, `<video>.nfo`, `tvshow.nfo`; the ids inside
//!   are read by [`analyze_folders`](crate::analyze_folders) with
//!   [`parse_nfo_ids`];
//! - **local art** — `poster`, `folder`, `fanart`, `clearlogo`, `banner`,
//!   `<video>-poster`, `season01-poster`…
//!
//! The patterns are written for this project from the public descriptions
//! of the Kodi, Plex and Jellyfin naming conventions; no code of those
//! projects is used.

use std::collections::{BTreeMap, HashMap, HashSet};

pub use pa_library::groups::{
    ArtKind, DiscUnit, ExtraType, Group, GroupEntry, GroupKind, Role, SubtitleInfo,
};

use crate::rules::{VIDEO_EXTENSIONS, split_ext};

/// One file of the listing given to [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// Path relative to the source root (`/Movies/Dune/Dune.mkv` or
    /// `Movies/Dune/Dune.mkv`); groups use the same form.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Duration from `pa-probe`, if known.
    pub duration_ms: Option<u64>,
}

impl ListedFile {
    /// A file without a known duration.
    #[must_use]
    pub fn new(path: impl Into<String>, size: u64) -> Self {
        Self {
            path: path.into(),
            size,
            duration_ms: None,
        }
    }
}

/// Tuning of [`analyze`].
#[derive(Debug, Clone, PartialEq)]
pub struct StructureOptions {
    /// A video shorter than this share of the longest video of its folder
    /// is an extra when the folder holds exactly one long item, and a part
    /// that short is a leftover ([`Role::Ignore`]). `None` turns the rule
    /// off; it never applies to files without a known duration.
    pub short_ratio: Option<f64>,
}

impl Default for StructureOptions {
    fn default() -> Self {
        Self {
            short_ratio: Some(0.2),
        }
    }
}

/// Sidecar subtitle extensions, lowercase.
pub const SUBTITLE_EXTENSIONS: &[&str] = &[
    "srt", "ass", "ssa", "sub", "idx", "sup", "vtt", "smi", "ttml",
];

/// Local art extensions, lowercase.
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "tbn"];

/// Kind of a file by extension, for the analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    Video,
    Iso,
    Subtitle,
    Nfo,
    Image,
    Other,
}

fn file_type(ext: &str) -> FileType {
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    if ext == "iso" {
        FileType::Iso
    } else if VIDEO_EXTENSIONS.contains(&ext) || ext == "vob" {
        FileType::Video
    } else if SUBTITLE_EXTENSIONS.contains(&ext) {
        FileType::Subtitle
    } else if ext == "nfo" {
        FileType::Nfo
    } else if IMAGE_EXTENSIONS.contains(&ext) {
        FileType::Image
    } else {
        FileType::Other
    }
}

/// `true` for names [`analyze`] uses; the folder walk keeps only these.
#[must_use]
pub fn is_structure_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "index.bdmv" || lower == "video_ts.ifo" {
        return true;
    }
    let (_, ext) = split_ext(name);
    ext.is_some_and(|e| file_type(e) != FileType::Other)
}

/// What a special folder says about the files inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderHint {
    /// Extras of one kind (`Featurettes/`).
    Extra(ExtraType),
    /// Extras of any kind (`Extras/`, `Bonus/`): the name decides, else `other`.
    AnyExtra,
    Sample,
    Subs,
    Art(ArtKind),
}

/// The hint of a folder name that belongs to its parent's item.
fn folder_hint(name: &str) -> Option<FolderHint> {
    let lower = name.to_lowercase();
    let compact: String = lower
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-' | '.'))
        .collect();
    Some(match compact.as_str() {
        "trailers" | "trailer" | "трейлеры" | "трейлер" => {
            FolderHint::Extra(ExtraType::Trailer)
        }
        "featurettes" | "featurette" => FolderHint::Extra(ExtraType::Featurette),
        "behindthescenes" | "makingof" => FolderHint::Extra(ExtraType::BehindTheScenes),
        "deletedscenes" | "deletedscene" => FolderHint::Extra(ExtraType::DeletedScene),
        "interviews" | "interview" => FolderHint::Extra(ExtraType::Interview),
        "scenes" => FolderHint::Extra(ExtraType::Scene),
        "shorts" => FolderHint::Extra(ExtraType::Short),
        "extras"
        | "extra"
        | "bonus"
        | "bonuses"
        | "other"
        | "others"
        | "допматериалы"
        | "дополнительныематериалы"
        | "бонусы"
        | "допы" => FolderHint::AnyExtra,
        "sample" | "samples" => FolderHint::Sample,
        "subs" | "subtitles" | "subtitle" | "субтитры" => FolderHint::Subs,
        "extrafanart" => FolderHint::Art(ArtKind::Fanart),
        "extrathumbs" => FolderHint::Art(ArtKind::Thumb),
        "scans" | "scan" | "booklet" | "booklets" | "covers" => FolderHint::Art(ArtKind::Scan),
        _ => return None,
    })
}

/// A file of the listing with its path split.
#[derive(Debug)]
struct Item<'a> {
    file: &'a ListedFile,
    /// Folder components (without the file name).
    dirs: Vec<&'a str>,
    name: &'a str,
    stem: &'a str,
    ty: FileType,
}

impl<'a> Item<'a> {
    fn new(file: &'a ListedFile) -> Self {
        let trimmed = file.path.trim_start_matches('/');
        let mut parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
        let name = parts.pop().unwrap_or("");
        let (stem, ext) = split_ext(name);
        Self {
            file,
            dirs: parts,
            name,
            stem,
            ty: ext.map_or(FileType::Other, file_type),
        }
    }

    fn duration(&self) -> Option<u64> {
        self.file.duration_ms
    }
}

/// Joins folder components back into the path form of the listing.
fn join(lead: bool, comps: &[&str]) -> String {
    let body = comps.join("/");
    if lead { format!("/{body}") } else { body }
}

/// Analyzes a listing; see the module docs. Groups come sorted by root.
#[must_use]
pub fn analyze(files: &[ListedFile], options: &StructureOptions) -> Vec<Group> {
    let lead = files.first().is_some_and(|f| f.path.starts_with('/'));
    let items: Vec<Item<'_>> = files.iter().map(Item::new).collect();

    // 1. Disc structures: the folder that contains BDMV/ or VIDEO_TS/.
    let mut discs: BTreeMap<Vec<&str>, DiscUnit> = BTreeMap::new();
    for it in &items {
        for (i, d) in it.dirs.iter().enumerate() {
            let unit = if d.eq_ignore_ascii_case("bdmv") {
                DiscUnit::Bdmv
            } else if d.eq_ignore_ascii_case("video_ts") {
                DiscUnit::VideoTs
            } else {
                continue;
            };
            let lower = it.name.to_ascii_lowercase();
            let marker = match unit {
                DiscUnit::Bdmv => {
                    lower == "index.bdmv"
                        || (it.ty == FileType::Video
                            && it
                                .dirs
                                .get(i + 1)
                                .is_some_and(|s| s.eq_ignore_ascii_case("stream")))
                }
                _ => lower == "video_ts.ifo" || lower.ends_with(".vob"),
            };
            if marker {
                discs.entry(it.dirs[..i].to_vec()).or_insert(unit);
            }
            break;
        }
    }

    let mut groups: BTreeMap<Vec<&str>, Vec<&Item<'_>>> = BTreeMap::new();
    let mut hints: HashMap<*const ListedFile, (FolderHint, Vec<&str>)> = HashMap::new();
    let mut disc_files: BTreeMap<Vec<&str>, Vec<&Item<'_>>> = BTreeMap::new();
    let mut isos: Vec<&Item<'_>> = Vec::new();

    for it in &items {
        // Inside a disc structure?
        if let Some(root) = disc_root_of(&discs, &it.dirs) {
            disc_files.entry(root.clone()).or_default().push(it);
            continue;
        }
        if it.ty == FileType::Iso {
            isos.push(it);
            continue;
        }
        if it.ty == FileType::Other {
            continue;
        }
        // 2. The owner folder: the nearest ancestor that is not an extras,
        // sample, subtitle or art folder.
        let dirs = &it.dirs;
        let mut owner_len = dirs.len();
        if let Some(last_special) = dirs.iter().rposition(|d| folder_hint(d).is_some()) {
            let mut start = last_special;
            while start > 0 && folder_hint(dirs[start - 1]).is_some() {
                start -= 1;
            }
            owner_len = start;
            let hint = folder_hint(dirs[last_special]).expect("special folder");
            hints.insert(
                std::ptr::from_ref(it.file),
                (hint, dirs[last_special + 1..].to_vec()),
            );
        }
        groups
            .entry(dirs[..owner_len].to_vec())
            .or_default()
            .push(it);
    }

    let mut out: Vec<Group> = Vec::new();
    let mut video_roots: HashSet<Vec<&str>> = HashSet::new();

    // Disc groups (plus whatever else lives in the disc folder).
    for (root, unit) in &discs {
        let mut group = Group {
            root: join(lead, root),
            kind: GroupKind::Disc,
            disc: Some(*unit),
            ..Group::default()
        };
        if let Some(inner) = disc_files.get(root) {
            group.entries.extend(disc_titles(*unit, inner));
        }
        if let Some(rest) = groups.remove(root) {
            classify_group(&mut group, &rest, &hints, options, true);
        }
        video_roots.insert(root.clone());
        out.push(group);
    }
    for it in isos {
        let mut e = GroupEntry::new(it.file.path.clone(), Role::Main);
        e.size = it.file.size;
        out.push(Group {
            root: it.file.path.clone(),
            kind: GroupKind::Disc,
            disc: Some(DiscUnit::Iso),
            entries: vec![e],
            ..Group::default()
        });
    }

    // Folder groups.
    let mut folder_groups: Vec<(Vec<&str>, Group, bool)> = Vec::new();
    for (root, files) in &groups {
        let mut group = Group {
            root: join(lead, root),
            ..Group::default()
        };
        let items = classify_group(&mut group, files, &hints, options, false);
        let has_video = files
            .iter()
            .any(|f| matches!(f.ty, FileType::Video | FileType::Iso));
        group.kind = if root.is_empty() {
            GroupKind::Loose
        } else {
            match items {
                0 if group.entries.iter().any(|e| {
                    e.path.to_lowercase().ends_with("tvshow.nfo") || e.season.is_some()
                }) =>
                {
                    GroupKind::SeriesFolder
                }
                0 => GroupKind::Loose,
                1 => GroupKind::MovieFolder,
                _ if looks_like_series(&group, root) => GroupKind::SeriesFolder,
                _ => GroupKind::Loose,
            }
        };
        if has_video {
            video_roots.insert(root.clone());
        }
        folder_groups.push((root.clone(), group, has_video));
    }
    // A folder without videos is kept only above a folder with videos (a
    // show folder with its art and tvshow.nfo); music folders with a cover
    // are not groups.
    for (root, group, has_video) in folder_groups {
        let above_video = video_roots
            .iter()
            .any(|v| v.len() > root.len() && v.starts_with(&root));
        if has_video || above_video {
            out.push(group);
        }
    }

    for g in &mut out {
        g.entries.sort_by(|a, b| a.path.cmp(&b.path));
    }
    out.sort_by(|a, b| a.root.cmp(&b.root));
    out
}

fn disc_root_of<'a, 'b>(
    discs: &'b BTreeMap<Vec<&'a str>, DiscUnit>,
    dirs: &[&'a str],
) -> Option<&'b Vec<&'a str>> {
    discs.keys().find(|root| {
        dirs.len() > root.len()
            && dirs.starts_with(root)
            && ["bdmv", "video_ts", "certificate", "audio_ts"]
                .iter()
                .any(|s| dirs[root.len()].eq_ignore_ascii_case(s))
    })
}

/// The main title of a disc: the longest BDMV stream, or the VOBs of the
/// largest DVD title set (one main, or parts in order).
fn disc_titles(unit: DiscUnit, inner: &[&Item<'_>]) -> Vec<GroupEntry> {
    let entry = |it: &Item<'_>, role| {
        let mut e = GroupEntry::new(it.file.path.clone(), role);
        e.size = it.file.size;
        e
    };
    match unit {
        DiscUnit::Bdmv => inner
            .iter()
            .filter(|it| {
                it.ty == FileType::Video
                    && it
                        .dirs
                        .last()
                        .is_some_and(|d| d.eq_ignore_ascii_case("stream"))
            })
            .max_by_key(|it| (it.duration().unwrap_or(0), it.file.size))
            .map(|it| vec![entry(it, Role::Main)])
            .unwrap_or_default(),
        DiscUnit::VideoTs | DiscUnit::Iso => {
            // VTS_<set>_<n>.VOB, n ≥ 1 (n = 0 is the title set menu).
            let mut sets: BTreeMap<u32, Vec<(u32, &Item<'_>)>> = BTreeMap::new();
            for it in inner {
                let upper = it.stem.to_ascii_uppercase();
                let Some(rest) = upper.strip_prefix("VTS_") else {
                    continue;
                };
                let Some((set, n)) = rest.split_once('_') else {
                    continue;
                };
                if !it.name.to_ascii_lowercase().ends_with(".vob") {
                    continue;
                }
                if let (Ok(set), Ok(n)) = (set.parse::<u32>(), n.parse::<u32>())
                    && n > 0
                {
                    sets.entry(set).or_default().push((n, it));
                }
            }
            let Some(mut best) = sets.into_values().max_by_key(|v| {
                (
                    v.iter()
                        .map(|(_, it)| it.duration().unwrap_or(0))
                        .sum::<u64>(),
                    v.iter().map(|(_, it)| it.file.size).sum::<u64>(),
                )
            }) else {
                return Vec::new();
            };
            best.sort_by_key(|(n, _)| *n);
            if best.len() == 1 {
                return vec![entry(best[0].1, Role::Main)];
            }
            best.iter()
                .zip(1u32..)
                .map(|((_, it), no)| {
                    let mut e = entry(it, Role::Part);
                    e.part = Some(no);
                    e
                })
                .collect()
        }
    }
}

/// Folder and name skeleton of a part stack.
type StackKey<'a> = (Vec<&'a str>, String);

/// A playable item of a folder: one file or a stack of parts.
#[derive(Debug)]
struct Candidate<'a, 'b> {
    files: Vec<(&'b Item<'a>, Option<u32>)>,
}

impl Candidate<'_, '_> {
    fn longest(&self) -> Option<u64> {
        self.files.iter().filter_map(|(it, _)| it.duration()).max()
    }
}

/// Fills `group.entries` with the roles of `files`; returns the number of
/// playable items (a stack counts once).
fn classify_group(
    group: &mut Group,
    files: &[&Item<'_>],
    hints: &HashMap<*const ListedFile, (FolderHint, Vec<&str>)>,
    options: &StructureOptions,
    in_disc: bool,
) -> usize {
    let hint_of = |it: &Item<'_>| hints.get(&std::ptr::from_ref(it.file));
    let mut candidates: Vec<&Item<'_>> = Vec::new();
    let mut targets: Vec<&Item<'_>> = Vec::new();

    // Videos: extras and samples by name and folder first.
    for it in files.iter().copied().filter(|it| it.ty == FileType::Video) {
        let hint = hint_of(it).map(|(h, _)| *h);
        let (role, extra) = if hint == Some(FolderHint::Sample) || is_sample_name(it.stem) {
            (Role::Sample, None)
        } else if let Some(t) = extra_by_suffix(it.stem).or_else(|| anime_extra(it.stem)) {
            (Role::Extra, Some(t))
        } else {
            match hint {
                Some(FolderHint::Extra(t)) => (Role::Extra, Some(t)),
                Some(FolderHint::AnyExtra) => (
                    Role::Extra,
                    Some(extra_by_phrase(it.stem).unwrap_or(ExtraType::Other)),
                ),
                _ => {
                    candidates.push(it);
                    targets.push(it);
                    continue;
                }
            }
        };
        let mut e = GroupEntry::new(it.file.path.clone(), role);
        e.extra_type = extra;
        e.size = it.file.size;
        group.entries.push(e);
        if role == Role::Extra {
            targets.push(it);
        }
    }

    // Parts: same name apart from the part marker, in the same folder.
    let mut stacks: BTreeMap<StackKey<'_>, Vec<(u32, &Item<'_>)>> = BTreeMap::new();
    let mut singles: Vec<&Item<'_>> = Vec::new();
    for it in candidates {
        match part_marker(it.stem) {
            Some((n, skeleton)) => stacks
                .entry((it.dirs.clone(), skeleton))
                .or_default()
                .push((n, it)),
            None => singles.push(it),
        }
    }
    let ratio = options.short_ratio.filter(|r| *r > 0.0);
    let short = |d: Option<u64>, max: Option<u64>| match (ratio, d, max) {
        (Some(r), Some(d), Some(max)) => (d as f64) < r * max as f64,
        _ => false,
    };
    let mut items: Vec<Candidate<'_, '_>> = Vec::new();
    for (_, mut members) in stacks {
        members.sort_by_key(|(n, it)| (*n, it.name));
        if members.len() < 2 {
            singles.extend(members.into_iter().map(|(_, it)| it));
            continue;
        }
        let max = members.iter().filter_map(|(_, it)| it.duration()).max();
        let (kept, leftovers): (Vec<_>, Vec<_>) = members
            .into_iter()
            .partition(|(_, it)| !short(it.duration(), max));
        for (_, it) in &leftovers {
            let mut e = GroupEntry::new(it.file.path.clone(), Role::Ignore);
            e.size = it.file.size;
            group.entries.push(e);
        }
        let numbers: Vec<u32> = kept.iter().map(|(n, _)| *n).collect();
        let contiguous = numbers.iter().zip(1u32..).all(|(n, want)| *n == want);
        if kept.len() >= 2 && contiguous {
            items.push(Candidate {
                files: kept.into_iter().map(|(n, it)| (it, Some(n))).collect(),
            });
        } else {
            singles.extend(kept.into_iter().map(|(_, it)| it));
        }
    }
    items.extend(singles.into_iter().map(|it| Candidate {
        files: vec![(it, None)],
    }));

    // Optional duration rule: in a folder with exactly one long item the
    // short ones are extras.
    let max = items.iter().filter_map(Candidate::longest).max();
    let is_long = |c: &Candidate<'_, '_>| c.longest().is_none_or(|d| !short(Some(d), max));
    let long_items = items.iter().filter(|c| is_long(c)).count() + usize::from(in_disc);
    let mut playable = 0;
    for c in &items {
        let extra = long_items == 1 && !is_long(c);
        if !extra {
            playable += 1;
        }
        for (it, part) in &c.files {
            let role = match (extra, part) {
                (true, _) => Role::Extra,
                (false, Some(_)) => Role::Part,
                (false, None) => Role::Main,
            };
            let mut e = GroupEntry::new(it.file.path.clone(), role);
            e.part = if role == Role::Part { *part } else { None };
            e.extra_type = extra.then_some(ExtraType::Other);
            e.size = it.file.size;
            group.entries.push(e);
        }
    }
    let mains: Vec<&Item<'_>> = items
        .iter()
        .filter(|c| !(long_items == 1 && !is_long(c)))
        .filter_map(|c| c.files.first().map(|(it, _)| *it))
        .collect();
    let single_main = (mains.len() == 1).then(|| mains[0]);
    let series = !in_disc && mains.len() > 1;

    // Sidecars.
    for it in files.iter().copied() {
        let hint = hint_of(it);
        let mut e = match it.ty {
            FileType::Subtitle => {
                let (target, rest) = match hint {
                    Some((FolderHint::Subs, sub_dirs)) => {
                        let by_dir = sub_dirs
                            .last()
                            .and_then(|d| targets.iter().find(|v| v.stem.eq_ignore_ascii_case(d)));
                        (by_dir.copied().or(single_main), it.stem)
                    }
                    _ => match video_prefix(&targets, it) {
                        Some((v, rest)) => (Some(v), rest),
                        None => (single_main, it.stem),
                    },
                };
                let mut e = GroupEntry::new(it.file.path.clone(), Role::Subtitle);
                e.target = target.map(|v| v.file.path.clone());
                e.subtitle = Some(subtitle_info(rest));
                e
            }
            FileType::Nfo => {
                let mut e = GroupEntry::new(it.file.path.clone(), Role::Nfo);
                e.target = targets
                    .iter()
                    .find(|v| v.dirs == it.dirs && v.stem.eq_ignore_ascii_case(it.stem))
                    .map(|v| v.file.path.clone());
                e
            }
            FileType::Image => {
                let folder_art = match hint {
                    Some((FolderHint::Art(kind), _)) => Some(*kind),
                    _ => None,
                };
                let Some((kind, season, target)) = folder_art
                    .map(|k| (k, None, None))
                    .or_else(|| art_of(it, &targets, series))
                else {
                    continue;
                };
                let mut e = GroupEntry::new(it.file.path.clone(), Role::Art);
                e.art = Some(kind);
                e.season = season;
                e.target = target;
                e
            }
            _ => continue,
        };
        e.size = it.file.size;
        group.entries.push(e);
    }
    playable
}

/// Several items whose names share a skeleton, or a season-like folder.
fn looks_like_series(group: &Group, root: &[&str]) -> bool {
    if root.last().is_some_and(|d| is_season_folder(d)) {
        return true;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for e in group.playable() {
        let name = e.path.rsplit('/').next().unwrap_or("");
        let (stem, _) = split_ext(name);
        let skeleton: String = stem
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(char::to_lowercase)
            .collect();
        *seen.entry(skeleton).or_default() += 1;
    }
    seen.values().any(|n| *n >= 2)
}

fn is_season_folder(name: &str) -> bool {
    let lower = name.to_lowercase();
    let rest = lower
        .strip_prefix("season")
        .or_else(|| lower.strip_prefix("сезон"))
        .or_else(|| lower.strip_prefix('s'));
    if lower == "specials" {
        return true;
    }
    rest.map(|r| r.trim_start_matches([' ', '_', '.', '-']))
        .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
}

/// `sample.mkv`, `Sample-dune.mkv`, `dune.2021.sample.mkv`, `dune-sample`.
fn is_sample_name(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    has_word_at_edge(&lower, "sample")
}

/// `word` at the start (followed by a separator or the end) or at the end
/// (after a separator, optionally closed by `]` or `)`).
fn has_word_at_edge(lower: &str, word: &str) -> bool {
    if let Some(rest) = lower.strip_prefix(word)
        && rest.chars().next().is_none_or(is_sep)
    {
        return true;
    }
    let trimmed = lower.trim_end_matches([']', ')']);
    trimmed.strip_suffix(word).is_some_and(|head| {
        head.chars()
            .next_back()
            .is_some_and(|c| is_sep(c) || c == '[' || c == '(')
    })
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | '_' | '-')
}

/// Plex-style suffixes (`Movie-trailer`, `Movie (2010)-featurette`) and
/// trailers/teasers named at the start or end.
fn extra_by_suffix(stem: &str) -> Option<ExtraType> {
    let lower = stem.to_lowercase();
    for (word, ty) in [
        ("trailer", ExtraType::Trailer),
        ("teaser", ExtraType::Teaser),
    ] {
        if has_word_at_edge(&lower, word) {
            return Some(ty);
        }
    }
    const DASHED: [(&str, ExtraType); 11] = [
        ("featurette", ExtraType::Featurette),
        ("behindthescenes", ExtraType::BehindTheScenes),
        ("deletedscene", ExtraType::DeletedScene),
        ("deleted", ExtraType::DeletedScene),
        ("interview", ExtraType::Interview),
        ("scene", ExtraType::Scene),
        ("short", ExtraType::Short),
        ("clip", ExtraType::Other),
        ("other", ExtraType::Other),
        ("extra", ExtraType::Other),
        ("featurettes", ExtraType::Featurette),
    ];
    DASHED.iter().find_map(|(word, ty)| {
        lower
            .strip_suffix(word)
            .is_some_and(|head| head.ends_with('-'))
            .then_some(*ty)
    })
}

/// Extra kind from words in the name, for files in a generic extras folder.
fn extra_by_phrase(stem: &str) -> Option<ExtraType> {
    let lower = stem.to_lowercase();
    let compact: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
    [
        ("behindthescenes", ExtraType::BehindTheScenes),
        ("makingof", ExtraType::BehindTheScenes),
        ("deletedscene", ExtraType::DeletedScene),
        ("interview", ExtraType::Interview),
        ("featurette", ExtraType::Featurette),
        ("trailer", ExtraType::Trailer),
        ("teaser", ExtraType::Teaser),
    ]
    .iter()
    .find_map(|(w, t)| compact.contains(w).then_some(*t))
}

/// Anime bonus tokens: `NCOP`, `NCED` (any position), and `Menu01`,
/// `Preview02`, `PV1`, `CM03`, `OP2`, `ED1`, `Teaser01` (with a number, or
/// right after ` - `).
fn anime_extra(stem: &str) -> Option<ExtraType> {
    let mut prev_dash = false;
    let mut found = None;
    for raw in stem.split(|c: char| c.is_whitespace() || "._[]()".contains(c)) {
        if raw.is_empty() {
            continue;
        }
        if raw == "-" {
            prev_dash = true;
            continue;
        }
        let token = raw.to_ascii_lowercase();
        let (letters, has_number) = split_number(&token);
        let strong = match letters {
            "ncop" | "creditlessop" => Some(ExtraType::Opening),
            "nced" | "creditlessed" => Some(ExtraType::Ending),
            _ => None,
        };
        let weak = match letters {
            "menu" => Some(ExtraType::Menu),
            "preview" | "yokoku" => Some(ExtraType::Preview),
            "pv" | "trailer" => Some(ExtraType::Trailer),
            "teaser" => Some(ExtraType::Teaser),
            "cm" | "spot" => Some(ExtraType::Other),
            // `OP`/`ED` only in capitals: "Ed" is a name.
            "op" if raw.starts_with("OP") => Some(ExtraType::Opening),
            "ed" if raw.starts_with("ED") => Some(ExtraType::Ending),
            _ => None,
        };
        if let Some(t) = strong {
            return Some(t);
        }
        if let Some(t) = weak
            && (has_number || prev_dash)
        {
            found = found.or(Some(t));
        }
        prev_dash = false;
    }
    found
}

/// `ncop1` → (`ncop`, true); `ncop1v2` → (`ncop`, true); `menu` → (`menu`, false).
fn split_number(token: &str) -> (&str, bool) {
    let no_digits = token.trim_end_matches(|c: char| c.is_ascii_digit());
    let mut letters = no_digits;
    let has_number = no_digits.len() < token.len();
    // A release version after the number: `01v2`.
    if has_number
        && let Some(head) = no_digits.strip_suffix('v')
        && head.ends_with(|c: char| c.is_ascii_digit())
    {
        letters = head.trim_end_matches(|c: char| c.is_ascii_digit());
    }
    (letters, has_number)
}

/// The part marker of a name: `cd2`, `CD 2`, `part1`, `Part.2`, `pt1`,
/// `disc 1`, `disk2` after a separator, followed by a separator or the end.
/// Returns the number and the name without the marker (lowercase).
fn part_marker(stem: &str) -> Option<(u32, String)> {
    let lower = stem.to_lowercase();
    let bytes = lower.as_bytes();
    let mut best = None;
    for word in ["cd", "part", "pt", "disc", "disk"] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(word) {
            let start = from + pos;
            from = start + word.len();
            if start > 0 && !is_sep_or_bracket(bytes[start - 1] as char) {
                continue;
            }
            let mut i = start + word.len();
            if i < bytes.len() && matches!(bytes[i], b' ' | b'.' | b'_' | b'-') {
                i += 1;
            }
            let digits_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i == digits_start || i - digits_start > 2 {
                continue;
            }
            if i < bytes.len() && !is_sep_or_bracket(bytes[i] as char) {
                continue;
            }
            let Ok(n) = lower[digits_start..i].parse::<u32>() else {
                continue;
            };
            if n == 0 {
                continue;
            }
            // The last marker in the name wins.
            if best.as_ref().is_none_or(|(s, _, _)| start > *s) {
                best = Some((start, i, n));
            }
        }
    }
    let (start, end, n) = best?;
    let skeleton = format!(
        "{}\u{0}{}",
        lower[..start].trim_end_matches(is_sep),
        &lower[end..]
    );
    Some((n, skeleton))
}

fn is_sep_or_bracket(c: char) -> bool {
    is_sep(c) || matches!(c, '[' | ']' | '(' | ')')
}

/// The video of the same folder whose name starts the sidecar's name, and
/// the rest of the sidecar's name.
fn video_prefix<'a, 'b>(
    videos: &[&'b Item<'a>],
    sidecar: &'b Item<'a>,
) -> Option<(&'b Item<'a>, &'b str)> {
    let lower = sidecar.stem.to_lowercase();
    videos
        .iter()
        .filter(|v| v.dirs == sidecar.dirs)
        .filter_map(|v| {
            let vs = v.stem.to_lowercase();
            let rest = lower.strip_prefix(&vs)?;
            (rest.is_empty() || rest.starts_with(is_sep)).then_some((*v, vs.len()))
        })
        .max_by_key(|(_, len)| *len)
        .map(|(v, len)| (v, sidecar.stem.get(len..).unwrap_or("")))
}

/// Language and flags from the tail of a subtitle name: `ru.forced`,
/// `English.SDH`, `2_English`, `rus.default`.
#[must_use]
pub fn subtitle_info(rest: &str) -> SubtitleInfo {
    let mut info = SubtitleInfo::default();
    let tokens: Vec<&str> = rest
        .split(|c: char| is_sep(c) || matches!(c, '[' | ']' | '(' | ')'))
        .filter(|t| !t.is_empty())
        .collect();
    // Only the tail of the name is looked at: stop at the first token that
    // is neither a flag, a number nor a language.
    for token in tokens.iter().rev() {
        let lower = token.to_lowercase();
        match lower.as_str() {
            "forced" | "foreign" | "форсированные" => info.forced = true,
            "sdh" | "hi" | "cc" | "hearing" => info.hearing_impaired = true,
            "default" => info.default = true,
            _ if lower.chars().all(|c| c.is_ascii_digit()) => {}
            _ => match language(&lower) {
                Some(code) => {
                    info.lang.get_or_insert_with(|| code.to_owned());
                }
                None => break,
            },
        }
    }
    info
}

/// ISO 639-2/B code of a language tag or name.
fn language(token: &str) -> Option<&'static str> {
    const LANGS: &[(&str, &[&str])] = &[
        (
            "rus",
            &["ru", "rus", "russian", "русский", "русские", "рус"],
        ),
        ("eng", &["en", "eng", "english", "английский", "англ"]),
        ("ukr", &["uk", "ua", "ukr", "ukrainian", "украинский"]),
        ("jpn", &["ja", "jp", "jpn", "japanese"]),
        ("ger", &["de", "ger", "deu", "german", "deutsch"]),
        (
            "fre",
            &["fr", "fre", "fra", "french", "francais", "français"],
        ),
        ("spa", &["es", "spa", "spanish", "espanol", "español"]),
        ("ita", &["it", "ita", "italian"]),
        ("por", &["pt", "por", "portuguese", "pt-br", "ptbr"]),
        ("chi", &["zh", "chi", "zho", "chinese", "chs", "cht"]),
        ("kor", &["ko", "kor", "korean"]),
        ("pol", &["pl", "pol", "polish"]),
        ("cze", &["cs", "cze", "ces", "czech"]),
        ("dut", &["nl", "dut", "nld", "dutch"]),
        ("swe", &["sv", "swe", "swedish"]),
        ("nor", &["no", "nor", "nob", "norwegian"]),
        ("dan", &["da", "dan", "danish"]),
        ("fin", &["fi", "fin", "finnish"]),
        ("tur", &["tr", "tur", "turkish"]),
        ("ara", &["ar", "ara", "arabic"]),
        ("heb", &["he", "heb", "hebrew"]),
        ("gre", &["el", "gre", "ell", "greek"]),
        ("hun", &["hu", "hun", "hungarian"]),
        ("rum", &["ro", "rum", "ron", "romanian"]),
        ("bul", &["bg", "bul", "bulgarian"]),
        ("srp", &["sr", "srp", "serbian"]),
        ("hrv", &["hr", "hrv", "croatian"]),
        ("bel", &["be", "bel", "belarusian"]),
        ("kaz", &["kk", "kaz", "kazakh"]),
        ("vie", &["vi", "vie", "vietnamese"]),
        ("tha", &["th", "tha", "thai"]),
        ("ind", &["id", "ind", "indonesian"]),
        ("hin", &["hin", "hindi"]),
        ("per", &["fa", "per", "fas", "persian"]),
    ];
    LANGS
        .iter()
        .find(|(_, names)| names.contains(&token))
        .map(|(code, _)| *code)
}

/// Kind, season and target video of a local art file.
fn art_of(
    it: &Item<'_>,
    videos: &[&Item<'_>],
    series: bool,
) -> Option<(ArtKind, Option<u32>, Option<String>)> {
    let lower = it.stem.to_lowercase();
    if let Some(kind) = art_name(&lower) {
        return Some((kind, None, None));
    }
    // season01-poster, season-specials-banner, season-all-poster
    if let Some(rest) = lower.strip_prefix("season")
        && let Some((season, kind)) = rest.rsplit_once('-')
        && let Some(kind) = art_name(kind)
    {
        let season = season.trim_start_matches('-');
        let number = if season == "specials" {
            Some(0)
        } else {
            season.parse::<u32>().ok()
        };
        return Some((kind, number, None));
    }
    // <video>-poster, <video>-fanart, <video>-thumb; <video>.jpg itself.
    for v in videos.iter().filter(|v| v.dirs == it.dirs) {
        let vs = v.stem.to_lowercase();
        let Some(rest) = lower.strip_prefix(&vs) else {
            continue;
        };
        if rest.is_empty() {
            let kind = if series {
                ArtKind::Thumb
            } else {
                ArtKind::Poster
            };
            return Some((kind, None, Some(v.file.path.clone())));
        }
        if let Some(kind) = rest.strip_prefix(['-', '.', '_']).and_then(art_name) {
            return Some((kind, None, Some(v.file.path.clone())));
        }
    }
    None
}

/// Art kind of a bare name (`poster`, `fanart2`, `clearlogo`).
fn art_name(name: &str) -> Option<ArtKind> {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    Some(match base {
        "poster" | "folder" | "cover" | "default" | "movie" | "show" | "keyart" => ArtKind::Poster,
        "fanart" | "backdrop" | "background" | "art" => ArtKind::Fanart,
        "clearlogo" | "logo" => ArtKind::Clearlogo,
        "clearart" => ArtKind::Clearart,
        "banner" => ArtKind::Banner,
        "thumb" | "landscape" => ArtKind::Thumb,
        "disc" | "discart" | "cdart" => ArtKind::Discart,
        _ => return None,
    })
}

/// External ids in an NFO file: Kodi/Jellyfin XML (`<uniqueid type="tmdb">`,
/// `<tmdbid>`, `<imdbid>`, `<tvdbid>`, `<id>tt…</id>`) and links in scene
/// NFOs (`imdb.com/title/tt…`, `themoviedb.org/movie/…`, `anidb.net/a…`).
///
/// TMDB ids get the kind when it is known (`movie:603`, `tv:1399`): from
/// the link, or from the root element (`<movie>`, `<tvshow>`).
#[must_use]
pub fn parse_nfo_ids(text: &str) -> BTreeMap<String, String> {
    let mut ids = BTreeMap::new();
    let lower = text.to_lowercase();
    let tmdb_kind = if lower.contains("<tvshow") {
        Some("tv")
    } else if lower.contains("<movie") {
        Some("movie")
    } else {
        None
    };
    let with_kind = |v: &str| match tmdb_kind {
        Some(k) => format!("{k}:{v}"),
        None => v.to_owned(),
    };

    // <uniqueid type="x" ...>value</uniqueid>
    let mut from = 0;
    while let Some(pos) = lower[from..].find("<uniqueid") {
        let start = from + pos;
        from = start + 9;
        let Some(close) = lower[start..].find('>').map(|i| start + i) else {
            break;
        };
        let attrs = &lower[start..close];
        let Some(ty) = attr(attrs, "type") else {
            continue;
        };
        let Some(end) = lower[close..].find("</uniqueid>").map(|i| close + i) else {
            continue;
        };
        let value = text[close + 1..end].trim();
        if !value.is_empty() {
            let v = if ty == "tmdb" {
                with_kind(value)
            } else {
                value.to_owned()
            };
            ids.entry(ty.to_owned()).or_insert(v);
        }
    }
    for (tag, key) in [
        ("tmdbid", "tmdb"),
        ("imdbid", "imdb"),
        ("imdb_id", "imdb"),
        ("tvdbid", "tvdb"),
        ("anidbid", "anidb"),
    ] {
        if let Some(v) = element(text, &lower, tag) {
            let v = if key == "tmdb" { with_kind(&v) } else { v };
            ids.entry(key.to_owned()).or_insert(v);
        }
    }
    if let Some(v) = element(text, &lower, "id").filter(|v| is_imdb_id(v)) {
        ids.entry("imdb".to_owned()).or_insert(v);
    }
    // Links.
    if let Some(v) =
        after(&lower, "imdb.com/title/").map(|s| take_while(s, |c| c.is_ascii_alphanumeric()))
        && is_imdb_id(&v)
    {
        ids.entry("imdb".to_owned()).or_insert(v);
    }
    for kind in ["movie", "tv"] {
        if let Some(v) = after(&lower, &format!("themoviedb.org/{kind}/"))
            .map(|s| take_while(s, |c| c.is_ascii_digit()))
            .filter(|v| !v.is_empty())
        {
            ids.entry("tmdb".to_owned())
                .or_insert(format!("{kind}:{v}"));
        }
    }
    for prefix in [
        "anidb.net/anime/",
        "anidb.net/a",
        "anidb.net/perl-bin/animedb.pl?show=anime&aid=",
    ] {
        if let Some(v) = after(&lower, prefix)
            .map(|s| take_while(s, |c| c.is_ascii_digit()))
            .filter(|v| !v.is_empty())
        {
            ids.entry("anidb".to_owned()).or_insert(v);
        }
    }
    ids
}

fn is_imdb_id(v: &str) -> bool {
    v.len() >= 9 && v.starts_with("tt") && v[2..].chars().all(|c| c.is_ascii_digit())
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pos = attrs.find(&format!("{name}="))?;
    let rest = &attrs[pos + name.len() + 1..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    rest.find(quote).map(|end| &rest[..end])
}

/// Trimmed text of the first `<tag>…</tag>` (tag without attributes).
fn element(text: &str, lower: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)? + open.len();
    let end = start + lower[start..].find(&close)?;
    let v = text[start..end].trim();
    (!v.is_empty()).then(|| v.to_owned())
}

fn after<'a>(s: &'a str, needle: &str) -> Option<&'a str> {
    s.find(needle).map(|i| &s[i + needle.len()..])
}

fn take_while(s: &str, f: impl Fn(char) -> bool) -> String {
    s.chars().take_while(|c| f(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[(&str, Option<u64>)]) -> Vec<ListedFile> {
        paths
            .iter()
            .map(|(p, d)| ListedFile {
                path: (*p).to_owned(),
                size: 1000,
                duration_ms: *d,
            })
            .collect()
    }

    fn role_of<'a>(groups: &'a [Group], path: &str) -> Option<&'a GroupEntry> {
        groups.iter().find_map(|g| g.entry(path))
    }

    #[test]
    fn part_markers() {
        assert_eq!(part_marker("The.Matrix.1999.cd1").map(|p| p.0), Some(1));
        assert_eq!(part_marker("Movie (1957) Disc 2").map(|p| p.0), Some(2));
        assert_eq!(part_marker("Dune (1984) - part3").map(|p| p.0), Some(3));
        assert_eq!(
            part_marker("Ironiya.1975.Part2.BDRip").map(|p| p.0),
            Some(2)
        );
        assert_eq!(part_marker("Movie.pt1").map(|p| p.0), Some(1));
        assert_eq!(part_marker("Back.to.the.Future.Part.II.1989"), None);
        assert_eq!(part_marker("Movie.DVD5.DivX"), None);
        assert_eq!(part_marker("Discovery.S01E01"), None);
        assert_eq!(part_marker("Partisan (2015)"), None);
        assert_eq!(part_marker("Apartment 143"), None);
        assert_eq!(
            part_marker("A.cd1").map(|p| p.1),
            part_marker("A.CD2").map(|p| p.1)
        );
    }

    #[test]
    fn subtitle_names() {
        let i = subtitle_info(".ru.forced");
        assert_eq!(i.lang.as_deref(), Some("rus"));
        assert!(i.forced && !i.hearing_impaired);
        let i = subtitle_info("2_English");
        assert_eq!(i.lang.as_deref(), Some("eng"));
        let i = subtitle_info(".en.sdh.default");
        assert_eq!(i.lang.as_deref(), Some("eng"));
        assert!(i.hearing_impaired && i.default);
        assert_eq!(subtitle_info("").lang, None);
    }

    #[test]
    fn anime_tokens() {
        assert_eq!(
            anime_extra("[Group] Frieren - NCOP1 [BD 1080p]"),
            Some(ExtraType::Opening)
        );
        assert_eq!(anime_extra("[G] Show - NCED [BD]"), Some(ExtraType::Ending));
        assert_eq!(anime_extra("[G] Show - Menu01 [BD]"), Some(ExtraType::Menu));
        assert_eq!(
            anime_extra("[G] Show - PV02 [BD]"),
            Some(ExtraType::Trailer)
        );
        assert_eq!(anime_extra("[G] Show - CM03 [BD]"), Some(ExtraType::Other));
        assert_eq!(anime_extra("The Menu (2022)"), None);
        assert_eq!(anime_extra("Ed Wood (1994)"), None);
        assert_eq!(anime_extra("[G] Show - 01 [BD]"), None);
    }

    #[test]
    fn extras_by_name() {
        assert_eq!(
            extra_by_suffix("Inception (2010)-trailer"),
            Some(ExtraType::Trailer)
        );
        assert_eq!(
            extra_by_suffix("Dune-featurette"),
            Some(ExtraType::Featurette)
        );
        assert_eq!(
            extra_by_suffix("Dune (2021) [Trailer]"),
            Some(ExtraType::Trailer)
        );
        assert_eq!(extra_by_suffix("Short Circuit (1986)"), None);
        assert_eq!(extra_by_suffix("The Trailer Park Boys S01E01"), None);
        assert!(is_sample_name("The.Matrix.1999.sample"));
        assert!(is_sample_name("sample"));
        assert!(!is_sample_name("Samples of Life (2019)"));
    }

    #[test]
    fn disc_folder_is_one_item() {
        let g = analyze(
            &files(&[
                ("/M/Inception (2010)/BDMV/index.bdmv", None),
                ("/M/Inception (2010)/BDMV/STREAM/00000.m2ts", Some(30_000)),
                (
                    "/M/Inception (2010)/BDMV/STREAM/00001.m2ts",
                    Some(8_880_000),
                ),
                ("/M/Inception (2010)/poster.jpg", None),
            ]),
            &StructureOptions::default(),
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].root, "/M/Inception (2010)");
        assert_eq!(g[0].disc, Some(DiscUnit::Bdmv));
        let main: Vec<_> = g[0].playable().map(|e| e.path.as_str()).collect();
        assert_eq!(main, ["/M/Inception (2010)/BDMV/STREAM/00001.m2ts"]);
        assert_eq!(
            role_of(&g, "/M/Inception (2010)/poster.jpg").unwrap().art,
            Some(ArtKind::Poster)
        );
    }

    #[test]
    fn dvd_title_set_becomes_parts() {
        let g = analyze(
            &files(&[
                ("DVD/X/VIDEO_TS/VIDEO_TS.IFO", None),
                ("DVD/X/VIDEO_TS/VIDEO_TS.VOB", None),
                ("DVD/X/VIDEO_TS/VTS_01_0.VOB", None),
                ("DVD/X/VIDEO_TS/VTS_01_1.VOB", None),
                ("DVD/X/VIDEO_TS/VTS_01_2.VOB", None),
            ]),
            &StructureOptions::default(),
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].root, "DVD/X");
        let parts: Vec<_> = g[0].playable().map(|e| (e.role, e.part)).collect();
        assert_eq!(parts, [(Role::Part, Some(1)), (Role::Part, Some(2))]);
    }

    #[test]
    fn duration_rule_is_optional() {
        let listing = files(&[
            ("/M/Dune (1984)/Dune (1984).mkv", Some(8_000_000)),
            ("/M/Dune (1984)/Making of.mkv", Some(600_000)),
        ]);
        let g = analyze(&listing, &StructureOptions::default());
        assert_eq!(
            role_of(&g, "/M/Dune (1984)/Making of.mkv").unwrap().role,
            Role::Extra
        );
        let off = StructureOptions { short_ratio: None };
        let g = analyze(&listing, &off);
        assert_eq!(
            role_of(&g, "/M/Dune (1984)/Making of.mkv").unwrap().role,
            Role::Main
        );
        // Without durations nothing is judged by duration.
        let unknown = files(&[
            ("/M/Dune (1984)/Dune (1984).mkv", None),
            ("/M/Dune (1984)/Making of.mkv", None),
        ]);
        let g = analyze(&unknown, &StructureOptions::default());
        assert!(g[0].entries.iter().all(|e| e.role == Role::Main));
    }

    #[test]
    fn episodes_are_not_extras_by_duration() {
        let g = analyze(
            &files(&[
                ("/TV/Show/Season 1/Show.S01E01.mkv", Some(1_400_000)),
                ("/TV/Show/Season 1/Show.S01E02.mkv", Some(1_420_000)),
                ("/TV/Show/Season 1/Show.S01E03.mkv", Some(200_000)),
                ("/TV/Show/tvshow.nfo", None),
                ("/TV/Show/fanart.jpg", None),
                ("/TV/Show/season01-poster.jpg", None),
            ]),
            &StructureOptions::default(),
        );
        let season = g.iter().find(|g| g.root == "/TV/Show/Season 1").unwrap();
        assert_eq!(season.kind, GroupKind::SeriesFolder);
        assert!(season.entries.iter().all(|e| e.role == Role::Main));
        let show = g.iter().find(|g| g.root == "/TV/Show").unwrap();
        assert_eq!(show.kind, GroupKind::SeriesFolder);
        let art = show.entry("/TV/Show/season01-poster.jpg").unwrap();
        assert_eq!((art.art, art.season), (Some(ArtKind::Poster), Some(1)));
    }

    #[test]
    fn sidecar_subtitles_and_nfo() {
        let g = analyze(
            &files(&[
                ("/M/Movie (2010)/Movie (2010).mkv", None),
                ("/M/Movie (2010)/Movie (2010).ru.forced.srt", None),
                ("/M/Movie (2010)/Movie (2010).nfo", None),
                ("/M/Movie (2010)/Subs/2_English.srt", None),
                ("/M/Movie (2010)/Movie (2010)-fanart.jpg", None),
                ("/M/Movie (2010)/clearlogo.png", None),
                ("/M/Movie (2010)/readme.txt", None),
            ]),
            &StructureOptions::default(),
        );
        assert_eq!(g.len(), 1);
        let video = Some("/M/Movie (2010)/Movie (2010).mkv".to_owned());
        let ru = g[0]
            .entry("/M/Movie (2010)/Movie (2010).ru.forced.srt")
            .unwrap();
        assert_eq!(ru.role, Role::Subtitle);
        assert_eq!(ru.target, video);
        let info = ru.subtitle.as_ref().unwrap();
        assert_eq!((info.lang.as_deref(), info.forced), (Some("rus"), true));
        let en = g[0].entry("/M/Movie (2010)/Subs/2_English.srt").unwrap();
        assert_eq!(en.target, video);
        assert_eq!(en.subtitle.as_ref().unwrap().lang.as_deref(), Some("eng"));
        let nfo = g[0].entry("/M/Movie (2010)/Movie (2010).nfo").unwrap();
        assert_eq!((nfo.role, nfo.target.clone()), (Role::Nfo, video.clone()));
        let fanart = g[0]
            .entry("/M/Movie (2010)/Movie (2010)-fanart.jpg")
            .unwrap();
        assert_eq!(fanart.art, Some(ArtKind::Fanart));
        assert_eq!(
            g[0].entry("/M/Movie (2010)/clearlogo.png").unwrap().art,
            Some(ArtKind::Clearlogo)
        );
        assert!(g[0].entry("/M/Movie (2010)/readme.txt").is_none());
    }

    #[test]
    fn music_folders_are_not_groups() {
        let g = analyze(
            &files(&[
                ("/Music/Artist/Album/cover.jpg", None),
                ("/Music/Artist/Album/01.flac", None),
            ]),
            &StructureOptions::default(),
        );
        assert!(g.is_empty());
    }

    #[test]
    fn nfo_ids() {
        let kodi = r#"<?xml version="1.0"?><movie><title>The Matrix</title>
            <uniqueid type="imdb" default="false">tt0133093</uniqueid>
            <uniqueid type="tmdb" default="true">603</uniqueid></movie>"#;
        let ids = parse_nfo_ids(kodi);
        assert_eq!(ids.get("imdb").map(String::as_str), Some("tt0133093"));
        assert_eq!(ids.get("tmdb").map(String::as_str), Some("movie:603"));

        let scene = "Release info\nhttps://www.imdb.com/title/tt1375666/\nhttps://www.themoviedb.org/movie/27205-inception";
        let ids = parse_nfo_ids(scene);
        assert_eq!(ids.get("imdb").map(String::as_str), Some("tt1375666"));
        assert_eq!(ids.get("tmdb").map(String::as_str), Some("movie:27205"));

        let show =
            "<tvshow><tmdbid>1399</tmdbid><tvdbid>121361</tvdbid><id>tt0944947</id></tvshow>";
        let ids = parse_nfo_ids(show);
        assert_eq!(ids.get("tmdb").map(String::as_str), Some("tv:1399"));
        assert_eq!(ids.get("tvdb").map(String::as_str), Some("121361"));
        assert_eq!(ids.get("imdb").map(String::as_str), Some("tt0944947"));
        assert!(parse_nfo_ids("nothing here").is_empty());
    }
}
