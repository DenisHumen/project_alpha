// SPDX-License-Identifier: MPL-2.0
//! Size variants through provider size URLs.
//!
//! TMDB serves every image in a fixed set of widths
//! (`https://image.tmdb.org/t/p/w342/<file>`), so a grid of posters does not
//! download the `w500` the catalog stores (`pa-catalog`, META-027): the cache
//! asks for the smallest provider width that covers the pixels on screen.
//! It never asks for more than the stored width, which is what the metadata
//! layer chose as the largest useful size. Other URLs (AniList, Shikimori,
//! unknown hosts) are fetched as they are; the app decodes them at display
//! size.

/// Image base of TMDB, with the trailing slash.
pub const TMDB_IMAGE_PREFIX: &str = "https://image.tmdb.org/t/p/";

/// Poster widths TMDB serves (`/configuration`, `poster_sizes`).
const POSTER: &[u32] = &[92, 154, 185, 342, 500, 780];
/// Backdrop widths (`backdrop_sizes`).
const BACKDROP: &[u32] = &[300, 780, 1280];

/// A parsed TMDB image URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TmdbUrl<'a> {
    /// `wNNN` width; `None` for `original` or a size we do not know.
    width: Option<u32>,
    /// `/abc.jpg`.
    file: &'a str,
}

fn parse_tmdb(url: &str) -> Option<TmdbUrl<'_>> {
    let rest = url.strip_prefix(TMDB_IMAGE_PREFIX)?;
    let (size, file) = rest.split_at(rest.find('/')?);
    if file.len() < 2 || file[1..].contains('/') {
        return None;
    }
    let width = size.strip_prefix('w').and_then(|w| w.parse().ok());
    Some(TmdbUrl { width, file })
}

/// Widths we may ask for instead of `stored`, ascending, all `<= stored`.
///
/// The widths of a size family are only valid for that kind of image, so the
/// family is told by the stored width: `w1280` is a backdrop, the poster
/// widths are posters. `w300` is both a backdrop and an episode still width
/// and stays as it is, as do `original` and unknown widths.
fn family(stored: Option<u32>) -> &'static [u32] {
    match stored {
        Some(1280) => BACKDROP,
        Some(300) => &[300],
        Some(w) if POSTER.contains(&w) => {
            let n = POSTER.iter().position(|&p| p == w).unwrap_or(0);
            &POSTER[..=n]
        }
        _ => &[],
    }
}

fn tmdb_url(width: u32, file: &str) -> String {
    format!("{TMDB_IMAGE_PREFIX}w{width}{file}")
}

/// URL of the variant to download for `width` physical pixels on screen.
///
/// TMDB URLs get the smallest provider width `>= width` that the stored
/// width allows (the stored one when none is large enough); every other URL,
/// and any URL without a `width`, comes back unchanged.
pub fn variant_url(url: &str, width: Option<u32>) -> String {
    let (Some(want), Some(t)) = (width, parse_tmdb(url)) else {
        return url.to_owned();
    };
    let widths = family(t.width);
    match widths.iter().find(|&&w| w >= want).or(widths.last()) {
        Some(&w) => tmdb_url(w, t.file),
        None => url.to_owned(),
    }
}

/// Every variant of `url` the cache may hold, largest first; `url` itself
/// first. Used offline: any cached size beats a placeholder.
pub fn variants(url: &str) -> Vec<String> {
    let mut out = vec![url.to_owned()];
    if let Some(t) = parse_tmdb(url) {
        for &w in family(t.width).iter().rev() {
            let v = tmdb_url(w, t.file);
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSTER_500: &str = "https://image.tmdb.org/t/p/w500/p.jpg";
    const BACKDROP_1280: &str = "https://image.tmdb.org/t/p/w1280/b.jpg";
    const STILL_300: &str = "https://image.tmdb.org/t/p/w300/s.jpg";

    #[test]
    fn poster_picks_smallest_covering_width() {
        assert_eq!(
            variant_url(POSTER_500, Some(300)),
            "https://image.tmdb.org/t/p/w342/p.jpg"
        );
        assert_eq!(
            variant_url(POSTER_500, Some(1)),
            "https://image.tmdb.org/t/p/w92/p.jpg"
        );
        assert_eq!(
            variant_url(POSTER_500, Some(342)),
            "https://image.tmdb.org/t/p/w342/p.jpg"
        );
    }

    #[test]
    fn never_larger_than_stored() {
        assert_eq!(variant_url(POSTER_500, Some(2000)), POSTER_500);
        assert_eq!(variant_url(BACKDROP_1280, Some(4000)), BACKDROP_1280);
    }

    #[test]
    fn backdrop_uses_backdrop_widths() {
        assert_eq!(
            variant_url(BACKDROP_1280, Some(200)),
            "https://image.tmdb.org/t/p/w300/b.jpg"
        );
        assert_eq!(
            variant_url(BACKDROP_1280, Some(700)),
            "https://image.tmdb.org/t/p/w780/b.jpg"
        );
    }

    #[test]
    fn ambiguous_original_and_foreign_urls_are_unchanged() {
        assert_eq!(variant_url(STILL_300, Some(50)), STILL_300);
        let orig = "https://image.tmdb.org/t/p/original/o.jpg";
        assert_eq!(variant_url(orig, Some(100)), orig);
        let anilist = "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/x.jpg";
        assert_eq!(variant_url(anilist, Some(100)), anilist);
        assert_eq!(variant_url(POSTER_500, None), POSTER_500);
        let nested = "https://image.tmdb.org/t/p/w500/a/b.jpg";
        assert_eq!(variant_url(nested, Some(100)), nested);
    }

    #[test]
    fn variants_list_all_sizes_largest_first() {
        let v = variants(POSTER_500);
        assert_eq!(v[0], POSTER_500);
        assert_eq!(v.last().unwrap(), "https://image.tmdb.org/t/p/w92/p.jpg");
        assert_eq!(v.len(), 5);
        assert_eq!(variants("https://example.org/x.png").len(), 1);
    }
}
