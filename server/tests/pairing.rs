// SPDX-License-Identifier: MPL-2.0
//! SYNC-005: pairing devices with the server and the `unauthorized` path of
//! the sync hub. "Done when": a revoked device can no longer connect.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pa_pairing::pair::{Joiner, PairMessage, PairingCode, QrInvite};
use pa_pairing::{DeviceIdentity, DeviceKind, Family, FamilyUpdate};
use pa_server::{Config, Server};
use pa_sync::proto::{self, Auth, Hello, Message, ProtoRange, Role, code};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const WAIT: Duration = Duration::from_secs(5);

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), pa_server::Error>>,
    hub_id: String,
    pairing: pa_server::pairing::Pairing,
}

impl Running {
    async fn start(data: &Path) -> Self {
        let config = Config {
            listen: "127.0.0.1:0".parse().unwrap(),
            ..Config::default()
        };
        assert!(config.sync.require_pairing, "pairing is on by default");
        let server = Server::bind(&config, data).await.unwrap();
        let addr = server.local_addr().unwrap();
        let hub_id = server.hub().hub_id().to_owned();
        let pairing = server.hub().pairing().clone();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run(async {
            let _ = stopped.await;
        }));
        Self {
            addr,
            stop,
            task,
            hub_id,
            pairing,
        }
    }

    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

async fn ws(addr: SocketAddr, path: &str) -> Ws {
    let url = format!("ws://{addr}{path}");
    tokio_tungstenite::connect_async(url).await.unwrap().0
}

