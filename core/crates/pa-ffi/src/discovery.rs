// SPDX-License-Identifier: MPL-2.0
//! LAN discovery over FFI for iOS and tvOS (NET-020, ADR-0019).
//!
//! Raw multicast on iOS and tvOS needs an entitlement Apple grants by hand,
//! so the app browses with `NWBrowser` and reports into the core: it
//! implements [`PlatformBrowser`] (`NWBrowserPlatformBrowser` in `PACore`)
//! and passes it to [`LanDiscovery::new`]. The core deduplicates, keeps the
//! picture and tells a [`DiscoveryListener`] the current list.
//!
//! `Info.plist` needs `NSLocalNetworkUsageDescription` and every type of
//! [`bonjour_services`] in `NSBonjourServices`.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use pa_discovery::{
    DiscoveredService, Discovery, DiscoveryError, DiscoveryEvent, PlatformBackend, ServiceKind,
};

/// Error of a discovery call.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum LanDiscoveryError {
    /// The platform browser refused to start (Local Network access denied).
    #[error("platform browser: {details}")]
    Platform {
        /// Details for the user and logs.
        details: String,
    },
    /// [`LanDiscovery::close`] was called.
    #[error("discovery is closed")]
    Closed,
}

impl From<uniffi::UnexpectedUniFFICallbackError> for LanDiscoveryError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Platform {
            details: err.reason,
        }
    }
}

impl From<DiscoveryError> for LanDiscoveryError {
    fn from(e: DiscoveryError) -> Self {
        match e {
            DiscoveryError::Shutdown => Self::Closed,
            DiscoveryError::Platform(details) => Self::Platform { details },
            other => Self::Platform {
                details: other.to_string(),
            },
        }
    }
}

/// A DNS-SD service type the core knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, uniffi::Enum)]
pub enum LanServiceKind {
    /// Our media server, `_projectalpha._tcp`.
    Server,
    /// Another app for LAN sync, `_alpha-sync._tcp`.
    SyncPeer,
    /// An app showing a pairing invitation, `_alpha-pair._tcp`.
    Pairing,
    /// SMB share, `_smb._tcp`.
    Smb,
    /// WebDAV over HTTP, `_webdav._tcp`.
    WebDav,
    /// WebDAV over HTTPS, `_webdavs._tcp`.
    WebDavs,
    /// NFS export, `_nfs._tcp`.
    Nfs,
}

impl From<LanServiceKind> for ServiceKind {
    fn from(k: LanServiceKind) -> Self {
        match k {
            LanServiceKind::Server => Self::Server,
            LanServiceKind::SyncPeer => Self::SyncPeer,
            LanServiceKind::Pairing => Self::Pairing,
            LanServiceKind::Smb => Self::Smb,
            LanServiceKind::WebDav => Self::WebDav,
            LanServiceKind::WebDavs => Self::WebDavs,
            LanServiceKind::Nfs => Self::Nfs,
        }
    }
}

impl From<ServiceKind> for LanServiceKind {
    fn from(k: ServiceKind) -> Self {
        match k {
            ServiceKind::Server => Self::Server,
            ServiceKind::SyncPeer => Self::SyncPeer,
            ServiceKind::Pairing => Self::Pairing,
            ServiceKind::Smb => Self::Smb,
            ServiceKind::WebDav => Self::WebDav,
            ServiceKind::WebDavs => Self::WebDavs,
            ServiceKind::Nfs => Self::Nfs,
        }
    }
}

/// Entries for `NSBonjourServices` in `Info.plist`: every type the core may
/// browse or announce (`_smb._tcp`, …).
#[uniffi::export]
pub fn bonjour_services() -> Vec<String> {
    pa_discovery::bonjour_services()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Kinds the add-source flow browses: SMB, WebDAV and our server.
#[uniffi::export]
pub fn discovery_source_kinds() -> Vec<LanServiceKind> {
    vec![
        LanServiceKind::Smb,
        LanServiceKind::WebDav,
        LanServiceKind::WebDavs,
        LanServiceKind::Server,
    ]
}

/// A TXT entry; `value` is `None` for a boolean attribute.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TxtEntry {
    pub key: String,
    pub value: Option<Vec<u8>>,
}

