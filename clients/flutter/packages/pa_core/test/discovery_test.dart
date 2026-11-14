// SPDX-License-Identifier: MPL-2.0
// NET-020 through the Dart bindings: one CoreDiscovery announces an SMB
// share, another finds it. Both stay on loopback and a private UDP port, so
// nothing reaches the network.
import 'dart:async';
import 'dart:io';
import 'dart:math';

import 'package:pa_core/pa_core.dart';
import 'package:test/test.dart';

import 'native_library.dart';

void main() {
  setUpAll(() async {
    await RustLib.init(externalLibrary: openBuiltCoreLibrary());
  });

  tearDownAll(RustLib.dispose);

  test('the add flow kinds are SMB, WebDAV and our server', () {
    expect(discoverySourceKinds().toSet(), {
      CoreServiceKind.smb,
      CoreServiceKind.webDav,
      CoreServiceKind.webDavs,
      CoreServiceKind.server,
    });
  });

  test('a share announced on loopback is found, lock held while browsing',
      () async {
    final port = 30000 + (pid * 7 + Random().nextInt(9000)) % 20000;
    final config = CoreDiscoveryConfig(loopbackPort: port, ownIds: const []);
    final announcer = await CoreDiscovery.open(
      config: config,
      multicastLock: (_) {},
    );
    addTearDown(announcer.close);
    await announcer.announce(
      kind: CoreServiceKind.smb,
      instance: 'Dart NAS',
      port: 4450,
      txt: const [],
    );

    final lock = <bool>[];
    final discovery = await CoreDiscovery.open(
      config: config,
      multicastLock: lock.add,
    );
    final lists = <List<CoreDiscoveredService>>[];
    final found = Completer<CoreDiscoveredService>();
    final ended = Completer<void>();
    final sub = discovery.watch().listen((list) {
      lists.add(list);
      for (final s in list) {
        if (s.instance == 'Dart NAS' && s.url != null && !found.isCompleted) {
          found.complete(s);
        }
      }
    }, onDone: ended.complete);
    addTearDown(sub.cancel);

    await discovery.browse(kinds: discoverySourceKinds());
    expect(
      discovery.browsing().toSet(),
      discoverySourceKinds().toSet(),
    );
    final nas = await found.future.timeout(const Duration(seconds: 20));
    expect(lists.first, isEmpty, reason: 'the current list comes at once');
    expect(nas.kind, CoreServiceKind.smb);
    expect(nas.port, 4450);
    expect(nas.url, 'smb://127.0.0.1:4450/');
    expect(discovery.services().map((s) => s.instance), contains('Dart NAS'));
    expect(lock, [true]);

    await discovery.close();
    await ended.future.timeout(const Duration(seconds: 10));
    // The lock callback runs on a core thread, in order.
    final deadline = DateTime.now().add(const Duration(seconds: 5));
    while (lock.length < 2 && DateTime.now().isBefore(deadline)) {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    expect(lock, [true, false]);
    await expectLater(
      discovery.browse(kinds: [CoreServiceKind.smb]),
      throwsA(isA<CoreDiscoveryError>().having((e) => e.code, 'code', 'closed')),
    );
  });
}
