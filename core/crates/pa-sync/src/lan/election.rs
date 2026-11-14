// SPDX-License-Identifier: MPL-2.0
//! Hub election, sans-IO: every device computes the same hub from what it
//! sees in the LAN, so no coordination round is needed.
//!
//! The hub is the best live candidate by `(class rank, device_id)`: server,
//! then desktop, TV, phone (ROADMAP SYNC-006), ties broken by the smaller
//! `device_id`. A peer whose connection failed is skipped for a cooldown,
//! so a powered-off hub is replaced as soon as its link times out, even if
//! its mDNS record is still cached.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Instant;

use pa_discovery::DeviceClass;

/// Rank of a device class in the election; lower wins.
pub const fn class_rank(class: DeviceClass) -> u8 {
    match class {
        DeviceClass::Server => 0,
        DeviceClass::Desktop => 1,
        DeviceClass::Tv => 2,
        DeviceClass::Mobile => 3,
        DeviceClass::Other => 4,
    }
}

/// Election key of a device: lower is a better hub.
pub fn hub_key(class: DeviceClass, device_id: &str) -> (u8, &str) {
    (class_rank(class), device_id)
}

/// A device seen in the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub class: DeviceClass,
    pub addr: SocketAddr,
    /// Skipped by the election until then (its connection failed).
    pub dead_until: Option<Instant>,
}

/// Who should be the hub now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hub {
    /// This device.
    Me,
    /// Another device: connect to it.
    Peer { device_id: String, addr: SocketAddr },
}

/// The peers this device knows and the election over them.
#[derive(Debug, Clone)]
pub struct Election {
    me: String,
    class: DeviceClass,
    peers: BTreeMap<String, Peer>,
}

impl Election {
    /// Election for this device (`me`, of `class`) with no peers yet.
    pub fn new(me: impl Into<String>, class: DeviceClass) -> Self {
        Self {
            me: me.into(),
            class,
            peers: BTreeMap::new(),
        }
    }

    /// A peer appeared or changed its address. A new address clears the
    /// dead mark: the device came back.
    pub fn upsert(&mut self, device_id: &str, class: DeviceClass, addr: SocketAddr) {
        if device_id == self.me {
            return;
        }
        let dead_until = self
            .peers
            .get(device_id)
            .filter(|p| p.addr == addr)
            .and_then(|p| p.dead_until);
        self.peers.insert(
            device_id.to_owned(),
            Peer {
                class,
                addr,
                dead_until,
            },
        );
    }

    /// A peer went away.
    pub fn remove(&mut self, device_id: &str) {
        self.peers.remove(device_id);
    }

    /// The link to `device_id` failed: skip it until `until`.
    pub fn mark_dead(&mut self, device_id: &str, until: Instant) {
        if let Some(p) = self.peers.get_mut(device_id) {
            p.dead_until = Some(until);
        }
    }

    /// Known peers.
    pub fn peers(&self) -> &BTreeMap<String, Peer> {
        &self.peers
    }

    /// The hub at `now`.
    pub fn hub(&self, now: Instant) -> Hub {
        let best = self
            .peers
            .iter()
            .filter(|(_, p)| p.dead_until.is_none_or(|t| t <= now))
            .min_by(|(a, pa), (b, pb)| hub_key(pa.class, a).cmp(&hub_key(pb.class, b)));
        match best {
            Some((id, p)) if hub_key(p.class, id) < hub_key(self.class, &self.me) => Hub::Peer {
                device_id: id.clone(),
                addr: p.addr,
            },
            _ => Hub::Me,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn server_desktop_tv_phone_order() {
        let now = Instant::now();
        let mut e = Election::new("phone", DeviceClass::Mobile);
        assert_eq!(e.hub(now), Hub::Me);
        e.upsert("tv", DeviceClass::Tv, addr(1));
        e.upsert("pc", DeviceClass::Desktop, addr(2));
        e.upsert("weird", DeviceClass::Other, addr(3));
        assert_eq!(
            e.hub(now),
            Hub::Peer {
                device_id: "pc".into(),
                addr: addr(2)
            }
        );
        e.upsert("nas", DeviceClass::Server, addr(4));
        assert!(matches!(e.hub(now), Hub::Peer { device_id, .. } if device_id == "nas"));
        // The desktop itself sees the server too, and nobody picks the phone.
        let mut pc = Election::new("pc", DeviceClass::Desktop);
        pc.upsert("phone", DeviceClass::Mobile, addr(5));
        assert_eq!(pc.hub(now), Hub::Me);
    }

    #[test]
    fn ties_go_to_the_smaller_id_on_every_device() {
        let now = Instant::now();
        let mut a = Election::new("tv-a", DeviceClass::Tv);
        a.upsert("tv-b", DeviceClass::Tv, addr(1));
        let mut b = Election::new("tv-b", DeviceClass::Tv);
        b.upsert("tv-a", DeviceClass::Tv, addr(2));
        assert_eq!(a.hub(now), Hub::Me);
        assert!(matches!(b.hub(now), Hub::Peer { device_id, .. } if device_id == "tv-a"));
    }

    #[test]
    fn dead_hub_is_skipped_until_cooldown_or_new_address() {
        let now = Instant::now();
        let mut e = Election::new("phone", DeviceClass::Mobile);
        e.upsert("pc", DeviceClass::Desktop, addr(1));
        e.upsert("tv", DeviceClass::Tv, addr(2));
        e.mark_dead("pc", now + Duration::from_secs(20));
        assert!(matches!(e.hub(now), Hub::Peer { device_id, .. } if device_id == "tv"));
        // The same announcement again does not revive it...
        e.upsert("pc", DeviceClass::Desktop, addr(1));
        assert!(matches!(e.hub(now), Hub::Peer { device_id, .. } if device_id == "tv"));
        // ...the cooldown does, and so does a new address (restart).
        assert!(
            matches!(e.hub(now + Duration::from_secs(21)), Hub::Peer { device_id, .. } if device_id == "pc")
        );
        e.upsert("pc", DeviceClass::Desktop, addr(9));
        assert!(matches!(e.hub(now), Hub::Peer { device_id, .. } if device_id == "pc"));
        e.remove("pc");
        e.remove("tv");
        assert_eq!(e.hub(now), Hub::Me);
        // Our own announcement is ignored.
        e.upsert("phone", DeviceClass::Server, addr(3));
        assert_eq!(e.hub(now), Hub::Me);
    }
}
