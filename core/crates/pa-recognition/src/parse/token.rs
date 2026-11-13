// SPDX-License-Identifier: MPL-2.0
//! Tokenizer: splits a name (without extension) into tokens and bracket groups.
//!
//! Single left-to-right pass plus a bracket-matching pre-pass, both linear in
//! the length of the name. Rules:
//!
//! - whitespace and `_` always separate tokens;
//! - `.` separates tokens when it is surrounded by non-space characters
//!   (`Breaking.Bad`), except inside decimals (`18.5`, `DD5.1`, dates
//!   `2025.03.14`) and `H.264`; a trailing `.` before a space stays with its
//!   word (`Kimi no Na wa.`, `Mr. Robot`);
//! - a `-` (or `–`, `—`) next to a separator is a *dash delimiter*
//!   (`Title - 05`); inside a word it is kept (`Erai-raws`, `WEB-DL`, `9-1-1`);
//! - `/`, `∕`, `|`, `／` are *slash delimiters* between alternative titles;
//! - `[] () {} 【】 「」 『』 （）` form bracket groups; unmatched brackets
//!   are treated as separators.

use std::ops::Range;

/// One token of a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    /// Original text of the token.
    pub text: String,
    /// Byte range in the name.
    pub span: Range<usize>,
    /// Index of the enclosing bracket group.
    pub bracket: Option<usize>,
    /// A dash delimiter precedes the token.
    pub dash_before: bool,
    /// A slash delimiter precedes the token.
    pub slash_before: bool,
    /// The token was split off a hyphenated word (`x264-GROUP` → `GROUP`).
    pub glued_hyphen: bool,
}

/// A bracket group and the tokens inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bracket {
    pub open: char,
    pub tokens: Range<usize>,
    /// Byte range of the content (without the brackets).
    pub inner: Range<usize>,
}

fn closing(open: char) -> Option<char> {
    Some(match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        '【' => '】',
        '「' => '」',
        '『' => '』',
        '（' => '）',
        _ => return None,
    })
}

fn is_close(c: char) -> bool {
    matches!(c, ']' | ')' | '}' | '】' | '」' | '』' | '）')
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '–' | '—')
}

fn is_slash(c: char) -> bool {
    matches!(c, '/' | '∕' | '|' | '／')
}

/// Matches brackets with a stack; returns for every char index the index of
/// its partner, if any. Linear time.
fn match_brackets(chars: &[(usize, char)]) -> Vec<Option<usize>> {
    let mut partner = vec![None; chars.len()];
    let mut stack: Vec<(usize, char)> = Vec::new();
    for (i, &(_, c)) in chars.iter().enumerate() {
        if let Some(close) = closing(c) {
            stack.push((i, close));
        } else if is_close(c) {
            // Pop until the matching opener; skipped openers stay unmatched.
            if let Some(pos) = stack.iter().rposition(|&(_, cl)| cl == c) {
                let (open_i, _) = stack[pos];
                stack.truncate(pos);
                partner[open_i] = Some(i);
                partner[i] = Some(open_i);
            }
        }
    }
    partner
}

/// Tokenizes a name. Returns tokens and bracket groups.
pub(crate) fn tokenize(name: &str) -> (Vec<Token>, Vec<Bracket>) {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let partner = match_brackets(&chars);
    let n = chars.len();
    let byte_at = |i: usize| if i < n { chars[i].0 } else { name.len() };
    let ch = |i: usize| chars.get(i).map(|&(_, c)| c);

    let mut tokens: Vec<Token> = Vec::new();
    let mut brackets: Vec<Bracket> = Vec::new();
    // Stack of open bracket groups (index into `brackets`), with their closing char index.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut tok_start: Option<usize> = None;
    let mut pending_dash = false;
    let mut pending_slash = false;

    let flush = |tokens: &mut Vec<Token>,
                 tok_start: &mut Option<usize>,
                 end: usize,
                 open: &[(usize, usize)],
                 dash: &mut bool,
                 slash: &mut bool| {
        if let Some(s) = tok_start.take() {
            let (bs, be) = (byte_at(s), byte_at(end));
            if bs < be {
                tokens.push(Token {
                    text: name[bs..be].to_string(),
                    span: bs..be,
                    bracket: open.first().map(|&(b, _)| b),
                    dash_before: std::mem::take(dash),
                    slash_before: std::mem::take(slash),
                    glued_hyphen: false,
                });
            }
        }
    };

    let is_sep_char = |c: char| c.is_whitespace() || c == '_';

    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        // Brackets.
        if closing(c).is_some() {
            if let Some(close_i) = partner[i] {
                flush(
                    &mut tokens,
                    &mut tok_start,
                    i,
                    &open,
                    &mut pending_dash,
                    &mut pending_slash,
                );
                if open.is_empty() {
                    brackets.push(Bracket {
                        open: c,
                        tokens: tokens.len()..tokens.len(),
                        inner: byte_at(i + 1)..byte_at(close_i),
                    });
                    open.push((brackets.len() - 1, close_i));
                } else {
                    // Nested: tokens belong to the outermost group.
                    let outer = open[0].0;
                    open.push((outer, close_i));
                }
                i += 1;
                continue;
            }
            flush(
                &mut tokens,
                &mut tok_start,
                i,
                &open,
                &mut pending_dash,
                &mut pending_slash,
            );
            i += 1;
            continue;
        }
        if is_close(c) {
            flush(
                &mut tokens,
                &mut tok_start,
                i,
                &open,
                &mut pending_dash,
                &mut pending_slash,
            );
            if let Some(&(b, close_i)) = open.last()
                && close_i == i
            {
                open.pop();
                if open.is_empty() {
                    brackets[b].tokens.end = tokens.len();
                }
            }
            i += 1;
            continue;
        }
        let in_bracket = !open.is_empty();
        let prev = if i > 0 { ch(i - 1) } else { None };
        let next = ch(i + 1);
        let sep_or_edge = |o: Option<char>| match o {
            None => true,
            Some(x) => is_sep_char(x) || x == '.' || closing(x).is_some() || is_close(x),
        };

        let separator = if is_sep_char(c) || (in_bracket && c == ',') {
            true
        } else if c == '.' {
            dot_is_separator(&chars, i)
        } else if is_dash(c) {
            if sep_or_edge(prev) || sep_or_edge(next) {
                if !in_bracket {
                    pending_dash = true;
                }
                true
            } else {
                false
            }
        } else if is_slash(c) {
            if !in_bracket {
                pending_slash = true;
            }
            true
        } else {
            false
        };

        if separator {
            flush(
                &mut tokens,
                &mut tok_start,
                i,
                &open,
                &mut pending_dash,
                &mut pending_slash,
            );
        } else if tok_start.is_none() {
            tok_start = Some(i);
        }
        i += 1;
    }
    flush(
        &mut tokens,
        &mut tok_start,
        n,
        &open,
        &mut pending_dash,
        &mut pending_slash,
    );
    // Close groups left open (cannot happen with matched brackets, but be safe).
    for &(b, _) in open.iter().take(1) {
        brackets[b].tokens.end = tokens.len();
    }
    (tokens, brackets)
}

