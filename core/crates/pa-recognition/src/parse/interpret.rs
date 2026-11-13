// SPDX-License-Identifier: MPL-2.0
//! From classified tokens to N-best [`ParsedName`] hypotheses.
//!
//! Classification leaves only a few genuinely ambiguous tokens: a bare
//! number (`- 05`: episode, absolute or part of the title) and year-like
//! numbers (`1917`: title or year). Each becomes an *ambiguity point* with
//! weighted options; hypotheses are the combinations of options, weighted by
//! the product of option weights.

use super::classify::{Classified, NumTok, Tag};
use super::lexicon;
use super::token::Bracket;
use super::types::{EpisodeKind, EpisodeNumber, ExtraKind, MediaHint, ParsedName};

/// Role chosen for an ambiguous number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Title,
    Episode,
    Absolute,
    Year,
}

impl Role {
    fn reason(self) -> &'static str {
        match self {
            Role::Title => "title",
            Role::Episode => "episode",
            Role::Absolute => "absolute",
            Role::Year => "year",
        }
    }
}

/// One ambiguous token and its weighted roles.
#[derive(Debug, Clone)]
pub(crate) struct Point {
    pub token: usize,
    pub options: Vec<(Role, f32)>,
}

/// Context that changes weights (from folders).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Context {
    /// A season is known from a folder.
    pub season_known: bool,
}

/// Result of the analysis of one name.
#[derive(Debug, Clone)]
pub(crate) struct Analysis {
    pub toks: Vec<Classified>,
    pub brackets: Vec<Bracket>,
    pub points: Vec<Point>,
    /// Bracket index of the leading release-group bracket.
    pub group_bracket: Option<usize>,
    pub group: Option<String>,
    pub crc_or_fansub: bool,
}

fn num(t: &Classified) -> Option<&NumTok> {
    match &t.tag {
        Tag::Num(n) => Some(n),
        _ => None,
    }
}

fn plain(t: &Classified) -> bool {
    t.tok.bracket.is_none()
}