/// A service as the platform browser reports it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlatformService {
    /// Instance name as shown by the platform (already unescaped).
    pub instance: String,
    /// Host name if resolved, else empty.
    pub host: String,
    /// Port if resolved, else 0.
    pub port: u16,
    /// Addresses if resolved (`192.168.1.2`, `fe80::1%en0`); unparsable
    /// ones are skipped.
    pub addresses: Vec<String>,
    /// TXT record.
    pub txt: Vec<TxtEntry>,
}

/// Parses an address as platforms print it: an IPv6 zone (`%en0`) and
/// brackets are dropped.
fn parse_address(s: &str) -> Option<IpAddr> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let s = s.split('%').next().unwrap_or(s);
    s.parse().ok()
}

/// Where the platform reports the results of one service type.
#[derive(uniffi::Object)]
pub struct PlatformSink {
    inner: Arc<pa_discovery::PlatformSink>,
}

#[uniffi::export]
impl PlatformSink {
    /// The service type this sink is for (`_smb._tcp`).
    pub fn service_type(&self) -> String {
        self.inner.service_type()
    }

    /// An instance was found, resolved or changed.
    pub fn service_found(&self, service: PlatformService) {
        self.inner.service_found(pa_discovery::PlatformService {
            instance: service.instance,
            host: service.host,
            port: service.port,
            addresses: service
                .addresses
                .iter()
                .filter_map(|a| parse_address(a))
                .collect(),
            txt: service.txt.into_iter().map(|e| (e.key, e.value)).collect(),
        });
    }

    /// An instance disappeared.
    pub fn service_lost(&self, instance: String) {
        self.inner.service_lost(instance);
    }
}

/// The platform browser: `NWBrowser` on iOS and tvOS.
#[uniffi::export(with_foreign)]
pub trait PlatformBrowser: Send + Sync {
    /// Starts browsing `service_type` (`_smb._tcp`, no domain) and reports
    /// into `sink` until [`PlatformBrowser::stop_browse`].
    fn start_browse(
        &self,
        service_type: String,
        sink: Arc<PlatformSink>,
    ) -> Result<(), LanDiscoveryError>;
    /// Stops browsing `service_type`.
    fn stop_browse(&self, service_type: String);
}

struct BrowserAdapter(Arc<dyn PlatformBrowser>);

impl pa_discovery::PlatformBrowser for BrowserAdapter {
    fn start_browse(
        &self,
        service_type: String,
        sink: Arc<pa_discovery::PlatformSink>,
    ) -> Result<(), String> {
        self.0
            .start_browse(service_type, Arc::new(PlatformSink { inner: sink }))
            .map_err(|e| match e {
                LanDiscoveryError::Platform { details } => details,
                other => other.to_string(),
            })
    }

    fn stop_browse(&self, service_type: String) {
        self.0.stop_browse(service_type);
    }
}

/// A service visible on the LAN.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LanService {
    pub kind: LanServiceKind,
    /// Instance name ("Living room NAS").
    pub instance: String,
    /// Host without the trailing dot; may be empty.
    pub host: String,
    /// Port; 0 until the platform resolved the service.
    pub port: u16,
    /// Addresses, best first.
    pub addresses: Vec<String>,
    /// TXT `path` of WebDAV and NFS, starting with `/`.
    pub path: Option<String>,
    /// Address of a file share for the source form
    /// (`smb://192.168.1.2/`); `None` before an address is known.
    pub url: Option<String>,
    /// TXT `id` of our server or another app.
    pub id: Option<String>,
}

impl From<&DiscoveredService> for LanService {
    fn from(s: &DiscoveredService) -> Self {
        Self {
            kind: s.kind.into(),
            instance: s.instance.clone(),
            host: s.host.trim_end_matches('.').to_owned(),
            port: s.port,
            addresses: s.addresses.iter().map(ToString::to_string).collect(),
            path: matches!(
                s.kind,
                ServiceKind::WebDav | ServiceKind::WebDavs | ServiceKind::Nfs
            )
            .then(|| s.path()),
            url: (s.port != 0).then(|| s.share_url()).flatten(),
            id: s.id().map(str::to_owned),
        }
    }
}

