// SPDX-License-Identifier: MPL-2.0
//! HTTP Basic (RFC 7617) and Digest (RFC 7616) authentication.
//!
//! The client sends the first request without credentials, picks the
//! strongest scheme from the `401` challenges (Digest SHA-256, Digest MD5,
//! Basic) and then authenticates every request preemptively until the
//! server rejects the state again (a stale nonce).

use std::fmt::Write as _;

use md5::Md5;
use sha2::{Digest as _, Sha256};

use crate::Credentials;

/// One challenge from `WWW-Authenticate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Challenge {
    /// Scheme in lower case (`basic`, `digest`).
    pub scheme: String,
    /// Parameters with lower-case names; values unquoted.
    pub params: Vec<(String, String)>,
}

impl Challenge {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parses all challenges of one or more `WWW-Authenticate` values. A value
/// may carry several challenges: `Digest realm="a", nonce="b", Basic realm="a"`.
pub(crate) fn parse_challenges<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<Challenge> {
    let mut out: Vec<Challenge> = Vec::new();
    for value in values {
        let mut p = Parser {
            s: value.as_bytes(),
            i: 0,
        };
        loop {
            p.skip_separators();
            let Some(token) = p.token() else { break };
            p.skip_ws();
            if p.peek() == Some(b'=') {
                // A parameter of the current challenge.
                p.i += 1;
                p.skip_ws();
                let v = p.value();
                if let Some(c) = out.last_mut() {
                    c.params.push((token.to_ascii_lowercase(), v));
                }
            } else {
                out.push(Challenge {
                    scheme: token.to_ascii_lowercase(),
                    params: Vec::new(),
                });
            }
        }
    }
    out
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b',')) {
            self.i += 1;
        }
    }
    fn token(&mut self) -> Option<String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~/".contains(&c) {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            // Skip one unexpected byte so the loop always advances.
            if self.i < self.s.len() {
                self.i += 1;
                return self.token();
            }
            return None;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
    fn value(&mut self) -> String {
        if self.peek() == Some(b'"') {
            self.i += 1;
            let mut out = Vec::new();
            while let Some(c) = self.peek() {
                self.i += 1;
                match c {
                    b'"' => break,
                    b'\\' => {
                        if let Some(n) = self.peek() {
                            out.push(n);
                            self.i += 1;
                        }
                    }
                    _ => out.push(c),
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        } else {
            let start = self.i;
            while !matches!(self.peek(), None | Some(b',' | b' ' | b'\t')) {
                self.i += 1;
            }
            String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
        }
    }
}

/// Hash algorithm of a Digest challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Md5,
    Sha256,
}

impl Algorithm {
    fn hex(self, data: &str) -> String {
        match self {
            Self::Md5 => hex(&Md5::digest(data.as_bytes())),
            Self::Sha256 => hex(&Sha256::digest(data.as_bytes())),
        }
    }
}

/// Authentication state learned from a challenge.
#[derive(Debug, Clone)]
pub(crate) enum AuthState {
    /// Precomputed `Basic …` header.
    Basic(String),
    /// Digest parameters; the nonce count grows per request.
    Digest(DigestState),
}

/// Digest parameters from the server and our nonce counter.
#[derive(Debug, Clone)]
pub(crate) struct DigestState {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    algorithm: Algorithm,
    algorithm_name: String,
    session: bool,
    qop_auth: bool,
    nc: u32,
}

impl AuthState {
    /// Picks the strongest supported challenge: Digest SHA-256, Digest MD5,
    /// then Basic. `None` if nothing is supported (NTLM, Negotiate, Bearer).
    pub(crate) fn from_challenges(challenges: &[Challenge], creds: &Credentials) -> Option<Self> {
        let digest = challenges
            .iter()
            .filter(|c| c.scheme == "digest")
            .filter_map(DigestState::from_challenge)
            .max_by_key(|d| d.algorithm == Algorithm::Sha256);
        if let Some(d) = digest {
            return Some(Self::Digest(d));
        }
        challenges
            .iter()
            .any(|c| c.scheme == "basic")
            .then(|| Self::Basic(basic_header(creds)))
    }

    /// `stale=true` in a Digest challenge: the password was fine, only the
    /// nonce expired, so retrying with the new nonce is allowed.
    pub(crate) fn is_stale(challenges: &[Challenge]) -> bool {
        challenges.iter().any(|c| {
            c.scheme == "digest"
                && c.param("stale")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        })
    }

    /// `Authorization` value for `method` and `uri` (path and query).
    pub(crate) fn header(&mut self, creds: &Credentials, method: &str, uri: &str) -> String {
        match self {
            Self::Basic(h) => h.clone(),
            Self::Digest(d) => {
                d.nc = d.nc.wrapping_add(1);
                d.header(creds, method, uri, &cnonce())
            }
        }
    }
}

impl DigestState {
    fn from_challenge(c: &Challenge) -> Option<Self> {
        let name = c.param("algorithm").unwrap_or("MD5");
        let upper = name.to_ascii_uppercase();
        let (base, session) = match upper.strip_suffix("-SESS") {
            Some(b) => (b.to_owned(), true),
            None => (upper.clone(), false),
        };
        let algorithm = match base.as_str() {
            "MD5" => Algorithm::Md5,
            "SHA-256" => Algorithm::Sha256,
            _ => return None,
        };
        let qop = c.param("qop");
        let qop_auth =
            qop.is_some_and(|q| q.split(',').any(|v| v.trim().eq_ignore_ascii_case("auth")));
        // Only `auth-int` offered: we do not hash bodies.
        if qop.is_some() && !qop_auth {
            return None;
        }
        Some(Self {
            realm: c.param("realm").unwrap_or_default().to_owned(),
            nonce: c.param("nonce")?.to_owned(),
            opaque: c.param("opaque").map(str::to_owned),
            algorithm,
            algorithm_name: name.to_owned(),
            session,
            qop_auth,
            nc: 0,
        })
    }

