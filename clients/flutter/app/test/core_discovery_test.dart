// SPDX-License-Identifier: MPL-2.0
// NET-020: discovered services become add-flow entries, and LAN browsing
// runs exactly while the add flow listens.
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/sources/core_discovery.dart';
import 'package:kinolune/sources/source_manager.dart';
import 'package:pa_core/pa_core.dart';

CoreDiscoveredService _svc(
  CoreServiceKind kind,
  String name, {
  String host = 'nas.local',
  int port = 445,
  String? path,
  String? url,
}) => CoreDiscoveredService(
  kind: kind,
  instance: name,
  host: host,
  port: port,
  addresses: const ['192.168.1.20'],
  path: path,
  url: url,
);

class _FakeCore implements DiscoveryCore {
  final log = <String>[];
  final _lists = StreamController<List<CoreDiscoveredService>>.broadcast();
  Completer<void>? holdStop;

  void emit(List<CoreDiscoveredService> list) => _lists.add(list);

  @override
  Stream<List<CoreDiscoveredService>> watch() => _lists.stream;

  @override
  Future<void> browse(List<CoreServiceKind> kinds) async {
    log.add('browse ${kinds.map((k) => k.name).join(',')}');
  }

  @override
  Future<void> stop(List<CoreServiceKind> kinds) async {
    await holdStop?.future;
    log.add('stop');
  }

  @override
  Future<void> close() async => log.add('close');
}

void main() {
  test('shares map to add-flow entries, other kinds are skipped', () {
    final smb = discoveredSourceFromCore(
      _svc(CoreServiceKind.smb, 'NAS', url: 'smb://192.168.1.20/'),
    )!;
    expect(smb.type, SourceType.smb);
    expect(smb.name, 'NAS');
    expect(smb.host, 'nas.local');
    expect(smb.url, 'smb://192.168.1.20/');

    final dav = discoveredSourceFromCore(
      _svc(CoreServiceKind.webDav, 'Files', port: 8080, path: '/dav'),
    )!;
    expect(dav.type, SourceType.webdav);
    expect(dav.secure, isFalse);
    expect((dav.port, dav.path), (8080, '/dav'));
    expect(
      discoveredSourceFromCore(_svc(CoreServiceKind.webDavs, 'S'))!.secure,
      isTrue,
    );

    final noHost = discoveredSourceFromCore(
      _svc(CoreServiceKind.smb, 'Bare', host: '', port: 0),
    )!;
    expect(noHost.host, '192.168.1.20');
    expect(noHost.port, isNull);

    expect(discoveredSourceFromCore(_svc(CoreServiceKind.server, 'S')), isNull);
    expect(
      discoveredSourceFromCore(_svc(CoreServiceKind.syncPeer, 'P')),
      isNull,
    );
  });

  test('browses while someone listens, in order', () async {
    final core = _FakeCore();
    final discovery = CoreSourceDiscovery(
      core,
      kinds: const [CoreServiceKind.smb, CoreServiceKind.webDav],
    );
    expect(discovery.supported, isTrue);

    final first = <List<String>>[];
    final a = discovery.watch().listen(
      (l) => first.add([for (final d in l) d.name]),
    );
    final b = discovery.watch().listen((_) {});
    await pumpEventQueue();
    expect(core.log, ['browse smb,webDav'], reason: 'one browse for both');

    core.emit([
      _svc(CoreServiceKind.server, 'Home server'),
      _svc(CoreServiceKind.smb, 'NAS'),
    ]);
    await pumpEventQueue();
    expect(first, [
      ['NAS'],
    ]);

    await a.cancel();
    expect(core.log, ['browse smb,webDav'], reason: 'b still listens');

    // The dialog closes and opens again before the stop is done: the new
    // browse runs after it.
    core.holdStop = Completer();
    final closing = b.cancel();
    final c = discovery.watch().listen((_) {});
    await pumpEventQueue();
    core.holdStop!.complete();
    await closing;
    await pumpEventQueue();
    expect(core.log, ['browse smb,webDav', 'stop', 'browse smb,webDav']);

    await c.cancel();
    await discovery.close();
    expect(core.log.skip(3), ['stop', 'close']);
  });
}
