// SPDX-License-Identifier: MPL-2.0
//
// LAN discovery of sources on the Rust core (NET-020, ADR-0019):
// `CoreDiscovery` of pa_core browses SMB, WebDAV and our server over mDNS
// while the add-source flow listens.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:pa_core/pa_core.dart';

import '../sync/device_info.dart';
import 'discovery.dart';
import 'source_manager.dart';

/// What [CoreSourceDiscovery] needs from the core; tests fake it.
abstract class DiscoveryCore {
  /// The visible services: the current list at once, then after every
  /// change.
  Stream<List<CoreDiscoveredService>> watch();
  Future<void> browse(List<CoreServiceKind> kinds);
  Future<void> stop(List<CoreServiceKind> kinds);
  Future<void> close();
}

class _PaCoreDiscovery implements DiscoveryCore {
  _PaCoreDiscovery(this._core);

  final CoreDiscovery _core;

  @override
  Stream<List<CoreDiscoveredService>> watch() => _core.watch();

  @override
  Future<void> browse(List<CoreServiceKind> kinds) =>
      _core.browse(kinds: kinds);

  @override
  Future<void> stop(List<CoreServiceKind> kinds) => _core.stop(kinds: kinds);

  @override
  Future<void> close() => _core.close();
}

/// The share a discovered service offers to the add flow; null for kinds
/// that are not sources (our server, sync peers).
DiscoveredSource? discoveredSourceFromCore(CoreDiscoveredService s) {
  final (type, secure) = switch (s.kind) {
    CoreServiceKind.smb => (SourceType.smb, false),
    CoreServiceKind.webDav => (SourceType.webdav, false),
    CoreServiceKind.webDavs => (SourceType.webdav, true),
    CoreServiceKind.nfs => (SourceType.nfs, false),
    _ => (null, false),
  };
  if (type == null) return null;
  final host = s.host.isNotEmpty
      ? s.host
      : (s.addresses.isEmpty ? '' : s.addresses.first);
  return DiscoveredSource(
    type: type,
    name: s.instance,
    host: host,
    port: s.port == 0 ? null : s.port,
    path: s.path,
    url: s.url,
    secure: secure,
  );
}

/// Browses while someone listens to [watch]: the first listener starts the
/// browse (and, on Android, takes the multicast lock through the core), the
/// last one to cancel stops it.
class CoreSourceDiscovery implements SourceDiscovery {
  CoreSourceDiscovery(this._core, {List<CoreServiceKind>? kinds})
    : _kinds = kinds ?? discoverySourceKinds();

  /// Opens mDNS discovery on every interface (5353). [loopbackPort] keeps
  /// it on loopback and a private port (tests).
  static Future<CoreSourceDiscovery> open({
    List<String> ownIds = const [],
    int? loopbackPort,
  }) async {
    final core = await CoreDiscovery.open(
      config: CoreDiscoveryConfig(ownIds: ownIds, loopbackPort: loopbackPort),
      multicastLock: (hold) => holdMulticastLock(hold, holder: 'discovery'),
    );
    return CoreSourceDiscovery(_PaCoreDiscovery(core));
  }

  final DiscoveryCore _core;
  final List<CoreServiceKind> _kinds;
  int _listeners = 0;

  /// Browse and stop calls in order (a dialog closed and opened again must
  /// not stop the new browse).
  Future<void> _ops = Future.value();

  Future<void> _serial(Future<void> Function() op) {
    final next = _ops.then((_) => op());
    _ops = next.catchError((Object _) {});
    return next;
  }

  @override
  bool get supported => true;

  @override
  Stream<List<DiscoveredSource>> watch() {
    StreamSubscription<List<CoreDiscoveredService>>? sub;
    late final StreamController<List<DiscoveredSource>> out;
    out = StreamController<List<DiscoveredSource>>(
      onListen: () {
        sub = _core.watch().listen(
          (list) =>
              out.add([for (final s in list) ?discoveredSourceFromCore(s)]),
          onError: out.addError,
        );
        if (_listeners++ == 0) {
          _serial(() => _core.browse(_kinds)).catchError((Object e) {
            // Sockets refused (no network): the list stays empty.
            debugPrint('LAN discovery unavailable: $e');
            out.addError(e);
          });
        }
      },
      onCancel: () async {
        // Count first: a listener that comes while this one is cancelled
        // must see the right number.
        final last = --_listeners == 0;
        final stopping = last ? _serial(() => _core.stop(_kinds)) : null;
        await sub?.cancel();
        await stopping;
      },
    );
    return out.stream;
  }

  /// Stops browsing and releases the multicast lock. Not queued behind
  /// pending browse or stop calls: after it they do nothing.
  Future<void> close() => _core.close();
}