    fn header(&self, creds: &Credentials, method: &str, uri: &str, cnonce: &str) -> String {
        let h = |s: &str| self.algorithm.hex(s);
        let nc = format!("{:08x}", self.nc);
        let mut ha1 = h(&format!(
            "{}:{}:{}",
            creds.username, self.realm, creds.password
        ));
        if self.session {
            ha1 = h(&format!("{ha1}:{}:{cnonce}", self.nonce));
        }
        let ha2 = h(&format!("{method}:{uri}"));
        let response = if self.qop_auth {
            h(&format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", self.nonce))
        } else {
            h(&format!("{ha1}:{}:{ha2}", self.nonce))
        };
        let mut out = format!(
            "Digest username={}, realm={}, nonce={}, uri={}, algorithm={}, response=\"{response}\"",
            quote(&creds.username),
            quote(&self.realm),
            quote(&self.nonce),
            quote(uri),
            self.algorithm_name,
        );
        if self.qop_auth {
            let _ = write!(out, ", qop=auth, nc={nc}, cnonce=\"{cnonce}\"");
        }
        if let Some(o) = &self.opaque {
            let _ = write!(out, ", opaque={}", quote(o));
        }
        out
    }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn cnonce() -> String {
    let mut b = [0u8; 16];
    // A failed RNG only weakens the client nonce; the server nonce still
    // protects against replay.
    let _ = getrandom::fill(&mut b);
    hex(&b)
}

/// `Basic base64(user:password)`.
pub(crate) fn basic_header(creds: &Credentials) -> String {
    format!(
        "Basic {}",
        base64(format!("{}:{}", creds.username, creds.password).as_bytes())
    )
}

/// Standard base64 with padding.
pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(u: &str, p: &str) -> Credentials {
        Credentials::new(u, p)
    }

    #[test]
    fn base64_vectors() {
        // RFC 4648 §10.
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        // RFC 7617 §2.
        assert_eq!(
            basic_header(&creds("Aladdin", "open sesame")),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn parses_several_challenges() {
        let c = parse_challenges([
            r#"Digest realm="a, b", qop="auth,auth-int", nonce="n\"1", Basic realm="x""#,
            "Bearer",
        ]);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].scheme, "digest");
        assert_eq!(c[0].param("realm"), Some("a, b"));
        assert_eq!(c[0].param("qop"), Some("auth,auth-int"));
        assert_eq!(c[0].param("nonce"), Some("n\"1"));
        assert_eq!(c[1].scheme, "basic");
        assert_eq!(c[2].scheme, "bearer");
    }

    #[test]
    fn prefers_digest_sha256() {
        let cr = creds("u", "p");
        let c = parse_challenges([
            r#"Basic realm="r""#,
            r#"Digest realm="r", nonce="n", algorithm=MD5, qop="auth""#,
            r#"Digest realm="r", nonce="n", algorithm=SHA-256, qop="auth""#,
        ]);
        let Some(AuthState::Digest(d)) = AuthState::from_challenges(&c, &cr) else {
            panic!("expected digest");
        };
        assert_eq!(d.algorithm, Algorithm::Sha256);
        let basic = parse_challenges([r#"Basic realm="r""#]);
        assert!(matches!(
            AuthState::from_challenges(&basic, &cr),
            Some(AuthState::Basic(_))
        ));
        assert!(AuthState::from_challenges(&parse_challenges(["NTLM"]), &cr).is_none());
        assert!(AuthState::is_stale(&parse_challenges([
            r#"Digest nonce="x", stale=TRUE"#
        ])));
    }

    /// RFC 7616 §3.9.1 (MD5 and SHA-256 with qop=auth).
    #[test]
    fn rfc7616_vectors() {
        let cr = creds("Mufasa", "Circle of Life");
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        for (alg, want) in [
            ("MD5", "8ca523f5e9506fed4657c9700eebdbec"),
            (
                "SHA-256",
                "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1",
            ),
        ] {
            let c = parse_challenges([format!(
                r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm={alg}, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#
            )
            .as_str()]);
            let mut d = DigestState::from_challenge(&c[0]).unwrap();
            d.nc = 1;
            let h = d.header(&cr, "GET", "/dir/index.html", cnonce);
            assert!(h.contains(&format!("response=\"{want}\"")), "{alg}: {h}");
            assert!(h.contains("nc=00000001"));
            assert!(h.contains("opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\""));
        }
    }

    /// RFC 2617 §3.5 (MD5, qop=auth).
    #[test]
    fn rfc2617_vector() {
        let c = parse_challenges([
            r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
        ]);
        let mut d = DigestState::from_challenge(&c[0]).unwrap();
        d.nc = 1;
        let h = d.header(
            &creds("Mufasa", "Circle Of Life"),
            "GET",
            "/dir/index.html",
            "0a4f113b",
        );
        assert!(
            h.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "{h}"
        );
    }

    #[test]
    fn only_auth_int_is_unsupported() {
        let c = parse_challenges([r#"Digest nonce="n", qop="auth-int""#]);
        assert!(DigestState::from_challenge(&c[0]).is_none());
        let c = parse_challenges([r#"Digest nonce="n", algorithm=SHA-512-256"#]);
        assert!(DigestState::from_challenge(&c[0]).is_none());
    }
}