/// Told the whole list of visible services after every change.
#[uniffi::export(with_foreign)]
pub trait DiscoveryListener: Send + Sync {
    /// The current list, sorted by kind and name. Called on a core thread.
    fn services_changed(&self, services: Vec<LanService>);
}

fn sort(list: &mut [LanService]) {
    list.sort_by_cached_key(|s| (s.kind, s.instance.to_lowercase()));
}

/// LAN discovery of the app over its platform browser.
#[derive(uniffi::Object)]
pub struct LanDiscovery {
    inner: Mutex<Option<Discovery>>,
}

#[uniffi::export]
impl LanDiscovery {
    /// Discovery over `browser`. `own_ids` are TXT ids of this app, never
    /// reported as found. Nothing is browsed until [`browse`](Self::browse).
    #[uniffi::constructor]
    pub fn new(browser: Arc<dyn PlatformBrowser>, own_ids: Vec<String>) -> Arc<Self> {
        let backend = PlatformBackend::new(Arc::new(BrowserAdapter(browser)));
        Arc::new(Self {
            inner: Mutex::new(Some(Discovery::new(backend, own_ids))),
        })
    }

    /// Starts browsing `kinds` (already browsed kinds are skipped).
    pub fn browse(&self, kinds: Vec<LanServiceKind>) -> Result<(), LanDiscoveryError> {
        let guard = self.lock();
        let d = guard.as_ref().ok_or(LanDiscoveryError::Closed)?;
        Ok(d.browse(kinds.into_iter().map(Into::into))?)
    }

    /// Stops browsing `kinds`; their instances are reported lost.
    pub fn stop(&self, kinds: Vec<LanServiceKind>) {
        if let Some(d) = self.lock().as_ref() {
            for kind in kinds {
                d.stop(kind.into());
            }
        }
    }

    /// Snapshot of the visible services, sorted by kind and name.
    pub fn services(&self) -> Vec<LanService> {
        let mut list: Vec<LanService> = self
            .lock()
            .as_ref()
            .map(|d| d.services().iter().map(Into::into).collect())
            .unwrap_or_default();
        sort(&mut list);
        list
    }

    /// Calls `listener` with the current list at once and after every
    /// change, until [`close`](Self::close).
    pub fn listen(&self, listener: Arc<dyn DiscoveryListener>) -> Result<(), LanDiscoveryError> {
        let rx = self
            .lock()
            .as_ref()
            .ok_or(LanDiscoveryError::Closed)?
            .subscribe();
        spawn_snapshots(rx, listener);
        Ok(())
    }

    /// Stops every browse; listeners get no more calls.
    pub fn close(&self) {
        let taken = self.lock().take();
        if let Some(mut d) = taken {
            d.shutdown();
        }
    }
}

