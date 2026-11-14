// SPDX-License-Identifier: MPL-2.0
//! Downloading images: the [`ImageFetcher`] seam and the live HTTPS fetcher.

/// Largest image the cache accepts. TMDB `original` backdrops stay well
/// below it.
pub const MAX_IMAGE_BYTES: u64 = 24 * 1024 * 1024;

/// A downloaded response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fetched {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: Vec<u8>,
}

/// The request never produced a response (DNS, TCP, TLS, timeout, a body
/// over [`MAX_IMAGE_BYTES`]). For the cache this means "offline".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FetchError(pub String);

/// Downloads an image. Implementations must be usable from several threads.
pub trait ImageFetcher: Send + Sync {
    /// `GET url`. Non-2xx statuses are responses, not errors.
    fn fetch(&self, url: &str) -> Result<Fetched, FetchError>;
}

#[cfg(feature = "http")]
pub use live::UreqFetcher;

#[cfg(feature = "http")]
mod live {
    use std::time::Duration;

    use super::{FetchError, Fetched, ImageFetcher, MAX_IMAGE_BYTES};

    /// Blocking HTTPS fetcher on `ureq` with rustls (ring) and the Mozilla
    /// root store — the same stack as `pa-metadata` (ADR-0018), no OpenSSL.
    #[derive(Debug, Clone)]
    pub struct UreqFetcher {
        agent: ureq::Agent,
    }

    impl UreqFetcher {
        /// A fetcher with the given `User-Agent` and a 30 s overall timeout.
        pub fn new(user_agent: &str) -> Self {
            let config = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(30)))
                .user_agent(user_agent)
                .build();
            Self {
                agent: ureq::Agent::new_with_config(config),
            }
        }
    }

    impl ImageFetcher for UreqFetcher {
        fn fetch(&self, url: &str) -> Result<Fetched, FetchError> {
            let mut response = self
                .agent
                .get(url)
                .header("accept", "image/avif,image/webp,image/*;q=0.8")
                .call()
                .map_err(|e| FetchError(describe(&e)))?;
            let status = response.status().as_u16();
            let body = response
                .body_mut()
                .with_config()
                .limit(MAX_IMAGE_BYTES)
                .read_to_vec()
                .map_err(|e| FetchError(describe(&e)))?;
            Ok(Fetched { status, body })
        }
    }

    fn describe(e: &ureq::Error) -> String {
        match e {
            ureq::Error::Timeout(_) => "request timed out".to_owned(),
            ureq::Error::HostNotFound => "host not found".to_owned(),
            ureq::Error::ConnectionFailed => "connection failed".to_owned(),
            ureq::Error::Io(io) => format!("I/O error: {}", io.kind()),
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS error".to_owned(),
            ureq::Error::BodyExceedsLimit(_) => "image too large".to_owned(),
            _ => "HTTP transport error".to_owned(),
        }
    }
}
