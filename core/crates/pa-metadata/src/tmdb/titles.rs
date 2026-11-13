// SPDX-License-Identifier: MPL-2.0
//! Cached TMDB responses as records of the local title index
//! ([`pa_recognition::title`], META-007): a search result plus its
//! translations and alternative titles, so works seen once are found again
//! offline, in any language the cache holds.

use pa_recognition::title::{TitleRecord, WorkKind};

use super::{AlternativeTitles, MediaKind, SearchResult, Translations};

/// A title record for a movie or series from TMDB data the client already
/// has. Returns `None` for people and unknown result types.
///
/// ```
/// use pa_metadata::tmdb::{SearchResult, titles::title_record};
///
/// let r: SearchResult = serde_json::from_str(
///     r#"{"id":603,"media_type":"movie","title":"Матрица","original_title":"The Matrix","release_date":"1999-03-31"}"#,
/// ).unwrap();
/// let rec = title_record(&r, None, None).unwrap();
/// assert_eq!(rec.ids["tmdb"], "movie:603");
/// assert_eq!(rec.titles, ["The Matrix", "Матрица"]);
/// assert_eq!(rec.year, Some(1999));
/// ```
pub fn title_record(
    result: &SearchResult,
    translations: Option<&Translations>,
    alternative: Option<&AlternativeTitles>,
) -> Option<TitleRecord> {
    let kind = result.kind()?;
    let mut titles: Vec<String> = Vec::new();
    let mut push = |t: &str| {
        let t = t.trim();
        if !t.is_empty() && !titles.iter().any(|x| x == t) {
            titles.push(t.to_owned());
        }
    };
    for t in [&result.original_title, &result.title]
        .into_iter()
        .flatten()
    {
        push(t);
    }
    for t in translations
        .into_iter()
        .flat_map(|t| &t.translations)
        .filter_map(|t| t.data.title.as_deref())
    {
        push(t);
    }
    for t in alternative.into_iter().flat_map(|a| &a.titles) {
        push(&t.title);
    }
    if titles.is_empty() {
        return None;
    }
    let mut record = TitleRecord {
        kind: match kind {
            MediaKind::Movie => WorkKind::Movie,
            MediaKind::Tv => WorkKind::Series,
        },
        year: result.year().and_then(|y| u16::try_from(y).ok()),
        titles,
        ..Default::default()
    };
    record
        .ids
        .insert("tmdb".into(), format!("{}:{}", kind.as_str(), result.id));
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_translations_and_alternative_titles() {
        let r: SearchResult = serde_json::from_str(
            r#"{"id":1,"media_type":"tv","name":"Сваты","original_name":"Свати","first_air_date":"2008-12-25"}"#,
        )
        .unwrap();
        let tr: Translations = serde_json::from_str(
            r#"{"id":1,"translations":[{"iso_3166_1":"US","iso_639_1":"en","data":{"name":"Matchmakers"}},{"iso_3166_1":"RU","iso_639_1":"ru","data":{"name":"Сваты"}}]}"#,
        )
        .unwrap();
        let alt: AlternativeTitles = serde_json::from_str(
            r#"{"id":1,"results":[{"iso_3166_1":"RU","title":"Svaty","type":""}]}"#,
        )
        .unwrap();
        let rec = title_record(&r, Some(&tr), Some(&alt)).unwrap();
        assert_eq!(rec.titles, ["Свати", "Сваты", "Matchmakers", "Svaty"]);
        assert_eq!(rec.kind, WorkKind::Series);
        assert_eq!(rec.ids["tmdb"], "tv:1");

        let person: SearchResult =
            serde_json::from_str(r#"{"id":2,"media_type":"person","name":"X"}"#).unwrap();
        assert!(title_record(&person, None, None).is_none());
    }
}