/// Finds the release group and the ambiguity points.
pub(crate) fn analyze(
    name: &str,
    mut toks: Vec<Classified>,
    brackets: Vec<Bracket>,
    ctx: Context,
) -> Analysis {
    let inner_text = |b: &Bracket| name[b.inner.clone()].trim().to_string();
    // Tags that never occur inside a release-group bracket.
    let hard = |t: &Classified| {
        matches!(
            t.tag,
            Tag::Num(_)
                | Tag::Year(_)
                | Tag::Resolution
                | Tag::VideoCodec
                | Tag::AudioCodec
                | Tag::Crc(_)
                | Tag::SeasonEp { .. }
                | Tag::Season(_)
                | Tag::Kind(..)
                | Tag::Date(_)
                | Tag::Part(_)
                | Tag::Edition
                | Tag::Version(_)
        )
    };
    // --- Release group -------------------------------------------------
    let mut group = None;
    let mut group_bracket = None;
    if let Some(b0) = brackets.first()
        && b0.inner.start == b0.open.len_utf8()
        && !b0.tokens.is_empty()
        && toks.first().is_some_and(|t| t.tok.bracket == Some(0))
        && !toks[b0.tokens.clone()].iter().any(hard)
        && toks[b0.tokens.clone()]
            .iter()
            .any(|t| matches!(t.tag, Tag::Free | Tag::Group | Tag::Dub))
        && b0.tokens.len() <= 6
    {
        // Leading `[Group]`: everything in it.
        group = Some(inner_text(b0));
        group_bracket = Some(0);
        for t in &mut toks[b0.tokens.clone()] {
            if t.tag != Tag::Consumed {
                t.tag = Tag::Group;
            }
        }
    }
    let first_marker = toks.iter().position(|t| {
        plain(t)
            && (t.tag.is_release_info()
                || t.tag.is_episode_marker()
                || matches!(t.tag, Tag::Season(_)))
    });
    if group.is_none() {
        // `x264-GROUP` glued after a release token, last plain token.
        let last_plain = toks.iter().rposition(plain);
        if let Some(k) = last_plain
            && toks[k].tok.glued_hyphen
            && matches!(toks[k].tag, Tag::Free | Tag::Group)
        {
            group = Some(toks[k].display.clone());
            toks[k].tag = Tag::Group;
        }
    }
    if group.is_none() {
        // A known group word after the first marker.
        if let Some(k) = toks
            .iter()
            .enumerate()
            .position(|(k, t)| t.tag == Tag::Group && first_marker.is_some_and(|f| k > f))
        {
            group = Some(toks[k].display.clone());
        }
    }
    if group.is_none() {
        // A trailing bracket of free words after release info: `[YTS]`.
        if let Some((bi, b)) = brackets.iter().enumerate().next_back()
            && bi > 0
            && !b.tokens.is_empty()
            && b.tokens.len() <= 2
            && toks[..b.tokens.start].iter().any(|t| {
                t.tag.is_release_info()
                    || t.tag.is_episode_marker()
                    || matches!(t.tag, Tag::Year(_))
            })
            && toks[b.tokens.clone()]
                .iter()
                .all(|t| matches!(t.tag, Tag::Free | Tag::Group))
            && b.tokens.end == toks.len()
        {
            group = Some(inner_text(b));
            for t in &mut toks[b.tokens.clone()] {
                t.tag = Tag::Group;
            }
        }
    }
    // Known group words in plain text that did not become *the* group are title words.
    for (k, t) in toks.iter_mut().enumerate() {
        if t.tag == Tag::Group && plain(t) && !first_marker.is_some_and(|f| k > f) {
            t.tag = Tag::Free;
        }
    }

    let crc_or_fansub = group_bracket.is_some()
        || toks.iter().any(|t| matches!(t.tag, Tag::Crc(_)))
        || group.as_deref().is_some_and(|g| {
            lexicon::lookup(&g.to_lowercase()).is_some_and(|e| e.lex == lexicon::Lex::Group)
        });

    // --- Ambiguity points ----------------------------------------------
    let mut points = Vec::new();
    let explicit_ep = toks.iter().any(|t| t.tag.is_episode_marker());
    let has_season = ctx.season_known || toks.iter().any(|t| matches!(t.tag, Tag::Season(_)));
    let has_kind = toks.iter().any(|t| matches!(t.tag, Tag::Kind(_, None)));
    let strong_year = toks.iter().any(|t| matches!(t.tag, Tag::Year(_)));
    let plain_idx: Vec<usize> = (0..toks.len()).filter(|&k| plain(&toks[k])).collect();
    let first_plain = plain_idx.first().copied();
    let has_free_words = plain_idx.iter().any(|&k| toks[k].tag == Tag::Free);
    let is_end_of_title = |k: usize| {
        // Next non-consumed token is a bracket, release info, marker or end.
        let next = (k + 1..toks.len()).find(|&j| toks[j].tag != Tag::Consumed);
        match next {
            None => true,
            Some(j) => {
                let t = &toks[j];
                !plain(t)
                    || t.tok.dash_before
                    || t.tag.is_release_info()
                    || matches!(t.tag, Tag::Kind(..) | Tag::Version(_) | Tag::Part(_))
            }
        }
    };

    // Year: the last year-like number that is not the first token.
    let year_tok = if strong_year {
        None
    } else {
        plain_idx.iter().copied().rfind(|&k| {
            num(&toks[k]).is_some_and(|n| n.year_like.is_some()) && Some(k) != first_plain
        })
    };
    if let Some(k) = year_tok {
        let followed_by_release = (k + 1..toks.len())
            .find(|&j| toks[j].tag != Tag::Consumed)
            .is_some_and(|j| {
                !plain(&toks[j])
                    || toks[j].tag.is_release_info()
                    || toks[j].tag.is_episode_marker()
                    || matches!(toks[j].tag, Tag::Part(_))
            });
        let w = if followed_by_release { 0.93 } else { 0.8 };
        points.push(Point {
            token: k,
            options: vec![(Role::Year, w), (Role::Title, 1.0 - w)],
        });
    }
    // Leading year-like number with nothing else to be the year.
    if let Some(k) = first_plain
        && year_tok.is_none()
        && !strong_year
        && num(&toks[k]).is_some_and(|n| n.year_like.is_some())
    {
        let more_title = plain_idx
            .iter()
            .any(|&j| j > k && matches!(toks[j].tag, Tag::Free));
        if !explicit_ep {
            let w = if more_title { 0.9 } else { 0.6 };
            points.push(Point {
                token: k,
                options: vec![(Role::Title, w), (Role::Year, 1.0 - w)],
            });
        }
    }

    // Episode number.
    if !explicit_ep {
        let candidate = |k: usize| {
            num(&toks[k]).is_some_and(|n| {
                n.year_like.is_none() && Some(k) != year_tok && n.values[0].number < 100_000
            })
        };
        let after_dash = plain_idx
            .iter()
            .copied()
            .rfind(|&k| candidate(k) && toks[k].tok.dash_before);
        let in_bracket = || {
            brackets.iter().enumerate().find_map(|(bi, b)| {
                (Some(bi) != group_bracket && b.tokens.len() == 1 && candidate(b.tokens.start))
                    .then_some(b.tokens.start)
            })
        };
        let at_title_end = || {
            plain_idx.iter().copied().rfind(|&k| {
                candidate(k) && (Some(k) != first_plain || !has_free_words) && is_end_of_title(k)
            })
        };
        let (cand, style) = match after_dash {
            Some(k) => (Some(k), 0),
            None => match in_bracket() {
                Some(k) if plain_idx.is_empty() || !plain_idx.iter().any(|&j| candidate(j)) => {
                    (Some(k), 1)
                }
                _ => (at_title_end(), 2),
            },
        };
        if let Some(k) = cand {
            let n = num(&toks[k]).expect("candidate is a number");
            let big = n.digits >= 3 || n.values[0].number >= 100;
            let structured = n.range || n.version.is_some() || n.values[0].fraction.is_some();
            let movie_like = style == 2 && (strong_year || year_tok.is_some());
            let mut w = if has_season || has_kind {
                [0.86, 0.04, 0.10]
            } else if movie_like {
                [0.12, 0.03, 0.85]
            } else if big {
                [0.22, 0.70, 0.08]
            } else if style == 0 || style == 1 {
                [0.70, 0.20, 0.10]
            } else if crc_or_fansub {
                [0.60, 0.15, 0.25]
            } else if strong_year || year_tok.is_some() {
                [0.12, 0.03, 0.85]
            } else {
                [0.40, 0.15, 0.45]
            };
            if structured {
                w[2] = 0.0;
            }
            if n.values[0].fraction.is_some() {
                w = [0.95, 0.05, 0.0];
            }
            let options = [
                (Role::Episode, w[0]),
                (Role::Absolute, w[1]),
                (Role::Title, w[2]),
            ]
            .into_iter()
            .filter(|&(_, w)| w > 0.0)
            .collect();
            points.push(Point { token: k, options });
        }
    }

    Analysis {
        toks,
        brackets,
        points,
        group_bracket,
        group,
        crc_or_fansub,
    }
}

