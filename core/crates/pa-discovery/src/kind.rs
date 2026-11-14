// SPDX-License-Identifier: MPL-2.0
//! DNS-SD service types the core browses for (ADR-0019).

use std::fmt;

/// The mDNS domain every type here lives in.
pub const LOCAL_DOMAIN: &str = "local.";

/// A kind of service the core can find in the LAN.
///
/// Each kind is one DNS-SD service type. Service names follow RFC 6763 §7.2
/// (at most 15 characters after the leading underscore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ServiceKind {
    /// Our media server (mode B): `_projectalpha._tcp`, TXT [`ServerTxt`](crate::ServerTxt).
    Server,
    /// Another app instance for LAN sync (SYNC-006): `_alpha-sync._tcp`,
    /// TXT [`PeerTxt`](crate::PeerTxt).
    SyncPeer,
    /// An app showing a pairing invitation (UI-019): `_alpha-pair._tcp`,
    /// TXT `id`, `k` (fingerprint of the inviter's signing key), `name`.
    /// Lets a device that typed only the 6-digit code find the inviter.
    Pairing,
    /// SMB file server: `_smb._tcp` (Samba via Avahi, macOS, many NAS).
    Smb,
    /// WebDAV over HTTP: `_webdav._tcp`, optional TXT `path`.
    WebDav,
    /// WebDAV over HTTPS: `_webdavs._tcp`, optional TXT `path`.
    WebDavs,
    /// NFS export: `_nfs._tcp`, optional TXT `path`.
    Nfs,
}

impl ServiceKind {
    /// Every kind, in a stable order.
    pub const ALL: [Self; 7] = [
        Self::Server,
        Self::SyncPeer,
        Self::Pairing,
        Self::Smb,
        Self::WebDav,
        Self::WebDavs,
        Self::Nfs,
    ];

    /// File-share kinds (sources for mode A).
    pub const SHARES: [Self; 4] = [Self::Smb, Self::WebDav, Self::WebDavs, Self::Nfs];

    /// Service type without the domain, e.g. `_smb._tcp`. This is the form
    /// `NWBrowser`, `NsdManager` and `NSBonjourServices` use.
    pub const fn service_type(self) -> &'static str {
        match self {
            Self::Server => "_projectalpha._tcp",
            Self::SyncPeer => "_alpha-sync._tcp",
            Self::Pairing => "_alpha-pair._tcp",
            Self::Smb => "_smb._tcp",
            Self::WebDav => "_webdav._tcp",
            Self::WebDavs => "_webdavs._tcp",
            Self::Nfs => "_nfs._tcp",
        }
    }

    /// Fully qualified type with the `local.` domain, e.g. `_smb._tcp.local.`.
    pub const fn service_type_local(self) -> &'static str {
        match self {
            Self::Server => "_projectalpha._tcp.local.",
            Self::SyncPeer => "_alpha-sync._tcp.local.",
            Self::Pairing => "_alpha-pair._tcp.local.",
            Self::Smb => "_smb._tcp.local.",
            Self::WebDav => "_webdav._tcp.local.",
            Self::WebDavs => "_webdavs._tcp.local.",
            Self::Nfs => "_nfs._tcp.local.",
        }
    }

    /// Parses a service type in any of the usual spellings: with or without
    /// the `local.` domain and the trailing dot, any letter case
    /// (`_SMB._tcp`, `_smb._tcp.local`, `_smb._tcp.local.`).
    pub fn from_service_type(ty: &str) -> Option<Self> {
        let ty = ty.trim().trim_end_matches('.').to_ascii_lowercase();
        let ty = ty.strip_suffix(".local").unwrap_or(&ty);
        Self::ALL.into_iter().find(|k| k.service_type() == ty)
    }

    /// Default TCP port when an announcement does not matter (display and
    /// URL building only; the SRV record always wins).
    pub const fn default_port(self) -> Option<u16> {
        match self {
            Self::Smb => Some(445),
            Self::WebDav => Some(80),
            Self::WebDavs => Some(443),
            Self::Nfs => Some(2049),
            Self::Server | Self::SyncPeer | Self::Pairing => None,
        }
    }

    /// Whether this kind is a file share (mode A source).
    pub const fn is_share(self) -> bool {
        matches!(self, Self::Smb | Self::WebDav | Self::WebDavs | Self::Nfs)
    }
}

impl fmt::Display for ServiceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.service_type())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_fit_rfc6763_limit() {
        for kind in ServiceKind::ALL {
            let name = kind
                .service_type()
                .strip_prefix('_')
                .and_then(|s| s.strip_suffix("._tcp"))
                .unwrap();
            assert!(
                (1..=15).contains(&name.len()),
                "{kind}: service name {name:?} must be 1..=15 chars"
            );
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
            assert_eq!(
                kind.service_type_local(),
                format!("{}.{LOCAL_DOMAIN}", kind.service_type())
            );
        }
    }

    #[test]
    fn parses_common_spellings() {
        for s in [
            "_smb._tcp",
            "_smb._tcp.",
            "_smb._tcp.local",
            "_SMB._TCP.local.",
            " _smb._tcp.local. ",
        ] {
            assert_eq!(
                ServiceKind::from_service_type(s),
                Some(ServiceKind::Smb),
                "{s}"
            );
        }
        assert_eq!(
            ServiceKind::from_service_type("_projectalpha._tcp.local."),
            Some(ServiceKind::Server)
        );
        assert_eq!(ServiceKind::from_service_type("_http._tcp"), None);
        assert_eq!(ServiceKind::from_service_type(""), None);
    }

    #[test]
    fn roundtrip_all() {
        for kind in ServiceKind::ALL {
            assert_eq!(
                ServiceKind::from_service_type(kind.service_type()),
                Some(kind)
            );
            assert_eq!(
                ServiceKind::from_service_type(kind.service_type_local()),
                Some(kind)
            );
            assert_eq!(kind.is_share(), ServiceKind::SHARES.contains(&kind));
        }
    }
}
