// SPDX-License-Identifier: MPL-2.0
//! SRV-004 "done when": a client finds the server over mDNS and rejects a
//! substituted certificate.
//!
//! mDNS runs on the IPv4 loopback and a private UDP port (not 5353), as in
//! the `pa-discovery` loopback tests, so nothing leaves the host.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pa_discovery::{
    Discovery, DiscoveryEvent, Fingerprint, Interfaces, MdnsBackend, MdnsConfig, PinnedVerifier,
    ServiceKind,
};
use pa_server::config::DiscoveryConfig;
use pa_server::{Config, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn private_port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let n = nanos.wrapping_add(NEXT.fetch_add(7919, Ordering::Relaxed)) ^ std::process::id();
    30_000 + u16::try_from(n % 20_000).unwrap()
}

fn loopback_mdns(port: u16) -> MdnsConfig {
    MdnsConfig {
        port: Some(port),
        interfaces: Interfaces::LoopbackOnly,
        multicast_lock: None,
    }
}

/// GET over TLS that trusts only `pin`. Returns the status code.
async fn get_pinned(addr: SocketAddr, pin: Fingerprint, path: &str) -> std::io::Result<u16> {
    let config = PinnedVerifier::client_config(pin).unwrap();
    let connector = TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(addr).await?;
    // The name is not checked by the pinned verifier.
    let name = ServerName::try_from("pa-server.local").unwrap();
    let mut tls = connector.connect(name, tcp).await?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    tls.write_all(request.as_bytes()).await?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match tls.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    Ok(text.split(' ').nth(1).unwrap().parse().unwrap())
}

/// A TLS endpoint with a certificate of its own, pretending to be the
/// server (same name, same port semantics, different key).
async fn impostor() -> SocketAddr {
    let pair = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .self_signed(&pair)
        .unwrap();
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = tokio_rustls::rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.der().to_vec())],
            PrivateKeyDer::try_from(pair.serialize_der()).unwrap(),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(mut tls) = acceptor.accept(tcp).await {
                    let _ = tls
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .await;
                }
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_finds_server_and_rejects_substituted_certificate() {
    let tmp = tempfile::tempdir().unwrap();
    let mdns_port = private_port();
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        discovery: DiscoveryConfig {
            enabled: true,
            name: Some("Test server".to_owned()),
        },
        ..Config::default()
    };
    let server = Server::bind(&config, tmp.path())
        .await
        .unwrap()
        .with_mdns(loopback_mdns(mdns_port));
    let addr = server.local_addr().unwrap();
    let server_id = server.server_id().to_owned();
    let fingerprint = server.fingerprint().expect("TLS is on by default");
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));

    // 1. The client finds the server on mDNS, with the fingerprint in TXT.
    let found = tokio::task::spawn_blocking(move || {
        let backend = MdnsBackend::with_config(loopback_mdns(mdns_port)).unwrap();
        let discovery = Discovery::new(backend, Vec::<String>::new());
        let rx = discovery.subscribe();
        discovery.browse([ServiceKind::Server]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left).expect("server announced in time") {
                DiscoveryEvent::Found(s) if s.kind == ServiceKind::Server => break s,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let txt = found.server_txt().unwrap();
    assert_eq!(txt.id, server_id);
    assert_eq!(txt.api_version, pa_server::announce::API_VERSION);
    assert_eq!(txt.name.as_deref(), Some("Test server"));
    assert_eq!(found.instance, "Test server");
    assert_eq!(found.port, addr.port());
    let announced: Fingerprint = txt.fingerprint.as_deref().unwrap().parse().unwrap();
    assert_eq!(announced, fingerprint);
    assert!(found.addresses.iter().any(IpAddr::is_loopback));

    // 2. The pinned client talks to the real server.
    let target = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), found.port);
    assert_eq!(get_pinned(target, announced, "/health").await.unwrap(), 200);

    // 3. A substituted certificate fails the handshake.
    let fake = impostor().await;
    let err = get_pinned(fake, announced, "/health").await.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("certificate"),
        "unexpected error: {err}"
    );
    // ...and so does the real server for a client holding another pin.
    let other = Fingerprint::of_der(b"some other certificate");
    assert!(get_pinned(target, other, "/health").await.is_err());

    // Plain HTTP on the TLS port gets nowhere.
    let mut tcp = TcpStream::connect(target).await.unwrap();
    tcp.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut buf)).await;
    assert!(!String::from_utf8_lossy(&buf).contains("\"status\":\"ok\""));

    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn certificate_survives_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        ..Config::default()
    };
    let first = Server::bind(&config, tmp.path()).await.unwrap();
    let (id, fp) = (first.server_id().to_owned(), first.fingerprint());
    drop(first);
    let second = Server::bind(&config, tmp.path()).await.unwrap();
    assert_eq!(second.server_id(), id);
    assert_eq!(second.fingerprint(), fp);
    assert!(fp.is_some());
}
