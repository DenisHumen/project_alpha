// SPDX-License-Identifier: MPL-2.0
//! The HTTP/1.1 subset the proxy speaks: `GET`/`HEAD`, one byte range,
//! keep-alive.

use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;

use pa_vfs::{ByteSource, ErrorKind};

use crate::{Shared, decode_path};

/// Longest request head accepted.
const MAX_HEAD: usize = 16 * 1024;

/// A parsed request head.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub head_only: bool,
    pub target: String,
    pub range: Option<String>,
    pub keep_alive: bool,
}

/// Why no request could be read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// The client closed the connection (or timed out) between requests.
    Closed,
    /// A malformed head: answered with this status, then closed.
    Bad(u16),
}

/// A byte range resolved against the file size.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Range {
    /// The whole file.
    Full,
    /// `start..=end`, both inside the file.
    Part(u64, u64),
    /// Outside the file.
    Unsatisfiable,
}

pub(crate) fn read_request(r: &mut impl BufRead) -> Result<Request, ReadError> {
    let mut line = String::new();
    let mut total = 0;
    // Request line; tolerate empty lines before it (RFC 9112 §2.2).
    loop {
        line.clear();
        let n = r.read_line(&mut line).map_err(|_| ReadError::Closed)?;
        if n == 0 {
            return Err(ReadError::Closed);
        }
        total += n;
        if total > MAX_HEAD {
            return Err(ReadError::Bad(431));
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(ReadError::Bad(400));
    };
    let head_only = match method {
        "GET" => false,
        "HEAD" => true,
        _ => return Err(ReadError::Bad(405)),
    };
    let mut keep_alive = version == "HTTP/1.1";
    let target = target.to_owned();
    let mut range = None;
    loop {
        line.clear();
        let n = r.read_line(&mut line).map_err(|_| ReadError::Bad(400))?;
        if n == 0 {
            return Err(ReadError::Bad(400));
        }
        total += n;
        if total > MAX_HEAD {
            return Err(ReadError::Bad(431));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        let Some((name, value)) = l.split_once(':') else {
            return Err(ReadError::Bad(400));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("range") {
            range = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("connection") {
            let v = value.to_ascii_lowercase();
            if v.contains("close") {
                keep_alive = false;
            } else if v.contains("keep-alive") {
                keep_alive = true;
            }
        }
    }
    Ok(Request {
        head_only,
        target,
        range,
        keep_alive,
    })
}

/// Resolves a `Range` header value against `size`. Anything but a single
/// `bytes=` range is ignored (the whole file is sent), as RFC 9110 allows.
pub(crate) fn parse_range(value: Option<&str>, size: u64) -> Range {
    let Some(spec) = value.and_then(|v| v.trim().strip_prefix("bytes=")) else {
        return Range::Full;
    };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return Range::Full;
    };
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        // Suffix: the last `b` bytes.
        return match b.parse::<u64>() {
            Ok(0) => Range::Unsatisfiable,
            Ok(_) if size == 0 => Range::Unsatisfiable,
            Ok(n) => Range::Part(size.saturating_sub(n), size - 1),
            Err(_) => Range::Full,
        };
    }
    let Ok(start) = a.parse::<u64>() else {
        return Range::Full;
    };
    let end = if b.is_empty() {
        u64::MAX
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => e,
            _ => return Range::Full,
        }
    };
    if start >= size {
        return Range::Unsatisfiable;
    }
    Range::Part(start, end.min(size - 1))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    }
}

/// Media type by file extension, for players that look at it.
pub(crate) fn content_type(resource: &str) -> &'static str {
    let ext = resource
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mkv" | "mk3d" => "video/x-matroska",
        "mka" => "audio/x-matroska",
        "mp4" | "m4v" => "video/mp4",
        "m4a" => "audio/mp4",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mpg" | "mpeg" | "vob" => "video/mpeg",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "srt" => "application/x-subrip",
        "ass" | "ssa" => "text/x-ssa",
        "vtt" => "text/vtt",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn write_status(w: &mut impl Write, status: u16, keep_alive: bool) -> io::Result<()> {
    write!(
        w,
        "HTTP/1.1 {status} {}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: {}\r\n\r\n",
        reason(status),
        if keep_alive { "keep-alive" } else { "close" }
    )?;
    w.flush()
}

fn status_of(kind: ErrorKind) -> u16 {
    match kind {
        ErrorKind::NotFound | ErrorKind::IsADirectory | ErrorKind::NotADirectory => 404,
        ErrorKind::InvalidPath => 400,
        ErrorKind::PermissionDenied | ErrorKind::AuthenticationFailed => 403,
        ErrorKind::TimedOut => 504,
        _ => 502,
    }
}

/// Answers `503` to a connection over the limit.
pub(crate) fn reject_busy(stream: TcpStream, shared: &Shared) {
    let _ = stream.set_write_timeout(Some(shared.options.idle_timeout));
    let mut w = &stream;
    let _ = write_status(&mut w, 503, false);
}

