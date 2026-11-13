// SPDX-License-Identifier: MPL-2.0
//! Sans-IO state of what is visible in the LAN: turns raw backend reports
//! (repeated resolves, removals, re-announcements) into clean
//! found/updated/lost events.

use std::collections::{BTreeMap, HashSet};

use crate::kind::ServiceKind;
use crate::service::DiscoveredService;

/// A change in the set of visible services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A new instance appeared.
    Found(DiscoveredService),
    /// A known instance changed its host, port, addresses or TXT.
    Updated(DiscoveredService),
    /// An instance went away (goodbye packet, TTL expiry, or its type is no
    /// longer browsed).
    Lost {
        /// Its type.
        kind: ServiceKind,
        /// Its instance name, as last seen.
        instance: String,
    },
}

/// A raw report from a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawEvent {
    /// The backend resolved an instance (possibly again).
    Resolved(DiscoveredService),
    /// The backend says an instance is gone.
    Removed {
        /// Its type.
        kind: ServiceKind,
        /// Its unescaped instance name.
        instance: String,
    },
}

/// The visible services and the rules for reporting changes.
#[derive(Debug, Default)]
pub struct Registry {
    services: BTreeMap<(ServiceKind, String), DiscoveredService>,
    ignored_ids: HashSet<String>,
}

impl Registry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ignores announcements whose TXT `id` is one of `ids` (our own server
    /// or device seeing its own announcement).
    pub fn with_ignored_ids<I: Into<String>>(mut self, ids: impl IntoIterator<Item = I>) -> Self {
        self.ignored_ids.extend(ids.into_iter().map(Into::into));
        self
    }

    /// Applies a raw report; returns the event to publish, if any.
    pub fn apply(&mut self, raw: RawEvent) -> Option<DiscoveryEvent> {
        match raw {
            RawEvent::Resolved(svc) => {
                if svc.id().is_some_and(|id| self.ignored_ids.contains(id)) {
                    return None;
                }
                match self.services.insert(svc.key(), svc.clone()) {
                    None => Some(DiscoveryEvent::Found(svc)),
                    Some(old) if old == svc => None,
                    Some(_) => Some(DiscoveryEvent::Updated(svc)),
                }
            }
            RawEvent::Removed { kind, instance } => self
                .services
                .remove(&(kind, instance.to_lowercase()))
                .map(|old| DiscoveryEvent::Lost {
                    kind,
                    instance: old.instance,
                }),
        }
    }

    /// Forgets every instance of `kind` (its browse stopped).
    pub fn clear_kind(&mut self, kind: ServiceKind) -> Vec<DiscoveryEvent> {
        let keys: Vec<_> = self.services.keys().filter(|(k, _)| *k == kind).cloned().collect();
        keys.into_iter()
            .filter_map(|key| self.services.remove(&key))
            .map(|old| DiscoveryEvent::Lost {
                kind,
                instance: old.instance,
            })
            .collect()
    }

    /// All visible services, ordered by kind and instance name.
    pub fn services(&self) -> impl Iterator<Item = &DiscoveredService> {
        self.services.values()
    }

    /// Visible services of one kind.
    pub fn services_of(&self, kind: ServiceKind) -> impl Iterator<Item = &DiscoveredService> {
        self.services.values().filter(move |s| s.kind == kind)
    }

    /// The server with TXT `id` = `id`, if visible.
    pub fn server_by_id(&self, id: &str) -> Option<&DiscoveredService> {
        self.services_of(ServiceKind::Server).find(|s| s.id() == Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Txt;
    use std::net::IpAddr;

    fn svc(kind: ServiceKind, name: &str, port: u16, ip: &str, id: Option<&str>) -> DiscoveredService {
        let txt = Txt::from_pairs(id.map(|i| ("id", i)));
        DiscoveredService::new(kind, name, "host.local.", port, [ip.parse::<IpAddr>().unwrap()], txt)
    }

    #[test]
    fn found_duplicate_updated_lost() {
        let mut r = Registry::new();
        let a = svc(ServiceKind::Smb, "NAS", 445, "10.0.0.2", None);
        assert_eq!(r.apply(RawEvent::Resolved(a.clone())), Some(DiscoveryEvent::Found(a.clone())));
        // Re-announcement with the same data is silent.
        assert_eq!(r.apply(RawEvent::Resolved(a.clone())), None);
        // Same instance, other letter case, new address: an update.
        let b = svc(ServiceKind::Smb, "nas", 445, "10.0.0.3", None);
        assert_eq!(r.apply(RawEvent::Resolved(b.clone())), Some(DiscoveryEvent::Updated(b)));
        assert_eq!(r.services().count(), 1);
        assert_eq!(
            r.apply(RawEvent::Removed {
                kind: ServiceKind::Smb,
                instance: "NAS".into()
            }),
            Some(DiscoveryEvent::Lost {
                kind: ServiceKind::Smb,
                instance: "nas".into()
            })
        );
        // Removing an unknown instance is silent.
        assert_eq!(
            r.apply(RawEvent::Removed {
                kind: ServiceKind::Smb,
                instance: "NAS".into()
            }),
            None
        );
    }

    #[test]
    fn same_name_different_kinds_are_distinct() {
        let mut r = Registry::new();
        r.apply(RawEvent::Resolved(svc(ServiceKind::Smb, "NAS", 445, "10.0.0.2", None)));
        r.apply(RawEvent::Resolved(svc(ServiceKind::Nfs, "NAS", 2049, "10.0.0.2", None)));
        assert_eq!(r.services().count(), 2);
        assert_eq!(r.services_of(ServiceKind::Nfs).count(), 1);
    }

    #[test]
    fn ignores_own_id() {
        let mut r = Registry::new().with_ignored_ids(["me"]);
        assert_eq!(
            r.apply(RawEvent::Resolved(svc(ServiceKind::SyncPeer, "Me", 7000, "10.0.0.2", Some("me")))),
            None
        );
        let other = svc(ServiceKind::Server, "Srv", 8096, "10.0.0.9", Some("srv-1"));
        assert!(r.apply(RawEvent::Resolved(other)).is_some());
        assert_eq!(r.server_by_id("srv-1").unwrap().port, 8096);
        assert!(r.server_by_id("me").is_none());
    }

    #[test]
    fn clear_kind_reports_lost() {
        let mut r = Registry::new();
        r.apply(RawEvent::Resolved(svc(ServiceKind::Smb, "A", 445, "10.0.0.2", None)));
        r.apply(RawEvent::Resolved(svc(ServiceKind::Smb, "B", 445, "10.0.0.3", None)));
        r.apply(RawEvent::Resolved(svc(ServiceKind::Nfs, "C", 2049, "10.0.0.4", None)));
        let lost = r.clear_kind(ServiceKind::Smb);
        assert_eq!(lost.len(), 2);
        assert!(lost.iter().all(|e| matches!(e, DiscoveryEvent::Lost { kind: ServiceKind::Smb, .. })));
        assert_eq!(r.services().count(), 1);
    }
}
