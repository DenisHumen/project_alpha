// SPDX-License-Identifier: MPL-2.0
//! Pure-Rust mDNS backend on [`mdns_sd`]: browsing and announcing.
//!
//! Used on Linux, Windows, macOS and Android. On Android the app must hold a
//! `WifiManager.MulticastLock` while browsing, otherwise the Wi-Fi driver
//! drops multicast: pass a [`MulticastLock`] implemented by the platform.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use mdns_sd::{IfKind, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::discovery::{Backend, DiscoveryError, EventSink};
use crate::kind::ServiceKind;
use crate::service::{
    DiscoveredService, LABEL_MAX, PeerTxt, ServerTxt, TXT_ENTRY_MAX, Txt, split_fullname,
};

/// Standard mDNS port.
pub const MDNS_PORT: u16 = 5353;

/// Platform hook that keeps multicast reception on while browsing
/// (Android `WifiManager.MulticastLock`). Calls are balanced: one
/// [`release`](MulticastLock::release) per [`acquire`](MulticastLock::acquire).
pub trait MulticastLock: Send + Sync {
    /// Called before the first browse starts.
    fn acquire(&self);
    /// Called after the last browse stops.
    fn release(&self);
}

/// Which network interfaces the daemon uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interfaces {
    /// Every interface (production).
    #[default]
    All,
    /// IPv4 loopback only: tests that must not touch the real network.
    LoopbackOnly,
}

/// Settings of [`MdnsBackend`] and [`Advertiser`].
#[derive(Clone, Default)]
pub struct MdnsConfig {
    /// UDP port; `None` is the standard [`MDNS_PORT`]. Tests use a private
    /// port so they do not talk to the system responder.
    pub port: Option<u16>,
    /// Interfaces to use.
    pub interfaces: Interfaces,
    /// Android multicast lock.
    pub multicast_lock: Option<Arc<dyn MulticastLock>>,
}

impl std::fmt::Debug for MdnsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdnsConfig")
            .field("port", &self.port)
            .field("interfaces", &self.interfaces)
            .field("multicast_lock", &self.multicast_lock.is_some())
            .finish()
    }
}

fn mdns_err(e: mdns_sd::Error) -> DiscoveryError {
    DiscoveryError::Mdns(e.to_string())
}

fn new_daemon(config: &MdnsConfig) -> Result<ServiceDaemon, DiscoveryError> {
    let daemon = ServiceDaemon::new_with_port(config.port.unwrap_or(MDNS_PORT)).map_err(mdns_err)?;
    if config.interfaces == Interfaces::LoopbackOnly {
        daemon.disable_interface(IfKind::All).map_err(mdns_err)?;
        daemon.enable_interface(IfKind::LoopbackV4).map_err(mdns_err)?;
    }
    Ok(daemon)
}

/// Browser on the pure-Rust mDNS daemon.
pub struct MdnsBackend {
    daemon: ServiceDaemon,
    lock: Option<Arc<dyn MulticastLock>>,
    browsing: Mutex<HashSet<ServiceKind>>,
}

impl MdnsBackend {
    /// Daemon on the standard port and every interface.
    pub fn new() -> Result<Self, DiscoveryError> {
        Self::with_config(MdnsConfig::default())
    }

    /// Daemon with explicit settings.
    pub fn with_config(config: MdnsConfig) -> Result<Self, DiscoveryError> {
        Ok(Self {
            daemon: new_daemon(&config)?,
            lock: config.multicast_lock,
            browsing: Mutex::new(HashSet::new()),
        })
    }

    /// An advertiser sharing this daemon (one socket set per process).
    pub fn advertiser(&self) -> Advertiser {
        Advertiser {
            daemon: self.daemon.clone(),
        }
    }
}

impl Backend for MdnsBackend {
    fn start(&self, kind: ServiceKind, sink: EventSink) -> Result<(), DiscoveryError> {
        let mut browsing = self.browsing.lock().unwrap_or_else(PoisonError::into_inner);
        if browsing.is_empty()
            && let Some(lock) = &self.lock
        {
            lock.acquire();
        }
        let rx = match self.daemon.browse(kind.service_type_local()) {
            Ok(rx) => rx,
            Err(e) => {
                if browsing.is_empty()
                    && let Some(lock) = &self.lock
                {
                    lock.release();
                }
                return Err(mdns_err(e));
            }
        };
        browsing.insert(kind);
        thread::Builder::new()
            .name(format!("pa-discovery-{kind}"))
            .spawn(move || {
                // Ends on SearchStopped or when the daemon drops the channel.
                while let Ok(event) = rx.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(resolved) => {
                            if let Some(svc) = convert(kind, &resolved) {
                                sink.resolved(svc);
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Some((instance, k)) = split_fullname(&fullname)
                                && k == kind
                            {
                                sink.removed(kind, instance);
                            }
                        }
                        ServiceEvent::SearchStopped(_) => break,
                        _ => {}
                    }
                }
            })
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        Ok(())
    }

    fn stop(&self, kind: ServiceKind) {
        let mut browsing = self.browsing.lock().unwrap_or_else(PoisonError::into_inner);
        if browsing.remove(&kind) {
            let _ = self.daemon.stop_browse(kind.service_type_local());
            if browsing.is_empty()
                && let Some(lock) = &self.lock
            {
                lock.release();
            }
        }
    }

    fn shutdown(&self) {
        let kinds: Vec<_> = self
            .browsing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .copied()
            .collect();
        for kind in kinds {
            self.stop(kind);
        }
        let _ = self.daemon.shutdown();
    }
}

