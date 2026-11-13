// SPDX-License-Identifier: MPL-2.0
//! A tiny in-process WebDAV server for tests: PROPFIND (Depth 0/1), GET and
//! HEAD with `Range`, Basic or Digest auth, redirects, optional TLS. It
//! serves a local directory under `/dav/`. One request per connection.
//!
//! It deliberately differs from the client in details real servers differ
//! in: hrefs keep Cyrillic and `&` unescaped (XML-escaped as `&amp;`), hex
//! escapes are lower case, directories get a trailing slash.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use md5::{Digest, Md5};

pub const USER: &str = "kinolune";
pub const PASS: &str = "contract-test";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    None,
    Basic,
    /// Digest MD5, qop=auth; the nonce rotates every `rotate` requests and
    /// the server answers `stale=true`.
    Digest { rotate: usize },
}

#[derive(Clone, Default)]
pub struct Options {
    pub auth: Auth,
    pub tls: Option<Arc<rustls::ServerConfig>>,
    /// Answer `200` with the whole body to ranged GETs.
    pub ignore_range: bool,
    /// PROPFIND on a collection without a trailing slash → `301` to the
    /// slash form (as nginx does).
    pub slash_redirect: bool,
    /// Requests under `/old/` → `308` to `/dav/`.
    pub moved: bool,
    /// Do not report `getcontentlength` (forces the HEAD fallback).
    pub no_length: bool,
}

pub struct MockServer {
    /// Root URL of the share (`…/dav/`).
    pub url: String,
    /// Origin (`http://127.0.0.1:port`).
    pub origin: String,
    pub requests: Arc<AtomicUsize>,
    /// `Range` header values of GETs, in order.
    pub ranges: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    addr: std::net::SocketAddr,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

struct State {
    root: PathBuf,
    opts: Options,
    nonce_gen: AtomicUsize,
    authed: AtomicUsize,
    requests: Arc<AtomicUsize>,
    ranges: Arc<Mutex<Vec<String>>>,
}

impl MockServer {
    pub fn start(root: &Path, opts: Options) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let scheme = if opts.tls.is_some() { "https" } else { "http" };
        let host = if opts.tls.is_some() { "localhost" } else { "127.0.0.1" };
        let origin = format!("{scheme}://{host}:{}", addr.port());
        let requests = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(State {
            root: root.to_path_buf(),
            opts,
            nonce_gen: AtomicUsize::new(1),
            authed: AtomicUsize::new(0),
            requests: Arc::clone(&requests),
            ranges: Arc::clone(&ranges),
        });
        let stop2 = Arc::clone(&stop);
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let _ = conn.set_read_timeout(Some(Duration::from_secs(10)));
                    if let Some(cfg) = state.opts.tls.clone() {
                        let Ok(tls) = rustls::ServerConnection::new(cfg) else { return };
                        let mut s = rustls::StreamOwned::new(tls, conn);
                        handle(&state, &mut s);
                        s.conn.send_close_notify();
                        let _ = s.flush();
                    } else {
                        let mut s = conn;
                        handle(&state, &mut s);
                    }
                });
            }
        });
        Self { url: format!("{origin}/dav/"), origin, requests, ranges, stop, addr }
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(s: &mut dyn Read) -> Option<Request> {
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.push((k.trim().to_owned(), v.trim().to_owned()));
    }
    let req = Request { method, target, headers };
    if let Some(n) = req.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
        std::io::copy(&mut r.take(n), &mut std::io::sink()).ok()?;
    }
    Some(req)
}

