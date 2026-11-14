// SPDX-License-Identifier: MPL-2.0
//! mDNS/DNS-SD announcement of the server: `_projectalpha._tcp` with the TXT
//! schema of `pa_discovery::ServerTxt` (SRV-004, ADR-0019).

use pa_discovery::{
    Advertiser, Announcement, DiscoveryError, Fingerprint, LABEL_MAX, MdnsConfig, Registration,
    ServerTxt,
};

/// Major version of the HTTP API (`/api/v1`), TXT `v`.
pub const API_VERSION: u32 = 1;

/// Name used when neither the config nor the OS gives one.
const FALLBACK_NAME: &str = "Kinolune";

/// A running announcement. Stop it with [`Announcer::stop`] (blocking: it
/// waits up to a second for the goodbye packet).
pub struct Announcer {
    advertiser: Advertiser,
    registration: Registration,
}

impl Announcer {
    /// Starts announcing server `id` on `port`.
    pub fn start(
        mdns: &MdnsConfig,
        name: &str,
        id: &str,
        port: u16,
        fingerprint: Option<Fingerprint>,
    ) -> Result<Self, DiscoveryError> {
        let txt = ServerTxt {
            id: id.to_owned(),
            api_version: API_VERSION,
            name: Some(name.to_owned()),
            fingerprint: fingerprint.map(|fp| fp.to_string()),
        };
        let advertiser = Advertiser::with_config(mdns)?;
        let registration =
            match advertiser.announce(&Announcement::server(instance_name(name), port, &txt)) {
                Ok(registration) => registration,
                Err(e) => {
                    advertiser.shutdown();
                    return Err(e);
                }
            };
        Ok(Self {
            advertiser,
            registration,
        })
    }

    /// DNS-SD full name as announced.
    pub fn fullname(&self) -> &str {
        self.registration.fullname()
    }

    /// Withdraws the service (goodbye packet) and stops the daemon.
    pub fn stop(self) {
        drop(self.registration);
        self.advertiser.shutdown();
    }
}

/// The configured name, else the host name, else "Kinolune".
pub fn display_name(configured: Option<&str>) -> String {
    configured
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(host_name)
        .unwrap_or_else(|| FALLBACK_NAME.to_owned())
}

/// One DNS label: at most 63 bytes, cut on a character boundary.
pub fn instance_name(name: &str) -> String {
    let mut end = name.len().min(LABEL_MAX);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let cut = name[..end].trim();
    if cut.is_empty() {
        FALLBACK_NAME.to_owned()
    } else {
        cut.to_owned()
    }
}

fn host_name() -> Option<String> {
    let from_file = ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok());
    from_file
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty() && n != "localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(display_name(Some(" Den ")), "Den");
        assert!(!display_name(Some("  ")).is_empty());
        assert!(!display_name(None).is_empty());
        let long = "ё".repeat(40); // 80 bytes
        let cut = instance_name(&long);
        assert!(cut.len() <= LABEL_MAX);
        assert_eq!(cut, "ё".repeat(31));
        assert_eq!(instance_name(" "), FALLBACK_NAME);
    }
}
