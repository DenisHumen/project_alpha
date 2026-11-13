// SPDX-License-Identifier: MPL-2.0
//! HTTP and time abstractions, so the client can run on fixtures.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

/// Boxed `Send` future used by the object-safe traits of this crate.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// HTTP method of a Helix call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `GET`
    Get,
    /// `POST`
    Post,
    /// `PATCH`
    Patch,
    /// `PUT`
    Put,
    /// `DELETE`
    Delete,
}

impl Method {
    /// Upper-case method name.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        }
    }
}

/// A fully built request: absolute URL with query, headers and body.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Method.
    pub method: Method,
    /// Absolute URL including the query string.
    pub url: String,
    /// Header name/value pairs.
    pub headers: Vec<(String, String)>,
    /// Request body (JSON for Helix writes).
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// First header value with this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

impl std::fmt::Debug for HttpRequest {
    // The Authorization header carries the user token: never print it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                if k.eq_ignore_ascii_case("authorization") {
                    (k.as_str(), "<redacted>")
                } else {
                    (k.as_str(), v.as_str())
                }
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// A response as received from the network.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Header name/value pairs.
    pub headers: Vec<(String, String)>,
    /// Raw body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// First header value with this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Network-level failure (DNS, TLS, connection reset, timeout).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

/// Sends HTTP requests. Implemented by [`crate::ReqwestTransport`], by the
/// fixture transport in [`crate::testing`] and by whatever HTTP stack the app
/// wants to inject.
pub trait Transport: Send + Sync {
    /// Sends one request. Non-2xx statuses are a normal `Ok` response.
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>>;
}

/// Wall clock and sleeping, injectable for tests.
pub trait Clock: Send + Sync {
    /// Current wall-clock time (`Ratelimit-Reset` is a Unix timestamp).
    fn now(&self) -> SystemTime;
    /// Waits for `duration`.
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()>;
}

/// Real clock: `SystemTime::now` and `tokio::time::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
