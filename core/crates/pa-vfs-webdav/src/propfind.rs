// SPDX-License-Identifier: MPL-2.0
//! PROPFIND request body and `207 Multi-Status` parser (RFC 4918 §9.1, §14).

use std::time::UNIX_EPOCH;

use pa_vfs::{ErrorKind, Result, VfsError};
use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::{Namespace, ResolveResult};

/// Asks only for what [`Metadata`](pa_vfs::Metadata) needs: servers compute
/// some live properties (quota, etag) expensively, so `allprop` is avoided.
pub(crate) const BODY: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<D:propfind xmlns:D="DAV:"><D:prop>"#,
    "<D:resourcetype/><D:getcontentlength/><D:getlastmodified/>",
    "</D:prop></D:propfind>"
);

/// One `<D:response>` with the properties found (`200` propstats only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DavEntry {
    /// Raw href, XML entities resolved, percent-escapes kept.
    pub href: String,
    /// `<D:resourcetype><D:collection/>`.
    pub is_collection: bool,
    /// `getcontentlength`.
    pub size: Option<u64>,
    /// `getlastmodified` as Unix milliseconds.
    pub modified_ms: Option<i64>,
    /// The response carried a non-2xx status for the resource itself.
    pub failed: bool,
}

#[derive(Default)]
struct PropStat {
    is_collection: bool,
    size: Option<u64>,
    modified_ms: Option<i64>,
    ok: bool,
}

/// Parses a multistatus body. Elements outside the `DAV:` namespace are
/// ignored; elements without a namespace are accepted as `DAV:` (some
/// embedded servers omit the declaration).
pub(crate) fn parse(xml: &[u8]) -> Result<Vec<DavEntry>> {
    let text = std::str::from_utf8(xml)
        .map_err(|_| VfsError::new(ErrorKind::Other, "PROPFIND reply is not UTF-8"))?;
    let mut reader = NsReader::from_str(text);
    reader.config_mut().expand_empty_elements = true;

    let mut out = Vec::new();
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut entry: Option<DavEntry> = None;
    let mut propstat: Option<PropStat> = None;
    let mut buf = String::new();

    loop {
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|e| VfsError::new(ErrorKind::Other, format!("bad PROPFIND reply: {e}")))?;
        let dav = match ns {
            ResolveResult::Bound(Namespace(n)) => n == "DAV:",
            ResolveResult::Unbound => true,
            ResolveResult::Unknown(_) => false,
        };
        match event {
            Event::Start(e) => {
                let name = dav.then(|| e.local_name().as_ref().to_owned());
                match name.as_deref() {
                    Some("response") => entry = Some(DavEntry::default()),
                    Some("propstat") => propstat = Some(PropStat::default()),
                    Some("collection") if in_elem(&stack, "resourcetype") => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.is_collection = true;
                        }
                    }
                    _ => {}
                }
                stack.push(name);
                buf.clear();
            }
            Event::Text(t) => buf.push_str(&t.xml10_content()),
            Event::CData(t) => buf.push_str(&t),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    buf.push(c);
                } else {
                    buf.push_str(match &*r {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let name = stack.pop().flatten();
                let value = std::mem::take(&mut buf);
                let value = value.trim();
                match name.as_deref() {
                    Some("href") if in_elem(&stack, "response") => {
                        if let Some(en) = entry.as_mut() {
                            en.href = value.to_owned();
                        }
                    }
                    Some("status") if in_elem(&stack, "propstat") => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.ok = status_is_success(value);
                        }
                    }
                    Some("status") if in_elem(&stack, "response") => {
                        if let Some(en) = entry.as_mut() {
                            en.failed = !status_is_success(value);
                        }
                    }
                    Some("getcontentlength") => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.size = value.parse().ok();
                        }
                    }
                    Some("getlastmodified") => {
                        if let Some(ps) = propstat.as_mut() {
                            ps.modified_ms = parse_http_date(value);
                        }
                    }
                    Some("propstat") => {
                        if let (Some(ps), Some(en)) = (propstat.take(), entry.as_mut())
                            && ps.ok
                        {
                            en.is_collection |= ps.is_collection;
                            en.size = en.size.or(ps.size);
                            en.modified_ms = en.modified_ms.or(ps.modified_ms);
                        }
                    }
                    Some("response") => {
                        if let Some(en) = entry.take()
                            && !en.href.is_empty()
                        {
                            out.push(en);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn in_elem(stack: &[Option<String>], name: &str) -> bool {
    stack.last().and_then(Option::as_deref) == Some(name)
}

/// `HTTP/1.1 200 OK` → true for any 2xx.
fn status_is_success(line: &str) -> bool {
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .is_some_and(|c| (200..300).contains(&c))
}

/// RFC 1123 / RFC 850 / asctime dates (`getlastmodified`) to Unix ms.
fn parse_http_date(s: &str) -> Option<i64> {
    let t = httpdate::parse_http_date(s).ok()?;
    let d = t.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apache_style_reply() {
        let xml = br#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:ns0="DAV:">
<D:response xmlns:lp1="DAV:" xmlns:lp2="http://apache.org/dav/props/">
<D:href>/dav/</D:href>
<D:propstat><D:prop><lp1:resourcetype><D:collection/></lp1:resourcetype>
<lp1:getlastmodified>Sat, 14 Nov 2026 10:00:00 GMT</lp1:getlastmodified></D:prop>
<D:status>HTTP/1.1 200 OK</D:status></D:propstat>
<D:propstat><D:prop><lp1:getcontentlength/></D:prop>
<D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
</D:response>
<D:response><D:href>/dav/special%20%231%20&amp;%20x.txt</D:href>
<D:propstat><D:prop><lp1:resourcetype xmlns:lp1="DAV:"/>
<lp1:getcontentlength xmlns:lp1="DAV:">8</lp1:getcontentlength></D:prop>
<D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>"#;
        let e = parse(xml).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].href, "/dav/");
        assert!(e[0].is_collection);
        assert_eq!(e[0].size, None);
        assert_eq!(e[0].modified_ms, Some(1_794_650_400_000));
        assert_eq!(e[1].href, "/dav/special%20%231%20&%20x.txt");
        assert!(!e[1].is_collection);
        assert_eq!(e[1].size, Some(8));
    }

    #[test]
    fn default_namespace_and_lowercase_prefix() {
        let xml = br#"<multistatus xmlns="DAV:"><response>
<href>http://h/a/&#1044;</href><propstat><prop>
<resourcetype><collection></collection></resourcetype></prop>
<status>HTTP/1.1 200 OK</status></propstat></response>
<d:response xmlns:d="DAV:"><d:href>/a/b</d:href><d:status>HTTP/1.1 404 Not Found</d:status></d:response>
</multistatus>"#;
        let e = parse(xml).unwrap();
        assert_eq!(e[0].href, "http://h/a/Д");
        assert!(e[0].is_collection);
        assert!(e[1].failed);
    }

    #[test]
    fn foreign_collection_is_not_dav_collection() {
        let xml = br#"<D:multistatus xmlns:D="DAV:" xmlns:X="urn:x"><D:response>
<D:href>/f</D:href><D:propstat><D:prop><D:resourcetype><X:collection/></D:resourcetype>
<D:getcontentlength>12</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status>
</D:propstat></D:response></D:multistatus>"#;
        let e = parse(xml).unwrap();
        assert!(!e[0].is_collection);
        assert_eq!(e[0].size, Some(12));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(&[0xff, 0xfe]).is_err());
    }
}
