// SPDX-License-Identifier: MPL-2.0
//! SRV-001 "done when": the server starts on an empty data directory and
//! answers `/health`; it shuts down gracefully and starts again on the same
//! database.

use std::net::SocketAddr;

use pa_runtime::{PlatformDirs, Runtime, RuntimeConfig};
use pa_server::{Config, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

/// Minimal HTTP/1.1 GET: no HTTP client crate (and no TLS stack) needed.
async fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, body.to_owned())
}

fn config() -> Config {
    Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        // Plain HTTP: these tests speak raw HTTP/WebSocket (TLS: tls_mdns.rs).
        tls: pa_server::config::TlsConfig {
            enabled: false,
            ..Default::default()
        },
        ..Config::default()
    }
}

/// Starts the server, runs `check` against it, then stops it.
async fn with_server<F, Fut>(config: &Config, data: &std::path::Path, check: F)
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: Future<Output = ()>,
{
    let server = Server::bind(config, data).await.unwrap();
    let addr = server.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));
    check(addr).await;
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    // The listener is closed after shutdown.
    assert!(TcpStream::connect(addr).await.is_err());
}

#[test]
fn starts_on_empty_db_and_serves_health() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("state");
    let dirs = PlatformDirs::new(&data, &data, data.join("cache"));
    let runtime = Runtime::start(RuntimeConfig::default().with_dirs(dirs)).unwrap();
    let config = config();

    runtime
        .block_on(async {
            with_server(&config, &data, |addr| async move {
                for path in ["/health", "/healthz"] {
                    let (status, body) = get(addr, path).await;
                    assert_eq!(status, 200, "{path}: {body}");
                    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
                    assert_eq!(json["status"], "ok");
                    assert_eq!(json["service"], pa_server::SERVICE);
                    assert_eq!(json["version"], pa_server::VERSION);
                    assert_eq!(json["db"]["status"], "ok");
                    assert_eq!(json["db"]["schema_version"], pa_library::latest_version());
                }
                assert_eq!(get(addr, "/nope").await.0, 404);
            })
            .await;
        })
        .unwrap();

    let db = data.join(pa_server::config::LIBRARY_FILE);
    assert!(db.is_file());
    // Graceful shutdown checkpointed the WAL into the main file.
    let wal = data.join(format!("{}-wal", pa_server::config::LIBRARY_FILE));
    assert!(!wal.exists() || std::fs::metadata(&wal).unwrap().len() == 0);

    // Second start on the same database: migrations are already applied.
    runtime
        .block_on(async {
            with_server(&config, &data, |addr| async move {
                assert_eq!(get(addr, "/health").await.0, 200);
            })
            .await;
        })
        .unwrap();
    runtime.shutdown().unwrap();
}

#[test]
fn bind_fails_on_busy_port() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let first = Server::bind(&config(), tmp.path()).await.unwrap();
        let busy = Config {
            listen: first.local_addr().unwrap(),
            ..Config::default()
        };
        let other = tmp.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let err = Server::bind(&busy, &other).await.err().unwrap();
        assert!(matches!(err, pa_server::Error::Io { .. }), "{err}");
    });
}