impl LanDiscovery {
    fn lock(&self) -> MutexGuard<'_, Option<Discovery>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn spawn_snapshots(rx: Receiver<DiscoveryEvent>, listener: Arc<dyn DiscoveryListener>) {
    let spawned = thread::Builder::new()
        .name("pa-discovery-listen".into())
        .spawn(move || {
            let mut visible: BTreeMap<(ServiceKind, String), LanService> = BTreeMap::new();
            let mut apply = |ev: DiscoveryEvent| match ev {
                DiscoveryEvent::Found(s) | DiscoveryEvent::Updated(s) => {
                    visible.insert(s.key(), (&s).into());
                }
                DiscoveryEvent::Lost { kind, instance } => {
                    visible.remove(&(kind, instance.to_lowercase()));
                }
            };
            let mut next = None;
            let mut changed = true;
            loop {
                if let Some(ev) = next.take() {
                    apply(ev);
                    changed = true;
                }
                while let Ok(ev) = rx.try_recv() {
                    apply(ev);
                    changed = true;
                }
                if changed {
                    let mut list: Vec<_> = visible.values().cloned().collect();
                    sort(&mut list);
                    listener.services_changed(list);
                    changed = false;
                }
                match rx.recv() {
                    Ok(ev) => next = Some(ev),
                    Err(_) => return,
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("pa-discovery: no listener thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// Plays `NWBrowser`: hands the sinks to the test.
    #[derive(Default)]
    struct FakeBrowser {
        sinks: Mutex<Vec<Arc<PlatformSink>>>,
        log: Mutex<Vec<String>>,
    }

    impl PlatformBrowser for FakeBrowser {
        fn start_browse(
            &self,
            service_type: String,
            sink: Arc<PlatformSink>,
        ) -> Result<(), LanDiscoveryError> {
            if service_type == "_nfs._tcp" {
                return Err(LanDiscoveryError::Platform {
                    details: "Local Network access denied".into(),
                });
            }
            assert_eq!(sink.service_type(), service_type);
            self.log.lock().unwrap().push(format!("start {service_type}"));
            self.sinks.lock().unwrap().push(sink);
            Ok(())
        }

        fn stop_browse(&self, service_type: String) {
            self.log.lock().unwrap().push(format!("stop {service_type}"));
        }
    }

    struct ChannelListener(Mutex<mpsc::Sender<Vec<LanService>>>);

    impl DiscoveryListener for ChannelListener {
        fn services_changed(&self, services: Vec<LanService>) {
            let _ = self.0.lock().unwrap().send(services);
        }
    }

    #[test]
    fn addresses_as_platforms_print_them() {
        assert_eq!(parse_address("192.168.1.2"), "192.168.1.2".parse().ok());
        assert_eq!(parse_address("fe80::1%en0"), "fe80::1".parse().ok());
        assert_eq!(parse_address("[fd00::2]"), "fd00::2".parse().ok());
        assert_eq!(parse_address("nas.local"), None);
    }

    #[test]
    fn nwbrowser_reports_become_services() {
        let browser = Arc::new(FakeBrowser::default());
        let d = LanDiscovery::new(browser.clone(), vec!["me".into()]);
        let (tx, rx) = mpsc::channel();
        d.listen(Arc::new(ChannelListener(Mutex::new(tx)))).unwrap();
        let next = || rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(next().is_empty());

        d.browse(discovery_source_kinds()).unwrap();
        assert!(matches!(
            d.browse(vec![LanServiceKind::Nfs]),
            Err(LanDiscoveryError::Platform { details }) if details.contains("denied")
        ));
        let smb = browser.sinks.lock().unwrap()[0].clone();
        assert_eq!(smb.service_type(), "_smb._tcp");

        // NWBrowser first gives the name and TXT, the address comes from a
        // connection later.
        smb.service_found(PlatformService {
            instance: "Living room NAS".into(),
            host: String::new(),
            port: 0,
            addresses: vec![],
            txt: vec![TxtEntry {
                key: "model".into(),
                value: Some(b"Samba".to_vec()),
            }],
        });
        let list = next();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].url, None);
        smb.service_found(PlatformService {
            instance: "Living room NAS".into(),
            host: "nas.local.".into(),
            port: 445,
            addresses: vec!["fe80::1%en0".into(), "192.168.1.20".into()],
            txt: vec![],
        });
        let list = next();
        assert_eq!(list[0].kind, LanServiceKind::Smb);
        assert_eq!(list[0].host, "nas.local");
        assert_eq!(list[0].addresses, ["192.168.1.20", "fe80::1"]);
        assert_eq!(list[0].url.as_deref(), Some("smb://192.168.1.20/"));
        assert_eq!(d.services(), list);

        smb.service_lost("Living room NAS".into());
        assert!(next().is_empty());
        d.close();
        assert!(matches!(d.browse(vec![]), Err(LanDiscoveryError::Closed)));
        let log = browser.log.lock().unwrap().clone();
        assert_eq!(log.len(), 8, "{log:?}");
        assert!(log.contains(&"stop _smb._tcp".to_owned()));
    }

    /// The tvOS app declares every type the core may browse (ADR-0019:
    /// `NSBonjourServices` from `bonjour_services()`), and a usage text.
    #[test]
    fn tvos_info_plist_lists_bonjour_services() {
        let spec = include_str!("../../../../clients/apple/TVApp/project.yml");
        assert!(spec.contains("NSLocalNetworkUsageDescription"));
        for ty in bonjour_services() {
            assert!(
                spec.contains(&format!("- {ty}")),
                "NSBonjourServices in TVApp/project.yml lacks {ty}"
            );
        }
        for kind in ServiceKind::ALL {
            assert_eq!(ServiceKind::from(LanServiceKind::from(kind)), kind);
        }
    }
}
