// SPDX-License-Identifier: MPL-2.0
//! A resolved service instance and the TXT schemas of our own service types.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::kind::ServiceKind;

/// Max length of one TXT entry `key=value` in bytes (one length octet).
pub const TXT_ENTRY_MAX: usize = 255;
/// Max length of a DNS label (an instance name is one label).
pub const LABEL_MAX: usize = 63;
/// Version of the TXT schema of our own service types (`txtvers`).
pub const TXT_VERSION: u32 = 1;

/// TXT record of a service: keys are lowercase (RFC 6763 §6.4 makes them
/// case-insensitive), a boolean attribute without `=` has an empty value.
/// Values that are not UTF-8 are decoded lossily.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Txt(BTreeMap<String, String>);

impl Txt {
    /// Empty TXT record.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a TXT record from pairs; the first occurrence of a key wins
    /// (RFC 6763 §6.4), keys are lowercased, empty keys are dropped.
    pub fn from_pairs<K: AsRef<str>, V: AsRef<str>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let mut map = BTreeMap::new();
        for (k, v) in pairs {
            let key = k.as_ref().trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            map.entry(key).or_insert_with(|| v.as_ref().to_owned());
        }
        Self(map)
    }

    /// Like [`Txt::from_pairs`] for raw values (as platform browsers deliver
    /// them); `None` is a boolean attribute.
    pub fn from_raw<K: AsRef<str>>(pairs: impl IntoIterator<Item = (K, Option<Vec<u8>>)>) -> Self {
        Self::from_pairs(pairs.into_iter().map(|(k, v)| {
            let v = v
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            (k, v)
        }))
    }

    /// Value of `key` (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(&key.to_ascii_lowercase()).map(String::as_str)
    }

    /// All entries, sorted by key.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn non_empty(&self, key: &str) -> Option<&str> {
        self.get(key).map(str::trim).filter(|v| !v.is_empty())
    }
}

/// One resolved service instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredService {
    /// Which service type it was found as.
    pub kind: ServiceKind,
    /// Human-readable instance name, unescaped (`Living room NAS`).
    pub instance: String,
    /// Target host from the SRV record, e.g. `nas.local.`; may be empty when
    /// a platform browser does not resolve hosts.
    pub host: String,
    /// TCP port from the SRV record.
    pub port: u16,
    /// Addresses, best first: IPv4 before IPv6, routable before link-local.
    pub addresses: Vec<IpAddr>,
    /// TXT record.
    pub txt: Txt,
}

impl DiscoveredService {
    /// Builds a service and puts its addresses in the preferred order.
    pub fn new(
        kind: ServiceKind,
        instance: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        addresses: impl IntoIterator<Item = IpAddr>,
        txt: Txt,
    ) -> Self {
        let mut addresses: Vec<IpAddr> = addresses.into_iter().collect();
        sort_addresses(&mut addresses);
        Self {
            kind,
            instance: instance.into(),
            host: host.into(),
            port,
            addresses,
            txt,
        }
    }

    /// Stable identity of the instance within its type: the lowercased
    /// instance name (DNS names are case-insensitive).
    pub fn key(&self) -> (ServiceKind, String) {
        (self.kind, self.instance.to_lowercase())
    }

    /// Full DNS-SD name, escaped: `Living room NAS._smb._tcp.local.`.
    pub fn fullname(&self) -> String {
        format!(
            "{}.{}",
            escape_instance(&self.instance),
            self.kind.service_type_local()
        )
    }

    /// Best address to connect to.
    pub fn primary_address(&self) -> Option<IpAddr> {
        self.addresses.first().copied()
    }

    /// Our own identity from TXT `id` (servers and sync peers).
    pub fn id(&self) -> Option<&str> {
        self.txt.non_empty("id")
    }

    /// TXT `path` of WebDAV and NFS announcements, normalized to start with
    /// `/`; `/` when absent.
    pub fn path(&self) -> String {
        match self.txt.non_empty("path") {
            Some(p) if p.starts_with('/') => p.to_owned(),
            Some(p) => format!("/{p}"),
            None => "/".to_owned(),
        }
    }

