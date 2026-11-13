// SPDX-License-Identifier: MPL-2.0
//! LAN discovery over mDNS/DNS-SD (NET-009, ADR-0019).
//!
//! What the core looks for ([`ServiceKind`]):
//!
//! | Kind | DNS-SD type | Used by |
//! |---|---|---|
//! | [`ServiceKind::Server`] | `_projectalpha._tcp` | own media server (mode B), TXT [`ServerTxt`] |
//! | [`ServiceKind::SyncPeer`] | `_alpha-sync._tcp` | LAN sync between apps (SYNC-006), TXT [`PeerTxt`] |
//! | [`ServiceKind::Smb`] | `_smb._tcp` | Samba/Avahi, macOS, NAS |
//! | [`ServiceKind::WebDav`], [`ServiceKind::WebDavs`] | `_webdav._tcp`, `_webdavs._tcp` | WebDAV shares |
//! | [`ServiceKind::Nfs`] | `_nfs._tcp` | NFS exports |
//!
//! Layers:
//!
//! - [`Registry`]: sans-IO picture of the LAN; turns repeated resolves and
//!   removals into [`DiscoveryEvent`]s.
//! - [`Discovery`]: facade over one [`Backend`] with subscriptions and
//!   snapshots.
//! - [`MdnsBackend`] / [`Advertiser`] (feature `mdns`): pure-Rust mDNS on
//!   `mdns-sd` for Linux, Windows, macOS and Android (Android holds a
//!   [`MulticastLock`]).
//! - [`PlatformBackend`]: `NWBrowser` on iOS and tvOS through a callback.
//! - `pin` (feature `tls`): the TXT `fp` format of our server and a rustls
//!   verifier that trusts only the pinned certificate (SRV-004).
//!
//! ```
//! use std::sync::Arc;
//! use pa_discovery::{Discovery, PlatformBackend, PlatformBrowser, PlatformSink, ServiceKind};
//!
//! struct NoBrowser;
//! impl PlatformBrowser for NoBrowser {
//!     fn start_browse(&self, _ty: String, _sink: Arc<PlatformSink>) -> Result<(), String> { Ok(()) }
//!     fn stop_browse(&self, _ty: String) {}
//! }
//!
//! let discovery = Discovery::new(PlatformBackend::new(Arc::new(NoBrowser)), ["this-device-id"]);
//! let events = discovery.subscribe();
//! discovery.browse(ServiceKind::SHARES).unwrap();
//! assert!(discovery.services().is_empty());
//! # drop(events);
//! ```

mod discovery;
mod kind;
#[cfg(feature = "mdns")]
mod mdns;
#[cfg(feature = "tls")]
mod pin;
mod platform;
mod registry;
mod service;

pub use discovery::{Backend, Discovery, DiscoveryError, EventSink};
pub use kind::{LOCAL_DOMAIN, ServiceKind};
#[cfg(feature = "mdns")]
pub use mdns::{
    Advertiser, Announcement, Interfaces, MDNS_PORT, MdnsBackend, MdnsConfig, MulticastLock,
    Registration, host_for_id,
};
#[cfg(feature = "tls")]
pub use pin::{
    FINGERPRINT_PREFIX, Fingerprint, FingerprintError, PinCheck, PinnedVerifier, check_pin,
};
pub use platform::{
    PlatformBackend, PlatformBrowser, PlatformService, PlatformSink, bonjour_services,
};
pub use registry::{DiscoveryEvent, RawEvent, Registry};
pub use service::{
    DeviceClass, DiscoveredService, LABEL_MAX, PeerTxt, ServerTxt, TXT_ENTRY_MAX, TXT_VERSION, Txt,
    TxtError, escape_instance, sort_addresses, split_fullname,
};
