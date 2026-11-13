// SPDX-License-Identifier: MPL-2.0
//! Real HTTPS transport: reqwest with rustls and the ring crypto provider.
//! No OpenSSL and no aws-lc (deny.toml); certificates are checked by the
//! platform verifier.

use std::time::Duration;

use crate::transport::{BoxFuture, HttpRequest, HttpResponse, Transport, TransportError};

/// [`Transport`] over reqwest.
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Builds a client with a per-request timeout. Installs ring as the
    /// process-wide rustls provider if none is installed yet.
    pub fn new(timeout: Duration) -> Result<Self, TransportError> {
        // Fails only if another provider is already installed: that is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| TransportError(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Transport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
                .map_err(|e| TransportError(e.to_string()))?;
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if let Some(body) = request.body {
                builder = builder.body(body);
            }
            // `without_url`: query strings must not end up in error logs.
            let response = builder
                .send()
                .await
                .map_err(|e| TransportError(e.without_url().to_string()))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_owned(),
                        String::from_utf8_lossy(v.as_bytes()).into_owned(),
                    )
                })
                .collect();
            let body = response
                .bytes()
                .await
                .map_err(|e| TransportError(e.without_url().to_string()))?
                .to_vec();
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}