    /// URL of a file share for the source manager (NET-011), built from the
    /// primary address so it works where `.local` names do not resolve
    /// (Android). `None` for our own service types or without an address.
    pub fn share_url(&self) -> Option<String> {
        let (scheme, default_port) = match self.kind {
            ServiceKind::Smb => ("smb", 445),
            ServiceKind::WebDav => ("http", 80),
            ServiceKind::WebDavs => ("https", 443),
            ServiceKind::Nfs => ("nfs", 2049),
            ServiceKind::Server | ServiceKind::SyncPeer | ServiceKind::Pairing => return None,
        };
        let addr = self.primary_address()?;
        let host = match addr {
            IpAddr::V4(a) => a.to_string(),
            IpAddr::V6(a) => format!("[{a}]"),
        };
        let port = if self.port == default_port || self.port == 0 {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        let path = if self.kind == ServiceKind::Smb {
            "/".to_owned()
        } else {
            self.path()
        };
        Some(format!("{scheme}://{host}{port}{path}"))
    }

    /// TXT of a `_projectalpha._tcp` announcement.
    pub fn server_txt(&self) -> Result<ServerTxt, TxtError> {
        ServerTxt::from_txt(&self.txt)
    }

    /// TXT of an `_alpha-sync._tcp` announcement.
    pub fn peer_txt(&self) -> Result<PeerTxt, TxtError> {
        PeerTxt::from_txt(&self.txt)
    }
}

/// Error in the TXT record of one of our service types.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TxtError {
    /// A required key is absent or empty.
    #[error("TXT key `{0}` is missing")]
    Missing(&'static str),
    /// A key has a value of the wrong form.
    #[error("TXT key `{key}` has an invalid value `{value}`")]
    Invalid {
        /// The key.
        key: &'static str,
        /// The value as received.
        value: String,
    },
    /// `txtvers` is newer than this build understands.
    #[error("TXT schema version {0} is not supported")]
    UnsupportedVersion(u32),
}

fn parse_u32(txt: &Txt, key: &'static str) -> Result<u32, TxtError> {
    let v = txt.non_empty(key).ok_or(TxtError::Missing(key))?;
    v.parse().map_err(|_| TxtError::Invalid {
        key,
        value: v.to_owned(),
    })
}

fn check_txtvers(txt: &Txt) -> Result<(), TxtError> {
    // Absent `txtvers` means version 1. A newer major schema is rejected so an
    // old client does not misread a new server.
    match txt.non_empty("txtvers") {
        None => Ok(()),
        Some(_) => match parse_u32(txt, "txtvers")? {
            v if v <= TXT_VERSION => Ok(()),
            v => Err(TxtError::UnsupportedVersion(v)),
        },
    }
}

/// TXT schema of `_projectalpha._tcp` (research 04 §1.6, ADR-0019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTxt {
    /// `id`: stable server UUID.
    pub id: String,
    /// `v`: major version of the server API (`/api/v1` → 1).
    pub api_version: u32,
    /// `name`: display name; the instance name is used when absent.
    pub name: Option<String>,
    /// `fp`: fingerprint of the server TLS certificate or key for pinning
    /// (SRV-004: `sha256:<hex>` of the DER certificate, see `Fingerprint`
    /// with the `tls` feature).
    pub fingerprint: Option<String>,
}

impl ServerTxt {
    /// Parses and validates the TXT record.
    pub fn from_txt(txt: &Txt) -> Result<Self, TxtError> {
        check_txtvers(txt)?;
        Ok(Self {
            id: txt
                .non_empty("id")
                .ok_or(TxtError::Missing("id"))?
                .to_owned(),
            api_version: parse_u32(txt, "v")?,
            name: txt.non_empty("name").map(str::to_owned),
            fingerprint: txt.non_empty("fp").map(str::to_owned),
        })
    }

    /// TXT entries to announce.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut out = vec![
            ("txtvers".to_owned(), TXT_VERSION.to_string()),
            ("id".to_owned(), self.id.clone()),
            ("v".to_owned(), self.api_version.to_string()),
        ];
        if let Some(name) = &self.name {
            out.push(("name".to_owned(), name.clone()));
        }
        if let Some(fp) = &self.fingerprint {
            out.push(("fp".to_owned(), fp.clone()));
        }
        out
    }
}

/// Device class of a sync peer; it orders hub election in SYNC-006
/// (server, desktop, TV, phone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceClass {
    /// The user's media server.
    Server,
    /// A desktop app.
    Desktop,
    /// A TV app.
    Tv,
    /// A phone or tablet app.
    Mobile,
    /// A class this build does not know (a newer peer).
    Other,
}

impl DeviceClass {
    /// Wire value of TXT `dev`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Desktop => "desktop",
            Self::Tv => "tv",
            Self::Mobile => "mobile",
            Self::Other => "other",
        }
    }

    /// Parses TXT `dev`; unknown values become [`DeviceClass::Other`].
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "server" => Self::Server,
            "desktop" => Self::Desktop,
            "tv" => Self::Tv,
            "mobile" => Self::Mobile,
            _ => Self::Other,
        }
    }
}