fn respond(s: &mut dyn Write, status: &str, headers: &[(&str, String)], body: &[u8], head: bool) {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let _ = s.write_all(out.as_bytes());
    if !head {
        let _ = s.write_all(body);
    }
    let _ = s.flush();
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn handle(st: &State, s: &mut dyn Stream) {
    let Some(req) = read_request(s) else { return };
    st.requests.fetch_add(1, Ordering::SeqCst);
    let head = req.method == "HEAD";

    if !authorized(st, &req, s) {
        return;
    }
    let path = req.target.split('?').next().unwrap_or("").to_owned();
    if st.opts.moved
        && let Some(rest) = path.strip_prefix("/old/")
    {
        respond(s, "308 Permanent Redirect", &[("Location", format!("/dav/{rest}"))], b"", head);
        return;
    }
    let Some(rest) = path.strip_prefix("/dav/").or(if path == "/dav" { Some("") } else { None }) else {
        respond(s, "404 Not Found", &[], b"", head);
        return;
    };
    let segs: Vec<String> = rest
        .split('/')
        .filter(|x| !x.is_empty())
        .map(|x| decode(x))
        .collect();
    let fs_path = segs.iter().fold(st.root.clone(), |p, c| p.join(c));
    let Ok(meta) = std::fs::metadata(&fs_path) else {
        respond(s, "404 Not Found", &[], b"", head);
        return;
    };
    match req.method.as_str() {
        "PROPFIND" => {
            if meta.is_dir() && !path.ends_with('/') && st.opts.slash_redirect {
                respond(s, "301 Moved Permanently", &[("Location", format!("{path}/"))], b"", false);
                return;
            }
            let mut xml = String::from(r#"<?xml version="1.0" encoding="utf-8"?><D:multistatus xmlns:D="DAV:">"#);
            push_entry(&mut xml, st, &segs, &fs_path);
            if meta.is_dir() && req.header("depth") != Some("0") {
                for e in std::fs::read_dir(&fs_path).expect("read_dir").flatten() {
                    let mut child = segs.clone();
                    child.push(e.file_name().to_string_lossy().into_owned());
                    push_entry(&mut xml, st, &child, &e.path());
                }
            }
            xml.push_str("</D:multistatus>");
            let ct = ("Content-Type", "application/xml; charset=utf-8".to_owned());
            respond(s, "207 Multi-Status", &[ct], xml.as_bytes(), false);
        }
        "GET" | "HEAD" if meta.is_file() => {
            let data = std::fs::read(&fs_path).expect("read");
            let range = req.header("range").map(str::to_owned);
            if let Some(r) = &range {
                st.ranges.lock().unwrap().push(r.clone());
            }
            match range.as_deref().and_then(parse_range).filter(|_| !st.opts.ignore_range) {
                Some((a, _)) if a >= data.len() as u64 => {
                    respond(s, "416 Range Not Satisfiable", &[], b"", head);
                }
                Some((a, b)) => {
                    let end = b.min(data.len() as u64 - 1);
                    let body = &data[a as usize..=end as usize];
                    let cr = ("Content-Range", format!("bytes {a}-{end}/{}", data.len()));
                    respond(s, "206 Partial Content", &[cr], body, head);
                }
                None => respond(s, "200 OK", &[], &data, head),
            }
        }
        "GET" | "HEAD" => respond(s, "405 Method Not Allowed", &[], b"", head),
        _ => respond(s, "405 Method Not Allowed", &[], b"", head),
    }
}

fn push_entry(xml: &mut String, st: &State, segs: &[String], fs_path: &Path) {
    let meta = std::fs::metadata(fs_path).expect("metadata");
    let mut href = String::from("/dav/");
    href.push_str(&segs.iter().map(|s| encode_loose(s)).collect::<Vec<_>>().join("/"));
    if meta.is_dir() && !segs.is_empty() {
        href.push('/');
    }
    let modified = httpdate::fmt_http_date(meta.modified().unwrap_or(SystemTime::now()));
    xml.push_str(&format!("<D:response><D:href>{}</D:href><D:propstat><D:prop>", escape(&href)));
    if meta.is_dir() {
        xml.push_str("<D:resourcetype><D:collection/></D:resourcetype>");
    } else {
        xml.push_str("<D:resourcetype/>");
        if !st.opts.no_length {
            xml.push_str(&format!("<D:getcontentlength>{}</D:getcontentlength>", meta.len()));
        }
    }
    xml.push_str(&format!(
        "<D:getlastmodified>{modified}</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>"
    ));
    if st.opts.no_length && meta.is_file() {
        xml.push_str(
            "<D:propstat><D:prop><D:getcontentlength/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>",
        );
    }
    xml.push_str("</D:response>");
}

fn parse_range(v: &str) -> Option<(u64, u64)> {
    let (a, b) = v.strip_prefix("bytes=")?.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn encode_loose(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            _ => out.push(c),
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { char::from(T[((n >> shift) & 63) as usize]) } else { '=' });
        }
    }
    out
}

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Parses `k=v, k="v"` parameters of a Digest `Authorization` header.
fn params(v: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = v.trim();
    while !rest.is_empty() {
        let Some((k, r)) = rest.split_once('=') else { break };
        let k = k.trim().trim_start_matches(',').trim().to_owned();
        let r = r.trim_start();
        let (val, r) = if let Some(r) = r.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            (r[..end].to_owned(), &r[(end + 1).min(r.len())..])
        } else {
            let end = r.find(',').unwrap_or(r.len());
            (r[..end].trim().to_owned(), &r[end..])
        };
        out.push((k, val));
        rest = r.trim_start_matches([',', ' ']);
    }
    out
}

