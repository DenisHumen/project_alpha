// SPDX-License-Identifier: MPL-2.0

import 'source_manager.dart';

/// A share found on the local network, offered at the top of the add flow.
class DiscoveredSource {
  const DiscoveredSource({
    required this.type,
    required this.name,
    required this.host,
    this.port,
    this.path,
    this.url,
    this.secure = true,
  });

  final SourceType type;

  /// Service instance name ("Living-room NAS").
  final String name;
  final String host;
  final int? port;

  /// Path from the TXT record (WebDAV `path=`), if any.
  final String? path;

  /// Address to connect to, built by the core from the IP address
  /// (`smb://192.168.1.2/`, `http://192.168.1.2:8080/dav`): `.local` names
  /// do not resolve everywhere (Android). Null before the address is known.
  final String? url;

  /// WebDAV over HTTPS (`_webdavs._tcp`) rather than HTTP (`_webdav._tcp`).
  final bool secure;
}

/// LAN discovery of shares (SMB, WebDAV, NFS) for the add flow.
///
/// `pa-discovery` (NET-009, mDNS/DNS-SD) through pa_core is
/// `CoreSourceDiscovery` (NET-020, core_discovery.dart). Without the Rust
/// core the app runs with [NoSourceDiscovery] and the add flow hides the
/// "On this network" section.
abstract class SourceDiscovery {
  /// False hides the "On this network" section.
  bool get supported;

  /// Current list of shares; emits again when one appears or goes away.
  Stream<List<DiscoveredSource>> watch();
}

class NoSourceDiscovery implements SourceDiscovery {
  const NoSourceDiscovery();

  @override
  bool get supported => false;

  @override
  Stream<List<DiscoveredSource>> watch() => const Stream.empty();
}

/// A fixed list, for demo mode and tests.
class StaticSourceDiscovery implements SourceDiscovery {
  const StaticSourceDiscovery(this.sources);

  final List<DiscoveredSource> sources;

  @override
  bool get supported => true;

  @override
  Stream<List<DiscoveredSource>> watch() => Stream.value(sources);
}