/// TXT schema of `_alpha-sync._tcp` (ADR-0019, used by SYNC-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTxt {
    /// `id`: `device_id` of the peer (the same id HLC timestamps carry).
    pub id: String,
    /// `v`: major version of the sync protocol (SYNC-002).
    pub protocol_version: u32,
    /// `dev`: device class.
    pub device_class: DeviceClass,
    /// `name`: display name of the device.
    pub name: Option<String>,
    /// `fp`: fingerprint of the peer key from pairing (SYNC-005).
    pub fingerprint: Option<String>,
}

impl PeerTxt {
    /// Parses and validates the TXT record.
    pub fn from_txt(txt: &Txt) -> Result<Self, TxtError> {
        check_txtvers(txt)?;
        Ok(Self {
            id: txt
                .non_empty("id")
                .ok_or(TxtError::Missing("id"))?
                .to_owned(),
            protocol_version: parse_u32(txt, "v")?,
            device_class: txt
                .non_empty("dev")
                .map_or(DeviceClass::Other, DeviceClass::parse),
            name: txt.non_empty("name").map(str::to_owned),
            fingerprint: txt.non_empty("fp").map(str::to_owned),
        })
    }

    /// TXT entries to announce.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut out = vec![
            ("txtvers".to_owned(), TXT_VERSION.to_string()),
            ("id".to_owned(), self.id.clone()),
            ("v".to_owned(), self.protocol_version.to_string()),
            ("dev".to_owned(), self.device_class.as_str().to_owned()),
        ];
        if let Some(name) = &self.name {
            out.push(("name".to_owned(), name.clone()));
        }
        if let Some(fp) = &self.fingerprint {
            out.push(("fp".to_owned(), fp.clone()));
        }
        out
    }
}

/// Orders addresses best first: IPv4 before IPv6, routable before
/// link-local and loopback, then numerically; drops duplicates.
pub fn sort_addresses(addrs: &mut Vec<IpAddr>) {
    fn rank(a: &IpAddr) -> (u8, u8) {
        let family = u8::from(a.is_ipv6());
        let scope = match a {
            IpAddr::V4(v4) if v4.is_loopback() => 2,
            IpAddr::V4(v4) if v4.is_link_local() => 1,
            IpAddr::V6(v6) if v6.is_loopback() => 2,
            IpAddr::V6(v6) if v6.is_unicast_link_local() => 1,
            _ => 0,
        };
        (family, scope)
    }
    addrs.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    addrs.dedup();
}

