// SPDX-License-Identifier: MPL-2.0
//! Minimal HTTP abstraction: provider clients build [`HttpRequest`]s and
//! send them through an injected [`HttpTransport`].
//!
//! Live builds use [`UreqTransport`] (feature `http`); tests and offline
//! tools use [`ReplayTransport`](crate::replay::ReplayTransport). Only `GET`
//! is needed: metadata providers are read-only for us.

use std::fmt::Write as _;

/// Query parameters that carry credentials. They are never part of cache
/// keys, fixture keys or recordings.
pub const AUTH_QUERY_PARAMS: &[&str] = &["api_key"];

/// Headers that carry credentials (lower-case).
pub const AUTH_HEADERS: &[&str] = &["authorization"];

/// One `GET` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Scheme, host and base path, without a trailing slash
    /// (`https://api.themoviedb.org/3`).
    pub base: String,
    /// Path below `base`, starting with `/` (`/movie/550`).
    pub path: String,
    /// Query parameters in the order they are sent.
    pub query: Vec<(String, String)>,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
}

impl HttpRequest {
    /// Full URL with a percent-encoded query string.
    pub fn url(&self) -> String {
        let mut url = format!("{}{}", self.base, self.path);
        for (i, (k, v)) in self.query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            encode_component(k, &mut url);
            url.push('=');
            encode_component(v, &mut url);
        }
        url
    }

    /// Credential-free canonical form: `path?k=v&...` with parameters sorted
    /// and auth parameters dropped. Used for cache and fixture keys.
    pub fn canonical(&self) -> String {
        let mut params: Vec<&(String, String)> = self
            .query
            .iter()
            .filter(|(k, _)| !AUTH_QUERY_PARAMS.contains(&k.as_str()))
            .collect();
        params.sort();
        let mut out = self.path.clone();
        for (i, (k, v)) in params.into_iter().enumerate() {
            out.push(if i == 0 { '?' } else { '&' });
            encode_component(k, &mut out);
            out.push('=');
            encode_component(v, &mut out);
        }
        out
    }
}

/// A response with the body read to a string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers; names are lower-case.
    pub headers: Vec<(String, String)>,
    /// Response body (UTF-8).
    pub body: String,
}

impl HttpResponse {
    /// First value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// The request never produced an HTTP response: DNS, TCP, TLS, timeout, or
/// a body that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

/// Sends HTTP requests. Implementations must be usable from several threads.
pub trait HttpTransport: Send + Sync {
    /// Performs a `GET`. Non-2xx statuses are responses, not errors.
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub(crate) fn encode_component(s: &str, out: &mut String) {
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
}

#[cfg(feature = "http")]
pub use live::UreqTransport;

#[cfg(feature = "http")]
mod live {
    use std::time::Duration;

    use super::{HttpRequest, HttpResponse, HttpTransport, TransportError};

    /// Body size limit: TMDB responses are far below it.
    const MAX_BODY: u64 = 16 * 1024 * 1024;

    /// Blocking HTTPS transport on `ureq` with rustls (ring) and the Mozilla
    /// root store (`webpki-roots`), so it behaves the same on every platform,
    /// Android included, and needs no OpenSSL.
    #[derive(Debug, Clone)]
    pub struct UreqTransport {
        agent: ureq::Agent,
    }

    impl UreqTransport {
        /// A transport with the given `User-Agent` and a 20 s overall timeout.
        pub fn new(user_agent: &str) -> Self {
            Self::with_timeout(user_agent, Duration::from_secs(20))
        }

        /// A transport with a custom overall per-request timeout.
        pub fn with_timeout(user_agent: &str, timeout: Duration) -> Self {
            let config = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(timeout))
                .user_agent(user_agent)
                .build();
            Self {
                agent: ureq::Agent::new_with_config(config),
            }
        }
    }

    impl HttpTransport for UreqTransport {
        fn get(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
            let mut builder = self.agent.get(request.url());
            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            // ureq errors can quote the URL, which may hold a v3 `api_key`;
            // report only the error kind, never the URL.
            let mut response = builder.call().map_err(|e| TransportError(describe(&e)))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(k, v)| {
                    Some((k.as_str().to_ascii_lowercase(), v.to_str().ok()?.to_owned()))
                })
                .collect();
            let body = response
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_string()
                .map_err(|e| TransportError(describe(&e)))?;
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        }
    }

    fn describe(e: &ureq::Error) -> String {
        match e {
            ureq::Error::Timeout(_) => "request timed out".to_owned(),
            ureq::Error::HostNotFound => "host not found".to_owned(),
            ureq::Error::ConnectionFailed => "connection failed".to_owned(),
            ureq::Error::Io(io) => format!("I/O error: {}", io.kind()),
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS error".to_owned(),
            ureq::Error::BodyExceedsLimit(_) => "response body too large".to_owned(),
            _ => "HTTP transport error".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(query: &[(&str, &str)]) -> HttpRequest {
        HttpRequest {
            base: "https://api.example.org/3".into(),
            path: "/search/movie".into(),
            query: query
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            headers: vec![],
        }
    }

    #[test]
    fn url_encodes_query() {
        let r = req(&[("query", "Amélie & co/1"), ("year", "2001")]);
        assert_eq!(
            r.url(),
            "https://api.example.org/3/search/movie?query=Am%C3%A9lie%20%26%20co%2F1&year=2001"
        );
    }

    #[test]
    fn canonical_is_sorted_and_credential_free() {
        let a = req(&[("year", "2001"), ("api_key", "secret"), ("query", "x")]);
        let b = req(&[("query", "x"), ("year", "2001")]);
        assert_eq!(a.canonical(), b.canonical());
        assert_eq!(a.canonical(), "/search/movie?query=x&year=2001");
        assert!(!a.canonical().contains("secret"));
    }
}
