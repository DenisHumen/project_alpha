// SPDX-License-Identifier: MPL-2.0
//! Updater tests with a mocked HTTP client (no network).

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

use pa_anime_mappings::update::{HttpClient, HttpResponse, Outcome, Source, Updater, sha256_hex};

const XML: &str = include_str!("fixtures/anime-list-master.xml");
const DAY: u64 = 24 * 60 * 60;

/// Serves queued responses and records requests.
#[derive(Default)]
struct Mock {
    responses: RefCell<Vec<HttpResponse>>,
    requests: RefCell<Vec<(String, Option<String>)>>,
}

impl Mock {
    fn push(&self, status: u16, etag: Option<&str>, body: &[u8]) {
        self.responses.borrow_mut().push(HttpResponse {
            status,
            etag: etag.map(str::to_owned),
            body: body.to_vec(),
        });
    }
}

impl HttpClient for Mock {
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<HttpResponse, String> {
        self.requests
            .borrow_mut()
            .push((url.to_owned(), if_none_match.map(str::to_owned)));
        let mut r = self.responses.borrow_mut();
        if r.is_empty() {
            return Err("offline".into());
        }
        Ok(r.remove(0))
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("pa-anime-mappings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn updater(dir: &TempDir) -> Updater {
    let mut u = Updater::new(&dir.0);
    u.min_entries_override = Some(1);
    u
}

fn data_files(dir: &TempDir) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != "manifest.json")
        .collect();
    v.sort();
    v
}

#[test]
fn download_cache_etag_and_atomic_replace() {
    let dir = TempDir::new("flow");
    let u = updater(&dir);
    let http = Mock::default();
    let src = Source::AnimeLists;

    assert_eq!(u.load(src).unwrap(), None);

    http.push(200, Some("\"v1\""), XML.as_bytes());
    assert_eq!(
        u.refresh(&http, src, 1_000, false).unwrap(),
        Outcome::Updated { entries: 6 }
    );
    assert_eq!(u.load(src).unwrap().unwrap(), XML.as_bytes());
    let st = u.status(src).unwrap().unwrap();
    assert_eq!(st.sha256, sha256_hex(XML.as_bytes()));
    assert_eq!(st.url, src.spec().url);

    // Within 24 h: no request at all.
    assert_eq!(
        u.refresh(&http, src, 1_000 + DAY - 1, false).unwrap(),
        Outcome::Skipped
    );
    assert_eq!(http.requests.borrow().len(), 1);

    // After 24 h: conditional request, 304.
    http.push(304, None, b"");
    assert_eq!(
        u.refresh(&http, src, 1_000 + DAY, false).unwrap(),
        Outcome::NotModified
    );
    assert_eq!(http.requests.borrow()[1].1.as_deref(), Some("\"v1\""));
    assert_eq!(u.status(src).unwrap().unwrap().checked_at, 1_000 + DAY);

    // Broken download: rejected, old version kept.
    http.push(200, Some("\"v2\""), b"<anime-list><anime tvdbid=");
    assert!(u.refresh(&http, src, 5 * DAY, true).is_err());
    assert_eq!(u.load(src).unwrap().unwrap(), XML.as_bytes());

    // Network error: old version kept.
    assert!(u.refresh(&http, src, 5 * DAY, true).is_err());
    assert_eq!(u.load(src).unwrap().unwrap(), XML.as_bytes());

    // New version replaces the old file; only one data file remains.
    let v2 = XML.replace("Example Saga</name>", "Example Saga (v2)</name>");
    http.push(200, Some("\"v2\""), v2.as_bytes());
    assert_eq!(
        u.refresh(&http, src, 6 * DAY, true).unwrap(),
        Outcome::Updated { entries: 6 }
    );
    assert_eq!(u.load(src).unwrap().unwrap(), v2.as_bytes());
    let files = data_files(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].starts_with("anime-lists.") && files[0].ends_with(".xml"));
}

#[test]
fn corrupted_cache_is_detected_and_refetched_without_etag() {
    let dir = TempDir::new("corrupt");
    let u = updater(&dir);
    let http = Mock::default();
    let src = Source::AnimeLists;
    http.push(200, Some("\"v1\""), XML.as_bytes());
    u.refresh(&http, src, 0, false).unwrap();

    let file = u.status(src).unwrap().unwrap().file;
    fs::write(dir.0.join(file), b"tampered").unwrap();
    assert!(u.load(src).is_err());

    // Not skipped despite the interval, and no ETag sent.
    http.push(200, Some("\"v1\""), XML.as_bytes());
    assert_eq!(
        u.refresh(&http, src, 10, false).unwrap(),
        Outcome::Updated { entries: 6 }
    );
    assert_eq!(http.requests.borrow()[1].1, None);
    assert_eq!(u.load(src).unwrap().unwrap(), XML.as_bytes());
}

#[test]
fn rejects_too_small_and_shrunken_lists() {
    let dir = TempDir::new("small");
    let mut u = Updater::new(&dir.0);
    let http = Mock::default();
    // Default minimum for Anime-Lists is thousands of entries.
    http.push(200, None, XML.as_bytes());
    assert!(u.refresh(&http, Source::AnimeLists, 0, false).is_err());
    assert_eq!(u.load(Source::AnimeLists).unwrap(), None);

    u.min_entries_override = Some(1);
    http.push(200, None, XML.as_bytes());
    u.refresh(&http, Source::AnimeLists, 0, false).unwrap();
    let tiny = "<anime-list><anime anidbid=\"1\" tvdbid=\"2\"/></anime-list>";
    http.push(200, None, tiny.as_bytes());
    assert!(u.refresh(&http, Source::AnimeLists, 0, true).is_err());
    assert_eq!(u.load(Source::AnimeLists).unwrap().unwrap(), XML.as_bytes());

    http.push(500, None, b"");
    assert!(u.refresh(&http, Source::AnimeLists, 0, true).is_err());
}

#[test]
fn unlicensed_sources_come_from_upstream_only() {
    for src in Source::ALL {
        let spec = src.spec();
        assert!(spec.url.starts_with("https://"), "{}", spec.url);
        assert!(
            !spec.url.to_lowercase().contains("project_alpha"),
            "{}",
            spec.url
        );
        assert!(
            !spec.url.to_lowercase().contains("denishumen"),
            "{}",
            spec.url
        );
    }
    let al = Source::AnimeLists.spec();
    assert!(!al.redistributable);
    assert!(
        al.url
            .starts_with("https://raw.githubusercontent.com/Anime-Lists/anime-lists/")
    );
    let fr = Source::Fribb.spec();
    assert!(!fr.redistributable);
    assert!(
        fr.url
            .starts_with("https://raw.githubusercontent.com/Fribb/anime-lists/")
    );
    let od = Source::OfflineDatabase.spec();
    assert!(od.redistributable);
    assert!(
        od.url
            .starts_with("https://github.com/cedya77/anime-offline-database/")
    );
}
