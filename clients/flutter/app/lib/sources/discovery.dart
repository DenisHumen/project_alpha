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
  });

  final SourceType type;

  /// Service instance name ("Living-room NAS").
  final String name;
  final String host;
  final int? port;

  /// Path from the TXT record (WebDAV `path=`), if any.
  final String? path;
}

/// LAN discovery of shares (SMB, WebDAV, NFS) for the add flow.
///
/// Hook for `pa-discovery` (NET-009, mDNS/DNS-SD): the crate is not exposed
/// through pa_core yet, so the app runs with [NoSourceDiscovery] and the add
/// flow hides the "On this network" section. A pa_core-backed
/// implementation plugs in through `AppServices(discovery: …)`.
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
