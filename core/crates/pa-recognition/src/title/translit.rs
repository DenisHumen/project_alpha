// SPDX-License-Identifier: MPL-2.0
//! Russian Cyrillic ↔ Latin transliteration.
//!
//! Two directions, used differently:
//!
//! - [`to_latin`] with a [`Scheme`] produces a readable transliteration
//!   (GOST 7.79-2000 system B, or the everyday scheme seen in release names:
//!   `Ironiya sudby`). It is meant for display and for building provider
//!   queries.
//! - [`to_cyrillic`] guesses Cyrillic from an everyday transliteration
//!   (`Ironiya sudby` → `Ирония судбы`). It is lossy by nature (`y` can be
//!   `й` or `ы`, soft signs are usually dropped), so it only generates query
//!   variants and is never used as a match key.
//!
//! Matching does not depend on either: the title index maps both scripts
//! into one loose Latin space ([`super::normalize::loose_token`]) where all
//! common spellings of a letter collapse, so `Ирония судьбы`, `Ironiya
//! sudby`, `Ironija sud'by` and `Ironia sudbi` meet regardless of which
//! scheme a releaser used.

/// Cyrillic → Latin scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    /// GOST 7.79-2000 system B (`ц` → `cz`/`c`, `х` → `x`, `щ` → `shh`,
    /// `ы` → ``y` ``, `э` → ``e` ``, `ь` → `` ` ``).
    Gost,
    /// The everyday scheme of release names and passports before 2013:
    /// `я` → `ya`, `ю` → `yu`, `й` → `y`, `х` → `kh`, `ц` → `ts`,
    /// `щ` → `shch`, `ь`/`ъ` dropped.
    #[default]
    Common,
}

/// Latin for one lower-case Cyrillic letter, or `None` for other characters.
/// `next` is the following character (GOST writes `ц` as `c` before
/// `i e y j`).
fn letter(c: char, next: Option<char>, scheme: Scheme) -> Option<&'static str> {
    let gost = scheme == Scheme::Gost;
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' => "e",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' if gost => "j",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' if gost => "x",
        'х' => "kh",
        'ц' if gost => {
            if matches!(next, Some('и' | 'е' | 'ы' | 'й')) {
                "c"
            } else {
                "cz"
            }
        }
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' if gost => "shh",
        'щ' => "shch",
        'ъ' if gost => "``",
        'ъ' => "",
        'ы' if gost => "y`",
        'ы' => "y",
        'ь' if gost => "`",
        'ь' => "",
        'э' if gost => "e`",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        // Ukrainian and Belarusian letters that appear in Russian releases.
        'і' => "i",
        'ї' => "yi",
        'є' => "ye",
        'ґ' => "g",
        'ў' => "u",
        _ => return None,
    })
}

/// Transliterates Russian Cyrillic to Latin; other characters pass through.
/// Upper-case letters keep their case on the first Latin letter (`Щ` →
/// `Shch`), or on all of them when the next letter is upper-case too.
///
/// ```
/// use pa_recognition::title::translit::{to_latin, Scheme};
/// assert_eq!(to_latin("Ирония судьбы", Scheme::Common), "Ironiya sudby");
/// assert_eq!(to_latin("Ирония судьбы", Scheme::Gost), "Ironiya sud`by`");
/// assert_eq!(to_latin("Щука", Scheme::Common), "Shchuka");
/// ```
pub fn to_latin(s: &str, scheme: Scheme) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let next = chars
            .get(i + 1)
            .map(|n| n.to_lowercase().next().unwrap_or(*n));
        match letter(lower, next, scheme) {
            Some(lat) if lower != c => {
                let all_caps = chars.get(i + 1).is_some_and(|n| n.is_uppercase());
                for (j, l) in lat.chars().enumerate() {
                    if all_caps || j == 0 {
                        out.extend(l.to_uppercase());
                    } else {
                        out.push(l);
                    }
                }
            }
            Some(lat) => out.push_str(lat),
            None => out.push(c),
        }
    }
    out
}