/// Checks credentials; on failure writes the 401 and returns false.
fn authorized(st: &State, req: &Request, s: &mut dyn Write) -> bool {
    let head = req.method == "HEAD";
    match st.opts.auth {
        Auth::None => true,
        Auth::Basic => {
            let want = format!("Basic {}", b64(format!("{USER}:{PASS}").as_bytes()));
            if req.header("authorization") == Some(want.as_str()) {
                return true;
            }
            respond(s, "401 Unauthorized", &[("WWW-Authenticate", r#"Basic realm="mock""#.to_owned())], b"", head);
            false
        }
        Auth::Digest { rotate } => {
            let nonce = format!("nonce{}", st.nonce_gen.load(Ordering::SeqCst));
            let challenge = |stale: bool| {
                let mut c = format!(r#"Digest realm="mock", qop="auth", algorithm=MD5, nonce="{nonce}", opaque="op""#);
                if stale {
                    c.push_str(", stale=true");
                }
                ("WWW-Authenticate", c)
            };
            let Some(h) = req.header("authorization").and_then(|h| h.strip_prefix("Digest ")) else {
                respond(s, "401 Unauthorized", &[challenge(false)], b"", head);
                return false;
            };
            let p = params(h);
            let get = |k: &str| p.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or("");
            if get("nonce") != nonce {
                respond(s, "401 Unauthorized", &[challenge(true)], b"", head);
                return false;
            }
            let ha1 = md5_hex(&format!("{}:mock:{PASS}", get("username")));
            let ha2 = md5_hex(&format!("{}:{}", req.method, get("uri")));
            let want = md5_hex(&format!("{ha1}:{nonce}:{}:{}:auth:{ha2}", get("nc"), get("cnonce")));
            let ok = get("username") == USER
                && get("response") == want
                && get("uri") == req.target
                && get("opaque") == "op";
            if !ok {
                respond(s, "401 Unauthorized", &[challenge(false)], b"", head);
                return false;
            }
            if rotate > 0 && st.authed.fetch_add(1, Ordering::SeqCst) % rotate == rotate - 1 {
                st.nonce_gen.fetch_add(1, Ordering::SeqCst);
            }
            true
        }
    }
}

/// A self-signed certificate for `localhost` and a server config with it.
pub fn self_signed() -> (Vec<u8>, Arc<rustls::ServerConfig>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("rcgen");
    let der = cert.cert.der().to_vec();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![der.clone().into()], key)
        .expect("server config");
    (der, Arc::new(cfg))
}