fn join_title(parts: &[&Classified]) -> String {
    let mut s = String::new();
    for (i, t) in parts.iter().enumerate() {
        if i > 0 {
            s.push_str(if t.tok.dash_before { " - " } else { " " });
        }
        s.push_str(&t.tok.text);
    }
    s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | ',' | ':' | '–' | '—'))
        .to_string()
}

/// Builds one hypothesis for a choice of roles (`choice[i]` indexes `points[i].options`).
pub(crate) fn build(a: &Analysis, choice: &[usize]) -> (ParsedName, Vec<String>) {
    let toks = &a.toks;
    let mut roles: Vec<Option<Role>> = vec![None; toks.len()];
    let mut reasons = Vec::new();
    for (p, &c) in a.points.iter().zip(choice) {
        let role = p.options[c].0;
        roles[p.token] = Some(role);
        reasons.push(format!(
            "number {}:{}",
            toks[p.token].tok.text,
            role.reason()
        ));
    }
    let mut out = ParsedName::default();

    let titlish = |k: usize| -> bool {
        match &toks[k].tag {
            Tag::Free => true,
            Tag::Num(_) => matches!(roles[k], None | Some(Role::Title)),
            _ => false,
        }
    };
    let marker = |k: usize| -> bool {
        toks[k].tag.is_episode_marker()
            || matches!(roles[k], Some(Role::Episode | Role::Absolute))
            || matches!(toks[k].tag, Tag::Season(_) | Tag::Kind(..))
    };

    // Title runs over plain tokens.
    let mut titles: Vec<Vec<&Classified>> = Vec::new();
    let mut episode_title: Vec<&Classified> = Vec::new();
    let mut state = 0; // 0: looking for title, 1: in title, 2: title done
    let mut seen_marker_before_title = false;
    let mut after_marker = false;
    let mut in_ep_title = false;
    #[allow(clippy::needless_range_loop)] // `k` also indexes `roles`
    for k in 0..toks.len() {
        let t = &toks[k];
        if t.tag == Tag::Consumed {
            continue;
        }
        if !plain(t) {
            if state == 1 {
                state = 2;
            }
            in_ep_title = false;
            continue;
        }
        if titlish(k) {
            match state {
                0 if !seen_marker_before_title => {
                    state = 1;
                    titles.push(vec![t]);
                }
                1 => {
                    if t.tok.slash_before {
                        titles.push(vec![t]);
                    } else {
                        titles.last_mut().expect("title started").push(t);
                    }
                }
                _ => {
                    if after_marker && (episode_title.is_empty() || in_ep_title) {
                        in_ep_title = true;
                        episode_title.push(t);
                    }
                }
            }
        } else {
            if state == 0 && marker(k) {
                seen_marker_before_title = true;
            }
            if state == 1 {
                state = 2;
            }
            in_ep_title = false;
            after_marker = marker(k);
            if after_marker {
                episode_title.clear();
            }
        }
    }
    // Title in its own bracket: `[Group][Title][05]`.
    if titles.is_empty() && !seen_marker_before_title {
        for (bi, b) in a.brackets.iter().enumerate() {
            if Some(bi) == a.group_bracket || b.tokens.is_empty() {
                continue;
            }
            let ts = &toks[b.tokens.clone()];
            if ts
                .iter()
                .all(|t| matches!(t.tag, Tag::Free | Tag::Consumed))
                && ts
                    .iter()
                    .any(|t| t.tag == Tag::Free && t.tok.text.chars().any(char::is_alphabetic))
            {
                titles.push(ts.iter().collect());
                reasons.push("title:bracket".into());
                break;
            }
        }
    }
    let mut titles = titles
        .into_iter()
        .map(|p| join_title(&p))
        .filter(|s| !s.is_empty());
    out.title = titles.next();
    out.alt_titles = titles.collect();
    if !episode_title.is_empty() {
        let s = join_title(&episode_title);
        if !s.is_empty() {
            out.episode_title = Some(s);
        }
    }

    // Tags.
    let mut langs: Vec<String> = Vec::new();
    let mut dub_seen = false;
    for (k, t) in toks.iter().enumerate() {
        let first = |slot: &mut Option<String>| {
            if slot.is_none() {
                *slot = Some(t.display.clone());
            }
        };
        match &t.tag {
            Tag::Num(n) => match roles[k] {
                Some(Role::Episode) => {
                    out.episodes.extend(n.values.iter().copied());
                    out.version = out.version.or(n.version);
                }
                Some(Role::Absolute) => {
                    out.absolute.extend(n.values.iter().copied());
                    out.version = out.version.or(n.version);
                }
                Some(Role::Year) => out.year = out.year.or(n.year_like),
                _ => {}
            },
            Tag::Year(y) => out.year = out.year.or(Some(*y)),
            Tag::SeasonEp {
                season,
                episodes,
                version,
            } => {
                if out.season.is_none() {
                    out.season = *season;
                }
                if out.episodes.is_empty() {
                    out.episodes.extend(episodes.iter().copied());
                }
                out.version = out.version.or(*version);
            }
            Tag::Season(s) => out.season = out.season.or(Some(*s)),
            Tag::Kind(kind, n) => {
                if out.episode_kind.is_none() {
                    out.episode_kind = Some(*kind);
                }
                if let Some(n) = n
                    && out.episodes.is_empty()
                {
                    out.episodes.push(*n);
                }
                match kind {
                    EpisodeKind::Ncop => out.extra = out.extra.or(Some(ExtraKind::Opening)),
                    EpisodeKind::Nced => out.extra = out.extra.or(Some(ExtraKind::Ending)),
                    _ => {}
                }
            }
            Tag::Part(p) => out.part = out.part.or(Some(*p)),
            Tag::Date(d) => out.date = out.date.or(Some(*d)),
            Tag::Resolution => first(&mut out.resolution),
            Tag::Source => first(&mut out.source),
            Tag::VideoCodec => first(&mut out.video_codec),
            Tag::AudioCodec => first(&mut out.audio_codec),
            Tag::Hdr => first(&mut out.hdr),
            Tag::Edition => first(&mut out.edition),
            Tag::Lang(l) => {
                if l.is_empty() {
                    push_unique(&mut out.other, "MULTI".into());
                } else {
                    push_unique(&mut langs, (*l).to_string());
                }
            }
            Tag::Other => push_unique(&mut out.other, t.display.to_uppercase()),
            Tag::Extra(e) => out.extra = out.extra.or(Some(*e)),
            Tag::Crc(c) => out.crc32 = out.crc32.clone().or(Some(c.clone())),
            Tag::Version(v) => out.version = out.version.or(Some(*v)),
            Tag::Dub => {
                if out.dub_studio.is_none() {
                    out.dub_studio = Some(t.display.clone());
                }
                dub_seen = true;
            }
            Tag::Group if t.tok.bracket.is_some() && t.tok.bracket == a.group_bracket => {
                // Dub studio as release group: `[AniLibria]`.
                let l = t.display.to_lowercase();
                if lexicon::is_dub_studio(&l) || lexicon::is_dub_studio(l.trim_end_matches(".tv")) {
                    if out.dub_studio.is_none() {
                        out.dub_studio = a.group.clone();
                    }
                    dub_seen = true;
                }
            }
            Tag::Free | Tag::Group | Tag::Consumed => {}
        }
    }
    if dub_seen {
        push_unique(&mut langs, "rus".into());
    }
    out.languages = langs;
    out.release_group = a.group.clone();

    out.anime_hint = a.crc_or_fansub
        || roles
            .iter()
            .any(|r| matches!(r, Some(Role::Episode | Role::Absolute)))
            && a.toks.iter().any(|t| t.tok.dash_before)
        || matches!(
            out.episode_kind,
            Some(EpisodeKind::Ncop | EpisodeKind::Nced | EpisodeKind::Ova | EpisodeKind::Ona)
        )
        || out.title.as_deref().is_some_and(|t| t.chars().any(is_cjk));
    out.media = media_hint(&out);
    (out, reasons)
}

