// SPDX-License-Identifier: MPL-2.0
//! The proxy over real sockets: ranges, HEAD, the token, errors,
//! keep-alive, the connection limit, and `curl` when it is installed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use pa_proxy::{Opener, Proxy, ProxyOptions};
use pa_vfs::{ByteSource, LocalFs, Vfs, VfsPath};

/// Serves a local folder: the resource is the path inside it.
struct Folder(LocalFs);

impl Opener for Folder {
    fn open(&self, resource: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        Ok(Arc::from(self.0.open(&VfsPath::parse(resource)?)?))
    }
}

fn data() -> Vec<u8> {
    (0..300_000u32).map(|i| (i % 251) as u8).collect()
}

fn setup(options: ProxyOptions) -> (tempfile::TempDir, Proxy) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Фильмы")).unwrap();
    std::fs::write(dir.path().join("Фильмы/Dune (2021).mkv"), data()).unwrap();
    let opener = Folder(LocalFs::new(dir.path()).unwrap());
    let proxy = Proxy::start(Arc::new(opener), options).unwrap();
    (dir, proxy)
}

/// Path part of a proxy URL.
fn path_of(url: &str) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    rest[rest.find('/').unwrap()..].to_owned()
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_reply(r: &mut BufReader<TcpStream>, head_only: bool) -> Reply {
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let status = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut headers = Vec::new();
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (n, v) = l.split_once(':').unwrap();
        headers.push((n.to_owned(), v.trim().to_owned()));
    }
    let mut reply = Reply {
        status,
        headers,
        body: Vec::new(),
    };
    if !head_only {
        let len: usize = reply.header("content-length").unwrap().parse().unwrap();
        reply.body = vec![0; len];
        r.read_exact(&mut reply.body).unwrap();
    }
    reply
}

fn connect(proxy: &Proxy) -> BufReader<TcpStream> {
    let s = TcpStream::connect(proxy.local_addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    BufReader::new(s)
}

fn request(r: &mut BufReader<TcpStream>, method: &str, path: &str, extra: &str) -> Reply {
    write!(
        r.get_mut(),
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}\r\n"
    )
    .unwrap();
    read_reply(r, method == "HEAD")
}

#[test]
fn serves_whole_files_and_ranges_on_one_connection() {
    let (_dir, proxy) = setup(ProxyOptions::default());
    let path = path_of(&proxy.url("Фильмы/Dune (2021).mkv"));
    let all = data();
    let mut c = connect(&proxy);

    let full = request(&mut c, "GET", &path, "");
    assert_eq!(full.status, 200);
    assert_eq!(full.header("accept-ranges"), Some("bytes"));
    assert_eq!(full.header("content-type"), Some("video/x-matroska"));
    assert_eq!(full.body, all);

    let part = request(&mut c, "GET", &path, "Range: bytes=1000-1999\r\n");
    assert_eq!(part.status, 206);
    assert_eq!(part.header("content-range"), Some("bytes 1000-1999/300000"));
    assert_eq!(part.body, all[1000..2000]);

    let tail = request(&mut c, "GET", &path, "Range: bytes=-10\r\n");
    assert_eq!(tail.body, all[299_990..]);

    let open = request(&mut c, "GET", &path, "Range: bytes=299000-\r\n");
    assert_eq!(open.body, all[299_000..]);

    let head = request(&mut c, "HEAD", &path, "");
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length"), Some("300000"));

    let past = request(&mut c, "GET", &path, "Range: bytes=300000-\r\n");
    assert_eq!(past.status, 416);
    assert_eq!(past.header("content-range"), Some("bytes */300000"));
}

#[test]
fn requests_without_the_token_are_forbidden() {
    let (_dir, proxy) = setup(ProxyOptions::default());
    let good = path_of(&proxy.url("Фильмы/Dune (2021).mkv"));
    let token = good.split('/').nth(1).unwrap().to_owned();
    let mut c = connect(&proxy);
    for path in [
        "/".to_owned(),
        "/Фильмы/Dune%20(2021).mkv".to_owned(),
        good.replacen(&token, "0123456789abcdef0123456789abcdef", 1),
        format!("/{token}"),
        format!("/{token}/"),
        format!("/x{token}/Фильмы/Dune%20(2021).mkv"),
    ] {
        assert_eq!(request(&mut c, "GET", &path, "").status, 403, "{path}");
    }
    assert!(!format!("{proxy:?}").contains(&token));
}

#[test]
fn errors_map_to_statuses() {
    let (_dir, proxy) = setup(ProxyOptions::default());
    let mut c = connect(&proxy);
    let missing = path_of(&proxy.url("Фильмы/nope.mkv"));
    assert_eq!(request(&mut c, "GET", &missing, "").status, 404);
    let dir = path_of(&proxy.url("Фильмы"));
    assert_eq!(request(&mut c, "GET", &dir, "").status, 404);
    let escape = format!("{}/..%2Fetc", proxy.base_url());
    assert_eq!(request(&mut c, "GET", &path_of(&escape), "").status, 400);
    let broken = format!("{}/%zz", proxy.base_url());
    assert_eq!(request(&mut c, "GET", &path_of(&broken), "").status, 400);

    let mut c = connect(&proxy);
    write!(c.get_mut(), "DELETE / HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(read_reply(&mut c, false).status, 405);
}

#[test]
fn connections_over_the_limit_get_503() {
    let (_dir, proxy) = setup(ProxyOptions {
        max_connections: 2,
        ..ProxyOptions::default()
    });
    let path = path_of(&proxy.url("Фильмы/Dune (2021).mkv"));
    let mut a = connect(&proxy);
    let mut b = connect(&proxy);
    assert_eq!(request(&mut a, "HEAD", &path, "").status, 200);
    assert_eq!(request(&mut b, "HEAD", &path, "").status, 200);
    let mut c = connect(&proxy);
    assert_eq!(read_reply(&mut c, false).status, 503);
    drop(a);
    // The slot comes back once the server notices the close.
    let mut ok = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        if proxy.connections() < 2 {
            let mut d = connect(&proxy);
            ok = request(&mut d, "HEAD", &path, "").status == 200;
            break;
        }
    }
    assert!(ok);
}

#[test]
fn a_client_that_goes_away_mid_body_frees_its_slot() {
    let (_dir, proxy) = setup(ProxyOptions {
        chunk_size: 4096,
        ..ProxyOptions::default()
    });
    let path = path_of(&proxy.url("Фильмы/Dune (2021).mkv"));
    {
        let mut c = connect(&proxy);
        write!(c.get_mut(), "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut first = [0u8; 64];
        c.read_exact(&mut first).unwrap();
    }
    for _ in 0..100 {
        if proxy.connections() == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("connection still counted");
}

#[test]
fn dropping_the_proxy_stops_listening() {
    let (_dir, proxy) = setup(ProxyOptions::default());
    let addr = proxy.local_addr();
    drop(proxy);
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}

/// NET-007 "done when": curl reads a file through the proxy (skipped
/// when curl is not installed).
#[test]
fn curl_reads_through_the_proxy() {
    if std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("curl not installed: skipped");
        return;
    }
    let (_dir, proxy) = setup(ProxyOptions::default());
    let url = proxy.url("Фильмы/Dune (2021).mkv");
    let out = std::process::Command::new("curl")
        .args(["-sf", "-r", "100-199", &url])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, data()[100..200]);
    let code = std::process::Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}"])
        .arg(format!("http://{}/Фильмы/x.mkv", proxy.local_addr()))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&code.stdout), "403");
}