/// Converts a resolved mDNS record set into our model; `None` for records
/// of another type or with an unparsable name.
fn convert(kind: ServiceKind, r: &ResolvedService) -> Option<DiscoveredService> {
    let (instance, k) = split_fullname(&r.fullname)?;
    if k != kind {
        return None;
    }
    let addresses: Vec<IpAddr> = r.addresses.iter().map(|ip| ip.to_ip_addr()).collect();
    let txt = Txt::from_raw(
        r.txt_properties
            .iter()
            .map(|p| (p.key().to_owned(), p.val().map(<[u8]>::to_vec))),
    );
    Some(DiscoveredService::new(kind, instance, r.host.clone(), r.port, addresses, txt))
}

/// A service to announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// Type.
    pub kind: ServiceKind,
    /// Instance name, human-readable (one DNS label, up to 63 bytes).
    pub instance: String,
    /// mDNS host name for the A/AAAA records, e.g. `pa-0192f1d0.local.`.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// TXT entries.
    pub txt: Vec<(String, String)>,
}

impl Announcement {
    /// `_projectalpha._tcp` for our server (used by SRV-004).
    pub fn server(instance: impl Into<String>, port: u16, txt: &ServerTxt) -> Self {
        Self {
            kind: ServiceKind::Server,
            instance: instance.into(),
            host: host_for_id(&txt.id),
            port,
            txt: txt.to_pairs(),
        }
    }

    /// `_alpha-sync._tcp` for a sync peer (used by SYNC-006).
    pub fn peer(instance: impl Into<String>, port: u16, txt: &PeerTxt) -> Self {
        Self {
            kind: ServiceKind::SyncPeer,
            instance: instance.into(),
            host: host_for_id(&txt.id),
            port,
            txt: txt.to_pairs(),
        }
    }

    /// Checks DNS-SD limits before anything goes on the wire.
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        let bad = |m: String| Err(DiscoveryError::InvalidAnnouncement(m));
        if self.instance.trim().is_empty() || self.instance.len() > LABEL_MAX {
            return bad(format!("instance name must be 1..={LABEL_MAX} bytes"));
        }
        if self.port == 0 {
            return bad("port must not be 0".into());
        }
        let host = self.host.strip_suffix(".local.").unwrap_or("");
        if host.is_empty()
            || host.len() > LABEL_MAX
            || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return bad(format!("host {:?} must be `<label>.local.`", self.host));
        }
        for (k, v) in &self.txt {
            if k.is_empty() || k.contains('=') || !k.is_ascii() {
                return bad(format!("TXT key {k:?} is invalid"));
            }
            if k.len() + 1 + v.len() > TXT_ENTRY_MAX {
                return bad(format!("TXT entry {k:?} exceeds {TXT_ENTRY_MAX} bytes"));
            }
        }
        Ok(())
    }
}

/// mDNS host label derived from an id: `pa-<first 12 alphanumerics>.local.`.
/// Distinct from the OS host name so we never answer for it.
pub fn host_for_id(id: &str) -> String {
    let label: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>()
        .to_ascii_lowercase();
    let label = if label.is_empty() { "device".to_owned() } else { label };
    format!("pa-{label}.local.")
}

/// Announces services on mDNS.
#[derive(Clone)]
pub struct Advertiser {
    daemon: ServiceDaemon,
}

impl Advertiser {
    /// Own daemon with explicit settings (a server that does not browse).
    pub fn with_config(config: &MdnsConfig) -> Result<Self, DiscoveryError> {
        Ok(Self {
            daemon: new_daemon(config)?,
        })
    }

    /// Starts announcing; the announcement lasts until the returned
    /// [`Registration`] is dropped (a goodbye packet is sent then).
    /// Addresses follow the host's interfaces automatically.
    pub fn announce(&self, a: &Announcement) -> Result<Registration, DiscoveryError> {
        a.validate()?;
        let info = ServiceInfo::new(
            a.kind.service_type_local(),
            &a.instance,
            &a.host,
            "",
            a.port,
            a.txt.as_slice(),
        )
        .map_err(mdns_err)?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_owned();
        self.daemon.register(info).map_err(mdns_err)?;
        Ok(Registration {
            daemon: self.daemon.clone(),
            fullname,
        })
    }

    /// Stops the daemon (and every registration on it).
    pub fn shutdown(&self) {
        let _ = self.daemon.shutdown();
    }
}

/// A live announcement; dropping it withdraws the service.
pub struct Registration {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Registration {
    /// DNS-SD full name as announced.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(rx) = self.daemon.unregister(&self.fullname) {
            // Wait briefly so the goodbye leaves before a process exits.
            let _ = rx.recv_timeout(std::time::Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_ann() -> Announcement {
        Announcement::server(
            "Home server",
            8096,
            &ServerTxt {
                id: "0192F1D0-AAAA-7000".into(),
                api_version: 1,
                name: None,
                fingerprint: None,
            },
        )
    }

    #[test]
    fn host_label_from_id() {
        assert_eq!(host_for_id("0192F1D0-AAAA-7000"), "pa-0192f1d0aaaa.local.");
        assert_eq!(host_for_id("--"), "pa-device.local.");
    }

    #[test]
    fn announcement_validation() {
        assert!(server_ann().validate().is_ok());
        let mut a = server_ann();
        a.instance = "x".repeat(64);
        assert!(a.validate().is_err());
        let mut a = server_ann();
        a.port = 0;
        assert!(a.validate().is_err());
        let mut a = server_ann();
        a.host = "bad host.local.".into();
        assert!(a.validate().is_err());
        let mut a = server_ann();
        a.txt.push(("big".into(), "v".repeat(252)));
        assert!(a.validate().is_err());
        let mut a = server_ann();
        a.txt.push(("a=b".into(), String::new()));
        assert!(a.validate().is_err());
    }
}