/// Escapes an instance name for a DNS-SD full name (`.` and `\`).
pub fn escape_instance(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '.' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Splits a DNS-SD full name `<instance>.<type>.local.` into the unescaped
/// instance name and the service kind. Handles `\.`, `\\` and `\DDD`
/// escapes. `None` when the name is not one of our types.
pub fn split_fullname(fullname: &str) -> Option<(String, ServiceKind)> {
    // The instance is the first label (it may contain escaped dots); the rest
    // is the type and the domain.
    let bytes = fullname.as_bytes();
    let mut i = 0;
    let mut raw: Vec<u8> = Vec::with_capacity(bytes.len());
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit) => {
                let d = &fullname[i + 1..i + 4];
                raw.push(d.parse::<u16>().ok().and_then(|n| u8::try_from(n).ok())?);
                i += 4;
            }
            b'\\' if i + 1 < bytes.len() => {
                raw.push(bytes[i + 1]);
                i += 2;
            }
            b'.' => break,
            b => {
                raw.push(b);
                i += 1;
            }
        }
    }
    if i >= bytes.len() || raw.is_empty() {
        return None;
    }
    let kind = ServiceKind::from_service_type(&fullname[i + 1..])?;
    Some((String::from_utf8_lossy(&raw).into_owned(), kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn txt(pairs: &[(&str, &str)]) -> Txt {
        Txt::from_pairs(pairs.iter().copied())
    }

    #[test]
    fn txt_keys_case_insensitive_first_wins() {
        let t = txt(&[("ID", "a"), ("id", "b"), ("", "x"), ("Path", "/media")]);
        assert_eq!(t.get("id"), Some("a"));
        assert_eq!(t.get("PATH"), Some("/media"));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn txt_raw_boolean_and_lossy() {
        let t = Txt::from_raw([("flag", None), ("bin", Some(vec![0xff, b'a']))]);
        assert_eq!(t.get("flag"), Some(""));
        assert_eq!(t.get("bin"), Some("\u{fffd}a"));
    }

    #[test]
    fn server_txt_roundtrip() {
        let s = ServerTxt {
            id: "0192f1d0-0000-7000-8000-000000000001".into(),
            api_version: 1,
            name: Some("Home server".into()),
            fingerprint: Some("sha256:abcd".into()),
        };
        let pairs = s.to_pairs();
        assert!(
            pairs
                .iter()
                .all(|(k, v)| k.len() + 1 + v.len() <= TXT_ENTRY_MAX)
        );
        assert_eq!(ServerTxt::from_txt(&Txt::from_pairs(pairs)).unwrap(), s);
    }

    #[test]
    fn server_txt_errors() {
        assert_eq!(
            ServerTxt::from_txt(&txt(&[("v", "1")])),
            Err(TxtError::Missing("id"))
        );
        assert_eq!(
            ServerTxt::from_txt(&txt(&[("id", "x")])),
            Err(TxtError::Missing("v"))
        );
        assert!(matches!(
            ServerTxt::from_txt(&txt(&[("id", "x"), ("v", "one")])),
            Err(TxtError::Invalid { key: "v", .. })
        ));
        assert_eq!(
            ServerTxt::from_txt(&txt(&[("id", "x"), ("v", "1"), ("txtvers", "2")])),
            Err(TxtError::UnsupportedVersion(2))
        );
        // Absent txtvers = 1; empty optional keys are None.
        let s = ServerTxt::from_txt(&txt(&[("id", "x"), ("v", "3"), ("fp", " ")])).unwrap();
        assert_eq!((s.api_version, s.fingerprint), (3, None));
    }

    #[test]
    fn peer_txt_roundtrip_and_unknown_class() {
        let p = PeerTxt {
            id: "tv-1".into(),
            protocol_version: 1,
            device_class: DeviceClass::Tv,
            name: None,
            fingerprint: None,
        };
        assert_eq!(
            PeerTxt::from_txt(&Txt::from_pairs(p.to_pairs())).unwrap(),
            p
        );
        let q = PeerTxt::from_txt(&txt(&[("id", "x"), ("v", "1"), ("dev", "fridge")])).unwrap();
        assert_eq!(q.device_class, DeviceClass::Other);
        assert!(
            DeviceClass::Server < DeviceClass::Desktop && DeviceClass::Tv < DeviceClass::Mobile
        );
    }

    #[test]
    fn addresses_sorted_best_first() {
        let mut a: Vec<IpAddr> = vec![
            "fe80::1".parse().unwrap(),
            "2001:db8::1".parse().unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "169.254.1.1".parse().unwrap(),
            "192.168.1.20".parse().unwrap(),
            "192.168.1.20".parse().unwrap(),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ];
        sort_addresses(&mut a);
        let s: Vec<String> = a.iter().map(ToString::to_string).collect();
        assert_eq!(
            s,
            [
                "192.168.1.20",
                "169.254.1.1",
                "127.0.0.1",
                "2001:db8::1",
                "fe80::1",
                "::1"
            ]
        );
    }

    #[test]
    fn share_urls() {
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        let smb =
            DiscoveredService::new(ServiceKind::Smb, "NAS", "nas.local.", 445, [ip], Txt::new());
        assert_eq!(smb.share_url().unwrap(), "smb://192.168.1.5/");
        let dav = DiscoveredService::new(
            ServiceKind::WebDavs,
            "NAS",
            "nas.local.",
            5006,
            [ip],
            txt(&[("path", "dav/media")]),
        );
        assert_eq!(
            dav.share_url().unwrap(),
            "https://192.168.1.5:5006/dav/media"
        );
        let v6: IpAddr = "2001:db8::5".parse().unwrap();
        let nfs = DiscoveredService::new(
            ServiceKind::Nfs,
            "NAS",
            "",
            2049,
            [v6],
            txt(&[("path", "/srv")]),
        );
        assert_eq!(nfs.share_url().unwrap(), "nfs://[2001:db8::5]/srv");
        let srv = DiscoveredService::new(ServiceKind::Server, "S", "", 8096, [ip], Txt::new());
        assert_eq!(srv.share_url(), None);
        let noaddr =
            DiscoveredService::new(ServiceKind::Smb, "NAS", "nas.local.", 445, [], Txt::new());
        assert_eq!(noaddr.share_url(), None);
    }

    #[test]
    fn fullname_escape_and_split() {
        let s = DiscoveredService::new(ServiceKind::Smb, r"My.NAS\1", "", 445, [], Txt::new());
        assert_eq!(s.fullname(), r"My\.NAS\\1._smb._tcp.local.");
        assert_eq!(
            split_fullname(&s.fullname()),
            Some((r"My.NAS\1".to_owned(), ServiceKind::Smb))
        );
        // Decimal escapes (Avahi/Bonjour presentation format), UTF-8 bytes.
        assert_eq!(
            split_fullname(r"Caf\195\169\032NAS._smb._tcp.local."),
            Some(("Café NAS".to_owned(), ServiceKind::Smb))
        );
        assert_eq!(split_fullname("x._http._tcp.local."), None);
        assert_eq!(split_fullname("._smb._tcp.local."), None);
        assert_eq!(split_fullname("nodots"), None);
        assert_eq!(split_fullname(r"bad\999._smb._tcp.local."), None);
    }
}
