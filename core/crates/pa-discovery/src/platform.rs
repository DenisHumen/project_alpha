// SPDX-License-Identifier: MPL-2.0
//! Backend over a platform browser.
//!
//! On iOS and tvOS raw multicast needs an entitlement Apple grants by hand,
//! while Bonjour through `NWBrowser` only needs `NSLocalNetworkUsageDescription`
//! and the types in `NSBonjourServices` (see [`bonjour_services`]). The app
//! implements [`PlatformBrowser`] (UniFFI callback interface) and reports
//! results into the [`PlatformSink`] it is given.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};

use crate::discovery::{Backend, DiscoveryError, EventSink};
use crate::kind::ServiceKind;
use crate::service::{DiscoveredService, Txt};

/// A service as a platform browser reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformService {
    /// Instance name as shown by the platform (already unescaped).
    pub instance: String,
    /// Host name, if the platform resolved it (may be empty).
    pub host: String,
    /// Port, if resolved (0 when the platform only gives an endpoint).
    pub port: u16,
    /// Addresses, if resolved.
    pub addresses: Vec<IpAddr>,
    /// TXT entries; `None` value is a boolean attribute.
    pub txt: Vec<(String, Option<Vec<u8>>)>,
}

/// Callbacks the platform implements (`NWBrowser` on Apple platforms).
pub trait PlatformBrowser: Send + Sync {
    /// Starts browsing `service_type` (e.g. `_smb._tcp`, no domain) and
    /// reports into `sink` until [`PlatformBrowser::stop_browse`]. An error
    /// text is shown to the user (e.g. Local Network permission denied).
    fn start_browse(&self, service_type: String, sink: Arc<PlatformSink>) -> Result<(), String>;
    /// Stops browsing `service_type`.
    fn stop_browse(&self, service_type: String);
}

/// Where the platform reports results for one service type.
#[derive(Debug)]
pub struct PlatformSink {
    kind: ServiceKind,
    sink: EventSink,
}

impl PlatformSink {
    /// Service type this sink is for (`_smb._tcp`).
    pub fn service_type(&self) -> String {
        self.kind.service_type().to_owned()
    }

    /// An instance was found or its details changed.
    pub fn service_found(&self, s: PlatformService) {
        let instance = s.instance.trim();
        if instance.is_empty() {
            return;
        }
        self.sink.resolved(DiscoveredService::new(
            self.kind,
            instance,
            s.host,
            s.port,
            s.addresses,
            Txt::from_raw(s.txt),
        ));
    }

    /// An instance disappeared.
    pub fn service_lost(&self, instance: String) {
        self.sink.removed(self.kind, instance.trim());
    }
}

/// [`Backend`] over a [`PlatformBrowser`].
pub struct PlatformBackend {
    browser: Arc<dyn PlatformBrowser>,
    active: Mutex<HashMap<ServiceKind, Arc<PlatformSink>>>,
}

impl PlatformBackend {
    /// Wraps the platform browser.
    pub fn new(browser: Arc<dyn PlatformBrowser>) -> Self {
        Self {
            browser,
            active: Mutex::new(HashMap::new()),
        }
    }
}

impl Backend for PlatformBackend {
    fn start(&self, kind: ServiceKind, sink: EventSink) -> Result<(), DiscoveryError> {
        let sink = Arc::new(PlatformSink { kind, sink });
        self.browser
            .start_browse(kind.service_type().to_owned(), Arc::clone(&sink))
            .map_err(DiscoveryError::Platform)?;
        self.active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(kind, sink);
        Ok(())
    }

    fn stop(&self, kind: ServiceKind) {
        let removed = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&kind);
        if removed.is_some() {
            self.browser.stop_browse(kind.service_type().to_owned());
        }
    }

    fn shutdown(&self) {
        let kinds: Vec<_> = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        for kind in kinds {
            self.stop(kind);
        }
    }
}

/// Entries for `NSBonjourServices` in `Info.plist` (Apple platforms): every
/// type the core may browse or announce.
pub fn bonjour_services() -> Vec<&'static str> {
    ServiceKind::ALL.iter().map(|k| k.service_type()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Discovery;
    use crate::registry::DiscoveryEvent;
    use std::time::Duration;

    #[derive(Default)]
    struct FakeBrowser {
        sinks: Mutex<Vec<Arc<PlatformSink>>>,
        log: Mutex<Vec<String>>,
    }

    impl PlatformBrowser for FakeBrowser {
        fn start_browse(&self, service_type: String, sink: Arc<PlatformSink>) -> Result<(), String> {
            if service_type == "_nfs._tcp" {
                return Err("Local Network access denied".into());
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

    #[test]
    fn platform_reports_reach_subscribers() {
        let browser = Arc::new(FakeBrowser::default());
        let mut d = Discovery::new(PlatformBackend::new(browser.clone()), Vec::<String>::new());
        let rx = d.subscribe();
        d.browse([ServiceKind::Server]).unwrap();
        let err = d.browse([ServiceKind::Nfs]).unwrap_err();
        assert!(err.to_string().contains("denied"));

        let sink = browser.sinks.lock().unwrap()[0].clone();
        sink.service_found(PlatformService {
            instance: "  ".into(),
            ..Default::default()
        });
        sink.service_found(PlatformService {
            instance: "Home server".into(),
            port: 8096,
            addresses: vec!["192.168.1.10".parse().unwrap()],
            txt: vec![
                ("id".into(), Some(b"srv-1".to_vec())),
                ("v".into(), Some(b"1".to_vec())),
            ],
            ..Default::default()
        });
        let ev = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let DiscoveryEvent::Found(svc) = ev else { panic!("{ev:?}") };
        assert_eq!(svc.server_txt().unwrap().id, "srv-1");
        sink.service_lost("Home server".into());
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            DiscoveryEvent::Lost { .. }
        ));
        d.shutdown();
        assert_eq!(
            *browser.log.lock().unwrap(),
            ["start _projectalpha._tcp", "stop _projectalpha._tcp"]
        );
    }

    #[test]
    fn bonjour_list_matches_kinds() {
        let list = bonjour_services();
        assert_eq!(list.len(), ServiceKind::ALL.len());
        assert!(list.contains(&"_smb._tcp") && list.contains(&"_projectalpha._tcp"));
    }
}