/// Whether the `.` at char index `i` separates tokens.
fn dot_is_separator(chars: &[(usize, char)], i: usize) -> bool {
    let ch = |j: usize| chars.get(j).map(|&(_, c)| c);
    let prev = if i > 0 { ch(i - 1) } else { None };
    let next = ch(i + 1);
    let (Some(p), Some(nx)) = (prev, next) else {
        return false; // leading/trailing dot stays with its word
    };
    if p.is_whitespace() || nx.is_whitespace() {
        return false; // `wa. (2016)`, `Mr. Robot`
    }
    if p.is_ascii_digit() && nx.is_ascii_digit() {
        // Decimal or date part: up to 2 digits after the dot, then a non-alphanumeric char.
        let mut j = i + 1;
        let mut digits = 0;
        while let Some(d) = ch(j) {
            if d.is_ascii_digit() {
                digits += 1;
                j += 1;
            } else {
                break;
            }
        }
        let after_ok = ch(j).is_none_or(|a| !a.is_alphanumeric());
        if digits <= 2 && after_ok {
            return false;
        }
    }
    if (p == 'h' || p == 'H')
        && (i < 2 || ch(i - 2).is_none_or(|b| !b.is_alphanumeric()))
        && ch(i + 1) == Some('2')
        && ch(i + 2) == Some('6')
        && matches!(ch(i + 3), Some('4' | '5'))
        && ch(i + 4).is_none_or(|a| !a.is_alphanumeric())
    {
        return false; // H.264
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(name: &str) -> Vec<String> {
        tokenize(name).0.into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn dotted_names() {
        assert_eq!(
            texts("Breaking.Bad.S01E01.1080p.BluRay.x264-GROUP"),
            ["Breaking", "Bad", "S01E01", "1080p", "BluRay", "x264-GROUP"]
        );
        assert_eq!(
            texts("The.Daily.Show.2025.03.14.Guest"),
            ["The", "Daily", "Show", "2025.03.14", "Guest"]
        );
        assert_eq!(texts("Movie.DD5.1.H.264"), ["Movie", "DD5.1", "H.264"]);
        assert_eq!(texts("1917.2019.2160p"), ["1917", "2019", "2160p"]);
    }

    #[test]
    fn brackets_and_dashes() {
        let (toks, br) = tokenize("[SubsPlease] One Piece - 1085 (1080p) [ABCD1234]");
        let t: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            t,
            ["SubsPlease", "One", "Piece", "1085", "1080p", "ABCD1234"]
        );
        assert_eq!(br.len(), 3);
        assert!(toks[3].dash_before);
        assert_eq!(toks[0].bracket, Some(0));
        assert_eq!(toks[4].bracket, Some(1));
        assert_eq!(br[1].open, '(');
    }

    #[test]
    fn keeps_trailing_dot_and_inner_hyphen() {
        assert_eq!(
            texts("Kimi no Na wa. (2016)"),
            ["Kimi", "no", "Na", "wa.", "2016"]
        );
        assert_eq!(texts("Erai-raws 9-1-1"), ["Erai-raws", "9-1-1"]);
    }

    #[test]
    fn slash_and_unmatched() {
        let (toks, br) = tokenize("Мастер ∕ Master (2024");
        assert!(toks[1].slash_before);
        assert!(br.is_empty());
        assert_eq!(toks.last().unwrap().text, "2024");
    }

    #[test]
    fn nested_brackets_belong_to_outer() {
        let (toks, br) = tokenize("[A [B] C] D");
        assert_eq!(br.len(), 1);
        assert_eq!(br[0].tokens, 0..3);
        assert_eq!(toks[3].bracket, None);
    }
}
