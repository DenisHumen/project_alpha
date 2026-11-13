// SPDX-License-Identifier: MPL-2.0
//! HTTP-level record/replay ("cassettes") for tests and offline tools.
//!
//! A cassette is JSONL, one exchange per line:
//!
//! ```json
//! {"request":{"path":"/search/movie","query":{"query":"x","language":"en-US"}},
//!  "response":{"status":200,"headers":{"retry-after":"1"},"body":{...}}}
//! ```
//!
//! Requests are matched by [`HttpRequest::canonical`] — path plus sorted
//! query, **without** credentials — so cassettes never contain keys and
//! replay works with any key. A `POST` additionally matches its JSON body
//! ([`HttpRequest::canonical_post`]); a GraphQL body is written as its
//! operation and variables:
//!
//! ```json
//! {"request":{"path":"/","graphql":{"operation":"Search","variables":{"search":"x"}}},
//!  "response":{"status":200,"body":{"data":{...}}}}
//! ```
//!
//! Other `POST` bodies go to `"body"` as JSON. Several lines for the same request are served
//! in order and the last one repeats (to script "429, then 200").
//!
//! Provider-neutral recording for the eval runner happens one level up
//! (`pa_eval::RecordingProvider` over a client); this one checks the HTTP
//! handling itself: status codes, headers, retries.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{
    AUTH_HEADERS, HttpRequest, HttpResponse, HttpTransport, TransportError, canonical_body,
    encode_component, graphql_parts,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Exchange {
    request: RecordedRequest,
    response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordedRequest {
    path: String,
    #[serde(default)]
    query: BTreeMap<String, String>,
    /// GraphQL `POST`: operation name and variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graphql: Option<RecordedGraphql>,
    /// Any other JSON `POST` body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordedGraphql {
    operation: String,
    #[serde(default)]
    variables: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordedResponse {
    status: u16,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    /// JSON bodies are stored as JSON; anything else as a string.
    #[serde(default)]
    body: Value,
}

impl RecordedRequest {
    fn canonical(&self) -> String {
        let mut out = self.path.clone();
        for (i, (k, v)) in self.query.iter().enumerate() {
            out.push(if i == 0 { '?' } else { '&' });
            encode_component(k, &mut out);
            out.push('=');
            encode_component(v, &mut out);
        }
        if let Some(g) = &self.graphql {
            let variables = match &g.variables {
                Value::Null => "{}".to_owned(),
                v => v.to_string(),
            };
            out.push_str(&format!(" graphql:{}{variables}", g.operation));
        } else if let Some(body) = &self.body {
            out.push(' ');
            out.push_str(&canonical_body(body));
        }
        out
    }

    fn from_live(request: &HttpRequest, body: Option<&str>) -> Self {
        let mut recorded = Self {
            path: request.path.clone(),
            query: request
                .query
                .iter()
                .filter(|(k, _)| !crate::http::AUTH_QUERY_PARAMS.contains(&k.as_str()))
                .cloned()
                .collect(),
            graphql: None,
            body: None,
        };
        if let Some(body) = body {
            let value: Value =
                serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_owned()));
            match graphql_parts(&value) {
                Some((operation, _)) => {
                    recorded.graphql = Some(RecordedGraphql {
                        operation,
                        variables: value.get("variables").cloned().unwrap_or(Value::Null),
                    });
                }
                None => recorded.body = Some(value),
            }
        }
        recorded
    }
}

impl From<RecordedResponse> for HttpResponse {
    fn from(r: RecordedResponse) -> Self {
        let body = match r.body {
            Value::Null => String::new(),
            Value::String(s) => s,
            other => other.to_string(),
        };
        HttpResponse {
            status: r.status,
            headers: r
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body,
        }
    }
}

/// Serves responses from a cassette; unknown requests fail with a
/// [`TransportError`] naming the canonical request.
#[derive(Debug, Default)]
pub struct ReplayTransport {
    routes: Mutex<HashMap<String, VecDeque<HttpResponse>>>,
    log: Mutex<Vec<HttpRequest>>,
}

impl ReplayTransport {
    /// Parses a JSONL cassette.
    pub fn from_jsonl(text: &str) -> Result<Self, String> {
        let transport = Self::default();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let ex: Exchange =
                serde_json::from_str(line).map_err(|e| format!("cassette line {}: {e}", i + 1))?;
            transport.push(&ex.request.canonical(), ex.response.into());
        }
        Ok(transport)
    }

    /// Adds a response for a canonical request (`/path?k=v`, sorted).
    pub fn push(&self, canonical: &str, response: HttpResponse) {
        self.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(canonical.to_owned())
            .or_default()
            .push_back(response);
    }

    /// Requests received so far (`GET` and `POST`), in order, as sent (credentials included —
    /// tests use this to check how the key is passed).
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl ReplayTransport {
    fn serve(&self, request: &HttpRequest, key: String) -> Result<HttpResponse, TransportError> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.clone());
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        let queue = routes
            .get_mut(&key)
            .ok_or_else(|| TransportError(format!("not in cassette: {key}")))?;
        let response = if queue.len() > 1 {
            queue.pop_front()
        } else {
            queue.front().cloned()
        };
        response.ok_or_else(|| TransportError(format!("not in cassette: {key}")))
    }
}

impl HttpTransport for ReplayTransport {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.serve(request, request.canonical())
    }

    fn post_json(&self, request: &HttpRequest, body: &str) -> Result<HttpResponse, TransportError> {
        self.serve(request, request.canonical_post(body))
    }
}

