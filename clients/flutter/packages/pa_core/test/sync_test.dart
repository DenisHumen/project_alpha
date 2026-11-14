// SPDX-License-Identifier: MPL-2.0
// UI-019 through the Dart bindings: two libraries in one process pair by QR
// code over loopback, and a position saved on one shows up on the other.
import 'dart:async';
import 'dart:io';

import 'package:pa_core/pa_core.dart';
import 'package:test/test.dart';

import 'native_library.dart';

/// A fake media file with content unique to [seed] (same seed, same
/// fingerprint on both devices).
void _media(String root, String rel, int seed) {
  final f = File('$root/$rel')..createSync(recursive: true);
  f.writeAsBytesSync([for (var i = 0; i < 150000; i++) (i * 31 + seed) & 0xff]);
}

class _Device {
  _Device(this.dir, this.library, this.sync);
  final Directory dir;
  final CoreLibrary library;
  final CoreSync sync;

  static Future<_Device> open(String name, CoreDeviceClass cls) async {
    final dir = Directory.systemTemp.createTempSync('pa_sync_test_');
    final media = '${dir.path}/media';
    _media(media, 'Movies/The Matrix 1999.mkv', 2);
    final library = await CoreLibrary.open(path: '${dir.path}/library.db');
    await library.addSource(kind: 'local', name: 'Media', location: media);
    await library.scan(roots: []).last;
    final sync = await CoreSync.open(
      library_: library,
      config: CoreSyncConfig(
        dataDir: '${dir.path}/sync',
        deviceName: name,
        deviceClass: cls,
        secretsDir: '${dir.path}/secrets',
        loopback: true,
      ),
    );
    return _Device(dir, library, sync);
  }

  Future<String> matrixId() async =>
      (await library.movies()).firstWhere((m) => m.title.contains('Matrix')).id;
}

Future<void> _until(String what, FutureOr<bool> Function() ok) async {
  final deadline = DateTime.now().add(const Duration(seconds: 15));
  while (!await ok()) {
    if (DateTime.now().isAfter(deadline)) fail('timed out: $what');
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
}

void main() {
  setUpAll(() async {
    await RustLib.init(externalLibrary: openBuiltCoreLibrary());
  });

  tearDownAll(RustLib.dispose);

  test('pair by QR, then progress on A appears on B', () async {
    final pc = await _Device.open('Desktop', CoreDeviceClass.desktop);
    final phone = await _Device.open('Phone', CoreDeviceClass.mobile);
    addTearDown(() {
      pc.dir.deleteSync(recursive: true);
      phone.dir.deleteSync(recursive: true);
    });
    final events = <CoreSyncEventKind>[];
    final sub = phone.sync.events().listen((e) => events.add(e.kind));
    addTearDown(sub.cancel);

    expect(pc.sync.devices(), isEmpty);
    final invite = await pc.sync.invite();
    expect(invite.code, matches(RegExp(r'^\d{6}$')));
    await expectLater(
      phone.sync.join(input: '12'),
      throwsA(
        isA<CoreSyncError>().having((e) => e.code, 'code', 'invalid_code'),
      ),
    );
    final inviter = await phone.sync.join(input: invite.qr);
    expect(inviter.name, 'Desktop');
    expect(inviter.fingerprint, pc.sync.fingerprint());
    await _until('desktop runs', () => pc.sync.isRunning());
    expect(pc.sync.devices().map((d) => d.name), ['Desktop', 'Phone']);
    expect(phone.sync.devices().first.thisDevice, isTrue);

    pc.sync.peerUp(
      deviceId: phone.sync.deviceId(),
      deviceClass: CoreDeviceClass.mobile,
      addr: phone.sync.syncAddr()!,
    );
    phone.sync.peerUp(
      deviceId: pc.sync.deviceId(),
      deviceClass: CoreDeviceClass.desktop,
      addr: pc.sync.syncAddr()!,
    );
    await _until(
      'both online',
      () => phone.sync.devices().every((d) => d.online),
    );

    final now = DateTime.now().millisecondsSinceEpoch;
    expect(
      await pc.library.saveProgress(
        itemId: await pc.matrixId(),
        positionMs: 2471200,
        durationMs: 8160000,
        updatedAtMs: now,
        watched: false,
      ),
      isTrue,
    );
    final onPhone = await phone.matrixId();
    await _until('progress on the phone', () async {
      final p = await phone.library.progress(itemId: onPhone);
      return p?.positionMs == 2471200;
    });
    expect(events, contains(CoreSyncEventKind.libraryChanged));
    final cont = await phone.library.continueWatching(limit: 10);
    expect(cont.single.item.id, onPhone);

    await pc.sync.unpair(deviceId: phone.sync.deviceId());
    expect(pc.sync.devices().map((d) => d.name), ['Desktop']);
    expect(pc.sync.isPaired(), isFalse);
  });
}
