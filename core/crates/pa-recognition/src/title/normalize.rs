// SPDX-License-Identifier: MPL-2.0
//! Title normalization: the match keys of the title index.
//!
//! Every title (from the catalogue and from a parsed file name) becomes a
//! [`TitleKey`] with three nested keys, from strict to loose:
//!
//! 1. **plain** — [`fold`]: NFKC (full-width letters, ligatures, `Ⅲ` → `III`),
//!    case folding, diacritics removed from Latin and Greek letters (`ō` →
//!    `o`, `é` → `e`; Cyrillic `й` stays, `ё` → `е`), apostrophes dropped
//!    (`Gintama'` → `gintama`, `Director's` → `directors`), `&` → `and`,
//!    other punctuation → space. Then roman numerals become arabic (`Rocky
//!    IV` → `rocky 4`) and leading articles are removed (`The`, `A`, `An`,
//!    `Der`, `Le`, `El`… and a trailing `, The`).
//! 2. **loose** — plain tokens mapped into one Latin space where common
//!    spellings collapse ([`loose_token`]): Cyrillic is transliterated, then
//!    transliteration variants merge (`ja`/`ia`/`iya` → `ya`, `kh` → `h`,
//!    `zh` → `j`, `shch`/`sch` → `sh`, final `-iy`/`-yy`/`-ij`/`-i` → `-y`),
//!    romaji long vowels merge (`ou`/`oo`/`ō` → `o`, `uu` → `u`, double
//!    letters collapse, particle `wo` → `o`, `mb` → `nb`). Folding is applied
//!    to both sides, so a collision only costs a slightly larger candidate
//!    list; the plain key still rewards an exact spelling.
//! 3. **base** — the loose key without trailing qualifiers: season (`Season
//!    2`, `2nd Season`, `S2`, `Сезон 2`, `第2期`, a trailing `2`/`II`), part
//!    (`Part 2`, `Cour 2`, `Часть 2`), `Final Season`, a trailing year
//!    (`Fairy Tail (2014)`). The qualifiers are kept in the key.
//!
//! All functions are pure and linear in the input length.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Normalized forms of one title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleKey {
    /// Strict key: folded, numerals, articles removed.
    pub plain: String,
    /// Loose key: one Latin space for transliteration and romaji variants.
    pub loose: String,
    /// Loose key without trailing season/part/year qualifiers.
    pub base: String,
    /// Season or sequel number found in the qualifiers.
    pub season: Option<u32>,
    /// Part or cour number found in the qualifiers.
    pub part: Option<u32>,
    /// Year found at the end of the title (`Title (2014)`).
    pub year: Option<u16>,
}

impl TitleKey {
    /// Builds all keys of `title`.
    ///
    /// ```
    /// use pa_recognition::title::normalize::TitleKey;
    ///
    /// let k = TitleKey::new("Mob Psycho 100 III");
    /// assert_eq!(k.plain, "mob psycho 100 3");
    /// assert_eq!(k.base, "mob psycho 100");
    /// assert_eq!(k.season, Some(3));
    ///
    /// assert_eq!(TitleKey::new("Ironiya sudby").loose, TitleKey::new("Ирония судьбы").loose);
    /// assert_eq!(TitleKey::new("Kyōkai no Kanata").loose, TitleKey::new("Kyoukai no Kanata").loose);
    /// ```
    pub fn new(title: &str) -> Self {
        let folded = fold(title);
        let mut tokens: Vec<String> = folded.split(' ').map(roman_to_arabic).collect();
        strip_articles(&mut tokens);
        let plain = tokens.join(" ");

        let mut loose: Vec<String> = tokens
            .iter()
            .map(|t| {
                let lat = cyrillic_to_latin(t);
                // Transliterated `и`/`в` must become the same numerals as a
                // Latin query (`Voyna i mir` → `voyna 1 mir`).
                loose_token(&roman_to_arabic(&lat))
            })
            .filter(|t| !t.is_empty())
            .collect();
        if loose.is_empty() {
            loose = tokens.clone();
        }
        let loose_key = loose.join(" ");

        let mut key = TitleKey {
            plain,
            loose: loose_key,
            ..Default::default()
        };
        strip_qualifiers(&mut loose, &mut key);
        key.base = loose.join(" ");
        key
    }