/// Wraps a live transport and records every exchange, credentials stripped.
///
/// Recordings of real provider data are for local checks only: TMDB data
/// must not be committed (docs/legal/providers.md, TMDB §6–7). Committed
/// fixtures are synthetic.
pub struct RecordingTransport<T> {
    inner: T,
    lines: Mutex<Vec<String>>,
}

impl<T: HttpTransport> RecordingTransport<T> {
    /// Starts recording.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            lines: Mutex::new(Vec::new()),
        }
    }

    /// The cassette recorded so far (JSONL).
    pub fn to_jsonl(&self) -> String {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().map(|l| format!("{l}\n")).collect()
    }
}

impl<T: HttpTransport> RecordingTransport<T> {
    fn record(
        &self,
        request: &HttpRequest,
        body: Option<&str>,
        response: &HttpResponse,
    ) -> Result<(), TransportError> {
        let exchange = Exchange {
            request: RecordedRequest::from_live(request, body),
            response: RecordedResponse {
                status: response.status,
                headers: response
                    .headers
                    .iter()
                    .filter(|(k, _)| {
                        !AUTH_HEADERS.contains(&k.as_str()) && !k.eq_ignore_ascii_case("set-cookie")
                    })
                    .cloned()
                    .collect(),
                body: serde_json::from_str(&response.body)
                    .unwrap_or_else(|_| Value::String(response.body.clone())),
            },
        };
        let line = serde_json::to_string(&exchange).map_err(|e| TransportError(e.to_string()))?;
        self.lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(line);
        Ok(())
    }
}

impl<T: HttpTransport> HttpTransport for RecordingTransport<T> {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let response = self.inner.get(request)?;
        self.record(request, None, &response)?;
        Ok(response)
    }

    fn post_json(&self, request: &HttpRequest, body: &str) -> Result<HttpResponse, TransportError> {
        let response = self.inner.post_json(request, body)?;
        self.record(request, Some(body), &response)?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, query: &[(&str, &str)]) -> HttpRequest {
        HttpRequest {
            base: "https://example.org/3".into(),
            path: path.into(),
            query: query
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            headers: vec![],
        }
    }

    #[test]
    fn replays_in_order_and_repeats_last() {
        let cassette = r#"
{"request":{"path":"/a","query":{"q":"x y"}},"response":{"status":429,"headers":{"Retry-After":"2"}}}
{"request":{"path":"/a","query":{"q":"x y"}},"response":{"status":200,"body":{"ok":true}}}
"#;
        let t = ReplayTransport::from_jsonl(cassette).unwrap();
        let r = request("/a", &[("api_key", "k"), ("q", "x y")]);
        let first = t.get(&r).unwrap();
        assert_eq!(
            (first.status, first.header("retry-after")),
            (429, Some("2"))
        );
        assert_eq!(t.get(&r).unwrap().body, r#"{"ok":true}"#);
        assert_eq!(t.get(&r).unwrap().status, 200);
        assert!(t.get(&request("/b", &[])).is_err());
        assert_eq!(t.requests().len(), 4);
    }

    #[test]
    fn recording_strips_credentials_and_round_trips() {
        let live = ReplayTransport::default();
        live.push(
            "/a?q=1",
            HttpResponse {
                status: 200,
                headers: vec![("set-cookie".into(), "s=1".into())],
                body: r#"{"v":1}"#.into(),
            },
        );
        let rec = RecordingTransport::new(live);
        rec.get(&request("/a", &[("q", "1"), ("api_key", "SECRETKEY")]))
            .unwrap();
        let jsonl = rec.to_jsonl();
        assert!(!jsonl.contains("SECRETKEY"), "{jsonl}");
        assert!(!jsonl.contains("set-cookie"), "{jsonl}");
        let replay = ReplayTransport::from_jsonl(&jsonl).unwrap();
        assert_eq!(
            replay.get(&request("/a", &[("q", "1")])).unwrap().body,
            r#"{"v":1}"#
        );
    }

    #[test]
    fn graphql_post_records_operation_and_replays() {
        let cassette = r#"{"request":{"path":"/","graphql":{"operation":"Media","variables":{"id":7}}},"response":{"status":200,"body":{"data":{"Media":{"id":7}}}}}"#;
        let t = ReplayTransport::from_jsonl(cassette).unwrap();
        let r = request("/", &[]);
        let body =
            r#"{"query":"query Media($id: Int) { Media(id: $id) { id } }","variables":{"id":7}}"#;
        assert_eq!(
            t.post_json(&r, body).unwrap().body,
            r#"{"data":{"Media":{"id":7}}}"#
        );
        assert!(t.get(&r).is_err(), "GET does not match a POST line");
        assert!(
            t.post_json(&r, r#"{"query":"query Media { x }","variables":{"id":8}}"#)
                .is_err()
        );

        let rec = RecordingTransport::new(t);
        rec.post_json(&r, body).unwrap();
        let jsonl = rec.to_jsonl();
        assert!(
            jsonl.contains(r#""graphql":{"operation":"Media","variables":{"id":7}}"#),
            "{jsonl}"
        );
        assert!(
            !jsonl.contains("query Media"),
            "query text is not recorded: {jsonl}"
        );
        let replay = ReplayTransport::from_jsonl(&jsonl).unwrap();
        assert!(replay.post_json(&r, body).is_ok());

        let plain = ReplayTransport::from_jsonl(
            r#"{"request":{"path":"/p","body":{"b":1,"a":[1]}},"response":{"status":201}}"#,
        )
        .unwrap();
        assert_eq!(
            plain
                .post_json(&r_path("/p"), r#"{"a":[1],"b":1}"#)
                .unwrap()
                .status,
            201
        );
    }

    fn r_path(path: &str) -> HttpRequest {
        request(path, &[])
    }
}