pub(crate) fn media_hint(p: &ParsedName) -> MediaHint {
    if p.extra.is_some() {
        MediaHint::Extra
    } else if p.season.is_some()
        || !p.episodes.is_empty()
        || !p.absolute.is_empty()
        || p.date.is_some()
        || p.episode_kind.is_some()
    {
        MediaHint::Episode
    } else if p.year.is_some() || p.disc.is_some() || p.part.is_some() {
        MediaHint::Movie
    } else {
        MediaHint::Unknown
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF)
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Enumerates all choices (cartesian product of point options) with weights.
pub(crate) fn choices(points: &[Point]) -> Vec<(Vec<usize>, f32)> {
    let mut out = vec![(Vec::new(), 1.0f32)];
    for p in points {
        let mut next = Vec::with_capacity(out.len() * p.options.len());
        for (c, w) in &out {
            for (i, &(_, ow)) in p.options.iter().enumerate() {
                let mut c2 = c.clone();
                c2.push(i);
                next.push((c2, w * ow));
            }
        }
        out = next;
    }
    out
}

/// Episode numbers helper for tests.
#[allow(dead_code)]
pub(crate) fn eps(v: &[u32]) -> Vec<EpisodeNumber> {
    v.iter().copied().map(EpisodeNumber::whole).collect()
}