    /// The loose key without spaces (`mobpsycho100`): catches `MobPsycho`,
    /// `Spider-Man`/`Spiderman`.
    pub fn compact(&self) -> String {
        self.loose.chars().filter(|c| *c != ' ').collect()
    }
}

/// NFKC, case folding, diacritics and punctuation removal; tokens are
/// separated by single spaces.
///
/// ```
/// use pa_recognition::title::normalize::fold;
/// assert_eq!(fold("Ｆａｔｅ／ｚｅｒｏ"), "fate zero");
/// assert_eq!(fold("Kyōkai no Kanata"), "kyokai no kanata");
/// assert_eq!(fold("Léon: The Professional"), "leon the professional");
/// assert_eq!(fold("Ёлки-палки!"), "елки палки");
/// assert_eq!(fold("Tom & Jerry"), "tom and jerry");
/// assert_eq!(fold("Gintama'"), "gintama");
/// ```
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfkc() {
        for c in c.to_lowercase() {
            fold_char(c, &mut out);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold_char(c: char, out: &mut String) {
    match c {
        // Apostrophes join: `Gintama'`, `Director's`, `Sud'by`.
        '\'' | '’' | '‘' | '`' | '´' | 'ʼ' | 'ʹ' | '′' | 'ʻ' => {}
        '&' => out.push_str(" and "),
        'ё' => out.push('е'),
        'ß' => out.push_str("ss"),
        'æ' => out.push_str("ae"),
        'œ' => out.push_str("oe"),
        'ø' => out.push('o'),
        'ł' => out.push('l'),
        'đ' | 'ð' => out.push('d'),
        'þ' => out.push_str("th"),
        'ı' => out.push('i'),
        _ if is_latin_or_greek(c) => {
            for d in std::iter::once(c).nfd() {
                if !is_combining_mark(d) {
                    out.push(d);
                }
            }
        }
        _ if c.is_alphanumeric() => out.push(c),
        // Kana voicing marks and other combining marks stay with their letter.
        _ if is_combining_mark(c) => out.push(c),
        _ => out.push(' '),
    }
}

fn is_latin_or_greek(c: char) -> bool {
    matches!(c as u32, 0x00C0..=0x024F | 0x1E00..=0x1EFF | 0x0370..=0x03FF)
}

/// Converts a roman numeral token (`ii` … `xxxix`) to arabic digits; other
/// tokens are returned unchanged.
fn roman_to_arabic(tok: &str) -> String {
    if tok.is_empty() || tok.len() > 7 || !tok.bytes().all(|b| matches!(b, b'i' | b'v' | b'x')) {
        return tok.to_owned();
    }
    let value = |b: u8| match b {
        b'i' => 1,
        b'v' => 5,
        _ => 10,
    };
    let bytes = tok.as_bytes();
    let mut n = 0i32;
    for (i, &b) in bytes.iter().enumerate() {
        let v = value(b);
        if bytes.get(i + 1).is_some_and(|&next| value(next) > v) {
            n -= v;
        } else {
            n += v;
        }
    }
    // Only canonical spellings (`iv`, not `iiii` or `vx`).
    if n > 0 && to_roman(n as u32) == tok {
        n.to_string()
    } else {
        tok.to_owned()
    }
}

fn to_roman(mut n: u32) -> String {
    let mut s = String::new();
    for (v, r) in [(10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

const LEADING_ARTICLES: &[&str] = &[
    "the", "a", "an", "der", "die", "das", "le", "la", "les", "el", "los", "las", "il", "lo",
];

fn strip_articles(tokens: &mut Vec<String>) {
    if tokens.len() > 1 && LEADING_ARTICLES.contains(&tokens[0].as_str()) {
        tokens.remove(0);
    }
    if tokens.len() > 1 && tokens.last().is_some_and(|t| t == "the") {
        tokens.pop();
    }
}

/// Transliterates the Cyrillic letters of a folded (lower-case) token.
fn cyrillic_to_latin(tok: &str) -> String {
    if !tok.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
        return tok.to_owned();
    }
    super::translit::to_latin(tok, super::translit::Scheme::Common)
}

/// Maps one lower-case Latin token into the loose matching space (see the
/// module docs). Tokens with other scripts are returned unchanged.
///
/// ```
/// use pa_recognition::title::normalize::loose_token;
/// assert_eq!(loose_token("ironija"), loose_token("ironiya"));
/// assert_eq!(loose_token("ironia"), loose_token("ironiya"));
/// assert_eq!(loose_token("sudbi"), loose_token("sudby"));
/// assert_eq!(loose_token("shippuuden"), loose_token("shippuden"));
/// assert_eq!(loose_token("kyoujin"), loose_token("kyojin"));
/// assert_eq!(loose_token("dostoevskij"), loose_token("dostoevskiy"));
/// ```
pub fn loose_token(tok: &str) -> String {
    if !tok.is_ascii() || tok.bytes().all(|b| b.is_ascii_digit()) {
        return tok.to_owned();
    }
    let mut t = tok.to_owned();
    for (from, to) in [
        // Transliteration variants of one Cyrillic letter.
        ("shch", "sh"),
        ("sch", "sh"),
        ("tch", "ch"),
        ("kh", "h"),
        ("zh", "j"),
        ("tz", "ts"),
        ("x", "ks"),
        ("ja", "ya"),
        ("ju", "yu"),
        ("jo", "yo"),
        ("iya", "ya"),
        ("iyu", "yu"),
        ("ia", "ya"),
        ("iu", "yu"),
        ("ye", "e"),
        ("ie", "e"),
    ] {
        if t.contains(from) {
            t = t.replace(from, to);
        }
    }
    // `-ий`, `-ый`, `-ой` endings and `й`/`ы` written as `i`, `j`, `y`.
    for end in ["iy", "yy", "ij", "ii", "yi"] {
        if t.len() > end.len() + 1 && t.ends_with(end) {
            t.truncate(t.len() - end.len());
            t.push('y');
            break;
        }
    }
    if t.len() > 2 && (t.ends_with('i') || t.ends_with('j')) {
        t.pop();
        t.push('y');
    }
    // Romaji: long vowels and `n` before labials.
    for (from, to) in [("ou", "o"), ("mb", "nb"), ("mp", "np")] {
        if t.contains(from) {
            t = t.replace(from, to);
        }
    }
    // Double letters (`shippuuden`, `kawaii`, `professiya`/`profesiya`).
    let mut out = String::with_capacity(t.len());
    let mut last = None;
    for c in t.chars() {
        if Some(c) != last || c.is_ascii_digit() {
            out.push(c);
        }
        last = Some(c);
    }
    if out == "wo" {
        out = "o".into();
    }
    out
}

fn number(tok: &str) -> Option<u32> {
    if tok.is_empty() || tok.len() > 4 || !tok.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tok.parse().ok()
}

/// `2nd`, `3rd`, `second`, `third`… or a plain number.
fn ordinal(tok: &str) -> Option<u32> {
    if let Some(n) = number(tok) {
        return Some(n);
    }
    for suffix in ["st", "nd", "rd", "th", "y", "j"] {
        if let Some(n) = tok.strip_suffix(suffix).and_then(number) {
            return Some(n);
        }
    }
    const WORDS: &[&str] = &[
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth",
    ];
    WORDS
        .iter()
        .position(|w| loose_token(w) == tok)
        .map(|i| i as u32 + 1)
}

fn is_word(tok: &str, words: &[&str]) -> bool {
    words.iter().any(|w| loose_token(w) == tok)
}

const SEASON_WORDS: &[&str] = &[
    "season",
    "seasons",
    "сезон",
    "sezon",
    "saison",
    "staffel",
    "temporada",
    "stagione",
    "series",
];
const PART_WORDS: &[&str] = &["part", "cour", "часть", "chast", "partie", "teil", "parte"];

/// Season number of a CJK token: `第2期`, `2期`, `第二季`.
fn cjk_season(tok: &str) -> Option<u32> {
    let body = tok
        .strip_suffix('期')
        .or_else(|| tok.strip_suffix('季'))?;
    let body = body.strip_prefix('第').unwrap_or(body);
    if let Some(n) = number(body) {
        return Some(n);
    }
    let digits = "一二三四五六七八九十";
    let mut chars = body.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    digits.chars().position(|d| d == c).map(|i| i as u32 + 1)
}

/// Removes trailing qualifiers from loose tokens into `key`.
fn strip_qualifiers(tokens: &mut Vec<String>, key: &mut TitleKey) {
    for _ in 0..5 {
        let n = tokens.len();
        if n < 2 {
            return;
        }
        let last = tokens[n - 1].as_str();
        let prev = tokens[n - 2].as_str();
        let (pop, season, part) = if let Some(s) = number(last).filter(|_| is_word(prev, SEASON_WORDS))
        {
            (2, Some(s), None)
        } else if is_word(last, SEASON_WORDS)
            && let Some(s) = ordinal(prev)
        {
            (2, Some(s), None)
        } else if is_word(last, SEASON_WORDS) && prev == "final" {
            (2, None, None)
        } else if let Some(p) = number(last).filter(|_| is_word(prev, PART_WORDS)) {
            (2, None, Some(p))
        } else if is_word(last, PART_WORDS)
            && let Some(p) = ordinal(prev)
        {
            (2, None, Some(p))
        } else if let Some(s) = last.strip_prefix('s').and_then(number) {
            (1, Some(s), None)
        } else if let Some(s) = cjk_season(last) {
            (1, Some(s), None)
        } else if last == "final" || last == "the" {
            (1, None, None)
        } else if let Some(y) = number(last).filter(|y| (1900..=2099).contains(y)) {
            key.year.get_or_insert(y as u16);
            (1, None, None)
        } else if let Some(s) = number(last).filter(|s| (1..=30).contains(s)) {
            (1, Some(s), None)
        } else {
            return;
        };
        if pop >= n {
            return;
        }
        tokens.truncate(n - pop);
        if let Some(s) = season {
            key.season.get_or_insert(s);
        }
        if let Some(p) = part {
            key.part.get_or_insert(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loose(s: &str) -> String {
        TitleKey::new(s).loose
    }

    #[test]
    fn unicode_folding() {
        assert_eq!(fold("ＳＰＹ×ＦＡＭＩＬＹ"), "spy family");
        assert_eq!(fold("Amélie"), "amelie");
        assert_eq!(fold("Æon Flux"), "aeon flux");
        assert_eq!(fold("Ægir Straße"), "aegir strasse");
        assert_eq!(fold("Pokémon: The Movie 2000"), "pokemon the movie 2000");
        assert_eq!(fold("  Spider-Man:   No Way Home  "), "spider man no way home");
        assert_eq!(fold("Фильм «Брат-2»"), "фильм брат 2");
        assert_eq!(fold("Тайна"), "тайна");
        assert_eq!(fold("進撃の巨人 The Final Season"), "進撃の巨人 the final season");
        assert_eq!(fold("ガンダム"), "ガンダム");
        assert_eq!(fold("Gintama°"), "gintama");
    }

    #[test]
    fn numerals_and_articles() {
        assert_eq!(TitleKey::new("Rocky IV").plain, "rocky 4");
        assert_eq!(TitleKey::new("Rocky 4").plain, "rocky 4");
        assert_eq!(TitleKey::new("モブサイコ100 Ⅲ").plain, "モブサイコ100 3");
        assert_eq!(TitleKey::new("The Matrix").plain, "matrix");
        assert_eq!(TitleKey::new("Office, The").plain, "office");
        assert_eq!(TitleKey::new("A Silent Voice").plain, "silent voice");
        assert_eq!(TitleKey::new("The").plain, "the");
        assert_eq!(TitleKey::new("Up").plain, "up");
        assert_eq!(TitleKey::new("Civilization VI").plain, "civilization 6");
        // Not canonical roman numerals.
        assert_eq!(TitleKey::new("Vixx").plain, "vixx");
    }

    #[test]
    fn cyrillic_and_transliterations_meet() {
        let ru = loose("Ирония судьбы, или С лёгким паром!");
        assert_eq!(loose("Ironiya sudby"), loose("Ирония судьбы"));
        assert_eq!(loose("Ironija sud'by"), loose("Ирония судьбы"));
        assert_eq!(loose("Ironia sudbi"), loose("Ирония судьбы"));
        assert!(ru.starts_with(&loose("Ironiya sudby")));
        assert_eq!(loose("Zhmurki"), loose("Жмурки"));
        assert_eq!(loose("Khrustalev mashinu"), loose("Хрусталёв, машину!"));
        assert_eq!(loose("Ivan Vasilievich"), loose("Иван Васильевич"));
        assert_eq!(loose("Ivan Vasilyevich"), loose("Иван Васильевич"));
        assert_eq!(loose("Shchit"), loose("Щит"));
        assert_eq!(loose("Brat 2"), loose("Брат 2"));
        assert_eq!(loose("Brat II"), loose("Брат 2"));
        assert_eq!(loose("Voyna i mir"), loose("Война и мир"));
        assert_eq!(loose("Tolstoj"), loose("Толстой"));
        assert_eq!(loose("Tolstoi"), loose("Толстой"));
    }

    #[test]
    fn romaji_variants_meet() {
        assert_eq!(loose("Shingeki no Kyoujin"), loose("Shingeki no Kyojin"));
        assert_eq!(loose("Shingeki no Kyōjin"), loose("Shingeki no Kyojin"));
        assert_eq!(loose("Naruto: Shippuuden"), loose("Naruto Shippuden"));
        assert_eq!(loose("Shimbun"), loose("Shinbun"));
        assert_eq!(loose("Kimi wo Aishiteru"), loose("Kimi o Aishiteru"));
        assert_eq!(loose("Ookami to Koushinryou"), loose("Okami to Koshinryo"));
        assert_eq!(loose("Yuuki Yuuna"), loose("Yuki Yuna"));
    }

    #[test]
    fn qualifiers() {
        let k = TitleKey::new("Attack on Titan Season 3 Part 2");
        assert_eq!(k.base, loose("Attack on Titan"));
        assert_eq!((k.season, k.part), (Some(3), Some(2)));

        let k = TitleKey::new("Mob Psycho 100 3rd Season");
        assert_eq!((k.base.as_str(), k.season), ("mob psycho 100", Some(3)));
        assert_eq!(TitleKey::new("Fairy Tail (2014)").year, Some(2014));
        assert_eq!(TitleKey::new("Fairy Tail (2014)").base, "fairy tail");
        assert_eq!(TitleKey::new("Fairy Tail S2").season, Some(2));
        assert_eq!(TitleKey::new("FAIRY TAIL (第2期)").season, Some(2));
        assert_eq!(TitleKey::new("灵能百分百 第三季").season, Some(3));
        assert_eq!(TitleKey::new("Psycho-Pass 2").season, Some(2));
        assert_eq!(TitleKey::new("Psycho-Pass 2").base, loose("Psycho-Pass"));
        assert_eq!(TitleKey::new("Сваты 2 сезон").season, Some(2));
        assert_eq!(TitleKey::new("Сваты. Сезон 2").base, loose("Сваты"));
        assert_eq!(TitleKey::new("Ведьмак: Часть 2").part, Some(2));
        assert_eq!(TitleKey::new("Gintama 2nd Cour").part, Some(2));
        let k = TitleKey::new("Shingeki no Kyojin: The Final Season");
        assert_eq!(k.base, loose("Shingeki no Kyojin"));
        assert_eq!(k.season, None);
        // Numbers that are the title stay.
        assert_eq!(TitleKey::new("Mob Psycho 100").base, "mob psycho 100");
        assert_eq!(TitleKey::new("1917").base, "1917");
        assert_eq!(TitleKey::new("24").base, "24");
        assert_eq!(TitleKey::new("Blade Runner 2049").plain, "blade runner 2049");
    }
}