async fn next_text(ws: &mut Ws) -> Option<String> {
    loop {
        let msg = tokio::time::timeout(WAIT, ws.next())
            .await
            .expect("server answers in time")?;
        match msg {
            Ok(WsMessage::Text(text)) => return Some(text.as_str().to_owned()),
            Ok(WsMessage::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

async fn send_pair(ws: &mut Ws, msg: &PairMessage) {
    ws.send(WsMessage::Text(msg.encode().into())).await.unwrap();
}

async fn recv_pair(ws: &mut Ws) -> PairMessage {
    PairMessage::decode(&next_text(ws).await.expect("pairing frame")).unwrap()
}

/// Runs pairing as the joiner `device` with `code`.
async fn pair(
    addr: SocketAddr,
    identity: DeviceIdentity,
    code: &PairingCode,
    inviter: Option<[u8; 32]>,
) -> pa_pairing::Result<Family> {
    let mut ws = ws(addr, pa_server::pairing::PAIR_PATH).await;
    let (mut joiner, hello) =
        Joiner::start(&identity, "Test device", DeviceKind::Phone, code, inviter)?;
    send_pair(&mut ws, &hello).await;
    let theirs = recv_pair(&mut ws).await;
    let intro = joiner.intro(&identity, &theirs)?;
    send_pair(&mut ws, &intro).await;
    let welcome = recv_pair(&mut ws).await;
    joiner.finish(&welcome)?.into_family(identity)
}

async fn send(ws: &mut Ws, msg: &Message) {
    let frame = proto::encode(msg).unwrap();
    ws.send(WsMessage::Text(frame.into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> Option<Message> {
    next_text(ws).await.map(|t| proto::decode(&t).unwrap())
}

fn hello(device: &str, nonce: Option<String>) -> Message {
    Message::Hello(Hello {
        proto: ProtoRange::LOCAL,
        device_id: device.to_owned(),
        role: Role::Device,
        cursor: None,
        head: None,
        nonce,
    })
}

/// Connects to the hub as `identity`, checked against `roster`.
/// Returns the socket after the device's `auth` and whether the hub proved
/// itself.
async fn connect(
    addr: SocketAddr,
    identity: &DeviceIdentity,
    roster: &pa_pairing::Roster,
) -> (Ws, bool) {
    let mut ws = ws(addr, pa_server::sync::PATH).await;
    let nonce = pa_pairing::auth::nonce().unwrap();
    send(&mut ws, &hello(identity.device_id(), Some(nonce.clone()))).await;
    let Some(Message::Hello(hub)) = recv(&mut ws).await else {
        panic!("no hello from the hub")
    };
    let hub_nonce = hub.nonce.expect("hub sends a nonce");
    let sig = pa_pairing::auth::prove(identity, &hub.device_id, &nonce, &hub_nonce);
    send(&mut ws, &Message::Auth(Auth { sig })).await;
    let Some(Message::Auth(auth)) = recv(&mut ws).await else {
        panic!("no auth from the hub")
    };
    let hub_ok = pa_pairing::auth::verify(
        roster,
        &hub.device_id,
        identity.device_id(),
        &hub_nonce,
        &nonce,
        &auth.sig,
    )
    .is_ok();
    (ws, hub_ok)
}

/// The next `family` update, skipping other frames.
async fn family_update(ws: &mut Ws) -> FamilyUpdate {
    loop {
        match recv(ws).await {
            Some(Message::Family(f)) => return FamilyUpdate::from_json(&f.body).unwrap(),
            Some(_) => {}
            None => panic!("closed before a family update"),
        }
    }
}

/// Expects `error{unauthorized}` and then the end of the connection.
async fn expect_unauthorized(ws: &mut Ws) {
    loop {
        match recv(ws).await {
            Some(Message::Error(e)) => {
                assert_eq!(e.code, code::UNAUTHORIZED);
                break;
            }
            Some(Message::Family(_) | Message::Change(_) | Message::Auth(_)) => {}
            other => panic!("expected unauthorized, got {other:?}"),
        }
    }
    assert!(recv(ws).await.is_none(), "connection stays open");
}

/// A raw HTTP/1.1 request; returns the status and the body.
async fn http(addr: SocketAddr, method: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let status = response[9..12].parse().unwrap();
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoked_device_can_no_longer_connect() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Running::start(tmp.path()).await;

    // No invitation yet: pairing is refused.
    let code = PairingCode::parse("123456").unwrap();
    let err = pair(
        server.addr,
        DeviceIdentity::generate("early").unwrap(),
        &code,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, pa_pairing::Error::Expired), "{err:?}");

    // The phone pairs with a 6-digit code from the loopback API.
    let (status, body) = http(server.addr, "POST", "/api/v1/pair/invite").await;
    assert_eq!(status, 200, "{body}");
    let invite: serde_json::Value = serde_json::from_str(&body).unwrap();
    let digits = PairingCode::parse(invite["code"].as_str().unwrap()).unwrap();
    assert_eq!(digits.expose().len(), 6);
    // A wrong code costs one attempt and pairs nothing.
    let wrong = PairingCode::parse(if digits.expose() == "000000" {
        "000001"
    } else {
        "000000"
    })
    .unwrap();
    let err = pair(
        server.addr,
        DeviceIdentity::generate("guess").unwrap(),
        &wrong,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, pa_pairing::Error::WrongCode), "{err:?}");
    let phone = pair(
        server.addr,
        DeviceIdentity::generate("phone").unwrap(),
        &digits,
        None,
    )
    .await
    .unwrap();
    // The invitation is used up.
    let err = pair(
        server.addr,
        DeviceIdentity::generate("late").unwrap(),
        &digits,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, pa_pairing::Error::Expired), "{err:?}");

    // The TV pairs by QR code, which pins the server's key.
    let invite = server.pairing.invite(true).unwrap();
    let qr = QrInvite::parse(&invite.qr).unwrap();
    assert_eq!(qr.addr.as_deref(), Some(server.addr.to_string().as_str()));
    let tv = pair(
        server.addr,
        DeviceIdentity::generate("tv").unwrap(),
        &qr.code,
        Some(qr.inviter),
    )
    .await
    .unwrap();
    assert_eq!(tv.key().expose(), phone.key().expose());

    // Paired devices connect; both sides prove themselves.
    let (mut phone_ws, hub_ok) = connect(server.addr, phone.identity(), phone.roster()).await;
    assert!(hub_ok, "the hub proves its key");
    let update = family_update(&mut phone_ws).await;
    assert!(update.roster.authorize("tv").is_ok());
    let (mut tv_ws, _) = connect(server.addr, tv.identity(), tv.roster()).await;
    family_update(&mut tv_ws).await;

    // Unpaired devices are refused: without a proof, and with a key that is
    // not in the family (also under a paired device's id).
    let mut ws_plain = ws(server.addr, pa_server::sync::PATH).await;
    send(&mut ws_plain, &hello("pc", None)).await;
    recv(&mut ws_plain).await.unwrap();
    expect_unauthorized(&mut ws_plain).await;
    for id in ["pc", "tv"] {
        let stranger = DeviceIdentity::generate(id).unwrap();
        let (mut ws, hub_ok) = connect(server.addr, &stranger, phone.roster()).await;
        assert!(hub_ok);
        expect_unauthorized(&mut ws).await;
    }

    // The device list shows both; the TV is revoked.
    let (status, body) = http(server.addr, "GET", "/api/v1/devices").await;
    assert_eq!(status, 200);
    let list: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list["devices"].as_array().unwrap().len(), 3);
    let (status, _) = http(server.addr, "DELETE", "/api/v1/devices/tv").await;
    assert_eq!(status, 204);
    let (status, _) = http(server.addr, "DELETE", "/api/v1/devices/tv").await;
    assert_eq!(status, 410);
    let (status, _) = http(server.addr, "DELETE", "/api/v1/devices/nobody").await;
    assert_eq!(status, 404);

    // The connected TV is dropped; the phone gets the rotated key.
    expect_unauthorized(&mut tv_ws).await;
    let mut phone = phone;
    let update = family_update(&mut phone_ws).await;
    let report = phone.apply_update(&update).unwrap();
    assert!(report.key_rotated);
    assert_eq!(phone.key().epoch(), 2);
    assert_ne!(phone.key().expose(), tv.key().expose());

    // The revoked TV cannot connect again, not even after a restart.
    let (mut ws, _) = connect(server.addr, tv.identity(), tv.roster()).await;
    expect_unauthorized(&mut ws).await;
    drop(phone_ws);
    server.stop().await;

    let server = Running::start(tmp.path()).await;
    let (mut ws, _) = connect(server.addr, tv.identity(), tv.roster()).await;
    expect_unauthorized(&mut ws).await;
    let (mut ws, hub_ok) = connect(server.addr, phone.identity(), phone.roster()).await;
    assert!(hub_ok);
    let update = family_update(&mut ws).await;
    assert_eq!(update.key.unwrap().epoch, 2);
    assert!(!server.hub_id.is_empty());
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_can_revoke_through_the_hub() {
    let tmp = tempfile::tempdir().unwrap();
    let server = Running::start(tmp.path()).await;
    let mut members = Vec::new();
    for id in ["phone", "tv"] {
        let invite = server.pairing.invite(false).unwrap();
        let code = PairingCode::parse(&invite.code).unwrap();
        members.push(
            pair(
                server.addr,
                DeviceIdentity::generate(id).unwrap(),
                &code,
                None,
            )
            .await
            .unwrap(),
        );
    }
    let tv = members.pop().unwrap();
    let mut phone = members.pop().unwrap();
    let (mut tv_ws, _) = connect(server.addr, tv.identity(), tv.roster()).await;
    family_update(&mut tv_ws).await;

    // The phone learns about the TV, revokes it and tells the hub.
    let (mut phone_ws, _) = connect(server.addr, phone.identity(), phone.roster()).await;
    let update = family_update(&mut phone_ws).await;
    phone.apply_update(&update).unwrap();
    phone.revoke("tv", u64::MAX / 2).unwrap();
    let body = phone.update_for(&server.hub_id).unwrap().to_json();
    send(&mut phone_ws, &Message::Family(proto::Family { body })).await;

    expect_unauthorized(&mut tv_ws).await;
    let (mut ws, _) = connect(server.addr, tv.identity(), tv.roster()).await;
    expect_unauthorized(&mut ws).await;
    let devices = server.pairing.devices();
    let tv_row = devices.iter().find(|d| d.device_id == "tv").unwrap();
    assert!(tv_row.revoked_ms.is_some());
    server.stop().await;
}