/// Serves one connection until it closes.
pub(crate) fn serve(stream: TcpStream, shared: &Shared) {
    let timeout = Some(shared.options.idle_timeout);
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut w = &stream;
    while !shared.stop.load(Ordering::Relaxed) {
        let req = match read_request(&mut reader) {
            Ok(r) => r,
            Err(ReadError::Closed) => return,
            Err(ReadError::Bad(status)) => {
                let _ = write_status(&mut w, status, false);
                return;
            }
        };
        match respond(&mut w, &req, shared) {
            Ok(true) if req.keep_alive => {}
            _ => return,
        }
    }
}

/// Answers one request. `Ok(true)`: the connection can be reused.
fn respond(w: &mut impl Write, req: &Request, shared: &Shared) -> io::Result<bool> {
    let path = req.target.split(['?', '#']).next().unwrap_or("");
    let resource = path
        .strip_prefix('/')
        .and_then(|p| p.strip_prefix(shared.token.as_str()))
        .and_then(|p| p.strip_prefix('/'));
    let Some(resource) = resource.filter(|r| !r.is_empty()) else {
        write_status(w, 403, req.keep_alive)?;
        return Ok(true);
    };
    let Some(resource) = decode_path(resource) else {
        write_status(w, 400, req.keep_alive)?;
        return Ok(true);
    };
    let source = match shared.opener.open(&resource) {
        Ok(s) => s,
        Err(e) => {
            write_status(w, status_of(e.kind()), req.keep_alive)?;
            return Ok(true);
        }
    };
    let size = source.size();
    let (status, start, len) = match parse_range(req.range.as_deref(), size) {
        Range::Full => (200, 0, size),
        Range::Part(a, b) => (206, a, b - a + 1),
        Range::Unsatisfiable => {
            write!(
                w,
                "HTTP/1.1 416 {}\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: {}\r\n\r\n",
                reason(416),
                if req.keep_alive {
                    "keep-alive"
                } else {
                    "close"
                }
            )?;
            w.flush()?;
            return Ok(true);
        }
    };
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nConnection: {}\r\n",
        reason(status),
        content_type(&resource),
        if req.keep_alive {
            "keep-alive"
        } else {
            "close"
        }
    );
    if status == 206 {
        head.push_str(&format!(
            "Content-Range: bytes {start}-{}/{size}\r\n",
            start + len - 1
        ));
    }
    head.push_str("\r\n");
    w.write_all(head.as_bytes())?;
    if req.head_only {
        w.flush()?;
        return Ok(true);
    }
    send_body(w, source.as_ref(), start, len, shared.options.chunk_size)?;
    w.flush()?;
    Ok(true)
}

/// Copies `len` bytes from `offset`. A source error mid-body cannot be
/// reported any more: the connection is closed (the client sees a short
/// body and retries with a range).
fn send_body(
    w: &mut impl Write,
    source: &dyn ByteSource,
    mut offset: u64,
    mut len: u64,
    chunk: usize,
) -> io::Result<()> {
    let mut buf = vec![0u8; chunk];
    while len > 0 {
        let want = usize::try_from(len).unwrap_or(usize::MAX).min(buf.len());
        let n = source
            .read_at(offset, &mut buf[..want])
            .map_err(io::Error::other)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source ended early",
            ));
        }
        w.write_all(&buf[..n])?;
        offset += n as u64;
        len -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 10), Range::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 10), Range::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=2-4"), 10), Range::Part(2, 4));
        assert_eq!(parse_range(Some("bytes=8-100"), 10), Range::Part(8, 9));
        assert_eq!(parse_range(Some("bytes=-3"), 10), Range::Part(7, 9));
        assert_eq!(parse_range(Some("bytes=-30"), 10), Range::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=10-"), 10), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 10), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-"), 0), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=4-2"), 10), Range::Full);
        assert_eq!(parse_range(Some("bytes=0-1,4-5"), 10), Range::Full);
        assert_eq!(parse_range(Some("items=0-1"), 10), Range::Full);
    }

    #[test]
    fn request_heads() {
        let mut r =
            &b"\r\nHEAD /t/a.mkv HTTP/1.1\r\nRange: bytes=1-2\r\nConnection: close\r\n\r\n"[..];
        let req = read_request(&mut r).unwrap();
        assert!(req.head_only && !req.keep_alive);
        assert_eq!(req.target, "/t/a.mkv");
        assert_eq!(req.range.as_deref(), Some("bytes=1-2"));

        let mut r = &b"GET / HTTP/1.0\r\n\r\n"[..];
        assert!(!read_request(&mut r).unwrap().keep_alive);
        let mut r = &b"POST / HTTP/1.1\r\n\r\n"[..];
        assert_eq!(read_request(&mut r), Err(ReadError::Bad(405)));
        let mut r = &b"GET / HTTP/1.1\r\nbroken\r\n\r\n"[..];
        assert_eq!(read_request(&mut r), Err(ReadError::Bad(400)));
        let mut r = &b""[..];
        assert_eq!(read_request(&mut r), Err(ReadError::Closed));
    }

    #[test]
    fn types() {
        assert_eq!(content_type("1/Movies/Dune.MKV"), "video/x-matroska");
        assert_eq!(content_type("1/a.mp4"), "video/mp4");
        assert_eq!(content_type("1/README"), "application/octet-stream");
    }
}