/// Latin → Cyrillic for everyday transliterations (greedy, longest match).
///
/// ```
/// use pa_recognition::title::translit::to_cyrillic;
/// assert_eq!(to_cyrillic("Ironiya sudby"), "Ирония судбы");
/// assert_eq!(to_cyrillic("Brat 2"), "Брат 2");
/// assert_eq!(to_cyrillic("Shchuka"), "Щука");
/// ```
pub fn to_cyrillic(s: &str) -> String {
    // Longest patterns first.
    const TABLE: &[(&str, &str)] = &[
        ("shch", "щ"),
        ("sch", "щ"),
        ("zh", "ж"),
        ("kh", "х"),
        ("ts", "ц"),
        ("tz", "ц"),
        ("ch", "ч"),
        ("sh", "ш"),
        ("yu", "ю"),
        ("ju", "ю"),
        ("ya", "я"),
        ("ja", "я"),
        ("yo", "ё"),
        ("jo", "ё"),
        ("ye", "е"),
        ("a", "а"),
        ("b", "б"),
        ("v", "в"),
        ("w", "в"),
        ("g", "г"),
        ("d", "д"),
        ("e", "е"),
        ("z", "з"),
        ("i", "и"),
        ("j", "й"),
        ("k", "к"),
        ("q", "к"),
        ("l", "л"),
        ("m", "м"),
        ("n", "н"),
        ("o", "о"),
        ("p", "п"),
        ("r", "р"),
        ("s", "с"),
        ("t", "т"),
        ("u", "у"),
        ("f", "ф"),
        ("h", "х"),
        ("c", "к"),
        ("x", "кс"),
        ("'", "ь"),
    ];
    let chars: Vec<char> = s.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let is_letter = |i: usize| lower.get(i).is_some_and(|c| c.is_ascii_alphabetic());
    let mut out = String::with_capacity(s.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        let c = lower[i];
        if !(c.is_ascii_alphabetic() || c == '\'') {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let word_start = i == 0 || !is_letter(i - 1);
        // `y`: `й` after a vowel, `ы` after a consonant; `e` at a word start is `э`.
        let special = match c {
            'y' if !matches!(lower.get(i + 1), Some('a' | 'u' | 'o' | 'e')) => {
                let after_vowel = i > 0 && matches!(lower[i - 1], 'a' | 'e' | 'i' | 'o' | 'u');
                Some((1, if after_vowel { "й" } else { "ы" }))
            }
            'e' if word_start => Some((1, "э")),
            _ => None,
        };
        let (len, cyr) = special.unwrap_or_else(|| {
            TABLE
                .iter()
                .find(|(lat, _)| {
                    lat.chars()
                        .enumerate()
                        .all(|(k, l)| lower.get(i + k) == Some(&l))
                })
                .map(|(lat, cyr)| (lat.chars().count(), *cyr))
                .unwrap_or((1, ""))
        });
        if chars[i].is_ascii_uppercase() {
            let mut it = cyr.chars();
            if let Some(first) = it.next() {
                out.extend(first.to_uppercase());
                out.extend(it);
            }
        } else {
            out.push_str(cyr);
        }
        i += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_scheme() {
        assert_eq!(to_latin("Брат 2", Scheme::Common), "Brat 2");
        assert_eq!(to_latin("Москва слезам не верит", Scheme::Common), "Moskva slezam ne verit");
        assert_eq!(to_latin("Ёлки", Scheme::Common), "Yolki");
        assert_eq!(to_latin("Жмурки", Scheme::Common), "Zhmurki");
        assert_eq!(to_latin("Солярис", Scheme::Common), "Solyaris");
        assert_eq!(to_latin("Цой", Scheme::Common), "Tsoy");
        assert_eq!(to_latin("СССР", Scheme::Common), "SSSR");
    }

    #[test]
    fn gost_scheme() {
        assert_eq!(to_latin("Цирк", Scheme::Gost), "Cirk");
        assert_eq!(to_latin("Царь", Scheme::Gost), "Czar`");
        assert_eq!(to_latin("Хрусталёв", Scheme::Gost), "Xrustalyov");
        assert_eq!(to_latin("Щит", Scheme::Gost), "Shhit");
        assert_eq!(to_latin("Толстой", Scheme::Gost), "Tolstoj");
    }

    #[test]
    fn other_scripts_pass_through() {
        assert_eq!(to_latin("Shrek 2 / Шрек", Scheme::Common), "Shrek 2 / Shrek");
        assert_eq!(to_latin("進撃の巨人", Scheme::Common), "進撃の巨人");
    }

    #[test]
    fn back_to_cyrillic() {
        assert_eq!(to_cyrillic("Zhmurki"), "Жмурки");
        assert_eq!(to_cyrillic("Moskva slezam ne verit"), "Москва слезам не верит");
        assert_eq!(to_cyrillic("Ekipazh"), "Экипаж");
        assert_eq!(to_cyrillic("Bolshoy"), "Болшой");
        assert_eq!(to_cyrillic("Dostoevskiy"), "Достоевский");
        assert_eq!(to_cyrillic("Solyaris (1972)"), "Солярис (1972)");
    }
}
