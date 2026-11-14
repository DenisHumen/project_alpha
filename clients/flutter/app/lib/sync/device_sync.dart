// SPDX-License-Identifier: MPL-2.0
//
// Pairing and LAN sync of devices (UI-019). The app talks to [DeviceSync];
// [CoreDeviceSync] runs it on the Rust core (pa-sync through pa_core), the
// demo and tests use FakeDeviceSync (lib/fakes/fake_device_sync.dart), and
// the v0 library fallback has [NoDeviceSync].

import 'dart:async';

import 'package:pa_core/pa_core.dart';

export 'package:pa_core/pa_core.dart' show CoreDeviceClass;

/// What kind of device a family member is.
enum SyncDeviceKind { phone, tablet, tv, desktop, server }

SyncDeviceKind _kind(String s) => switch (s) {
  'tablet' => SyncDeviceKind.tablet,
  'tv' => SyncDeviceKind.tv,
  'desktop' => SyncDeviceKind.desktop,
  'server' => SyncDeviceKind.server,
  _ => SyncDeviceKind.phone,
};

/// A device of this device's family.
class SyncDevice {
  const SyncDevice({
    required this.id,
    required this.name,
    required this.kind,
    required this.fingerprint,
    this.thisDevice = false,
    this.online = false,
  });

  final String id;
  final String name;
  final SyncDeviceKind kind;

  /// Fingerprint of the device key; the same on both screens.
  final String fingerprint;
  final bool thisDevice;

  /// Linked or visible on the LAN now.
  final bool online;
}

/// An open invitation: the 6-digit [code] and the [qr] payload.
class SyncInvite {
  const SyncInvite({
    required this.code,
    required this.qr,
    required this.expiresAt,
  });

  final String code;
  final String qr;
  final DateTime expiresAt;
}

/// Why a pairing call failed.
enum SyncErrorCode {
  invalidCode,
  wrongCode,
  expired,
  notFound,
  alreadyPaired,
  refused,
  unavailable,
  other,
}

class SyncException implements Exception {
  const SyncException(this.code, [this.detail = '']);

  final SyncErrorCode code;
  final String detail;

  static SyncErrorCode codeOf(String code) => switch (code) {
    'invalid_code' => SyncErrorCode.invalidCode,
    'wrong_code' => SyncErrorCode.wrongCode,
    'expired' => SyncErrorCode.expired,
    'not_found' => SyncErrorCode.notFound,
    'already_paired' => SyncErrorCode.alreadyPaired,
    'refused' || 'unknown_device' => SyncErrorCode.refused,
    'secrets' => SyncErrorCode.unavailable,
    _ => SyncErrorCode.other,
  };

  @override
  String toString() => 'SyncException($code${detail.isEmpty ? '' : ': $detail'})';
}

/// Kind of a [SyncEvent].
enum SyncEventKind {
  /// Changes from another device are in the library.
  libraryChanged,

  /// The device list or online states changed.
  devicesChanged,

  /// A device joined through this device's invitation ([SyncEvent.name]).
  paired,

  /// An attempt on the invitation failed; it stays open.
  pairingFailed,

  /// The invitation closed (expired, used up or used).
  inviteClosed,

  /// What another device plays now ([SyncEvent.payload]: JSON `pb.update`).
  /// Hook for the "continue here" banner of SYNC-003.
  playback,

  /// Another device started a title ([SyncEvent.payload]: `pb.takeover`).
  takeover,
}

class SyncEvent {
  const SyncEvent(this.kind, {this.deviceId, this.name, this.payload});

  final SyncEventKind kind;
  final String? deviceId;
  final String? name;
  final String? payload;
}

/// Pairing and sync of this app.
abstract class DeviceSync {
  /// False when sync cannot run here (no Rust core); the Devices screen
  /// explains it.
  bool get available;

  /// The family, this device first; empty before the first pairing.
  List<SyncDevice> devices();

  /// Whether other devices are paired.
  bool get paired;

  /// Fingerprint of this device's key.
  String get fingerprint;

  /// Starts syncing if paired; returns whether it runs.
  Future<bool> start();

  /// Opens an invitation for another device (5 minutes, 5 attempts).
  Future<SyncInvite> invite();

  Future<void> cancelInvite();

  /// Joins the family of the device showing [input]: a scanned QR payload or
  /// the typed 6-digit code. Returns that device.
  Future<SyncDevice> join(String input);

  /// Removes [deviceId] from the family; this device's id leaves it.
  Future<void> unpair(String deviceId);

  /// Events from now on (broadcast).
  Stream<SyncEvent> get events;
}

/// No sync (the v0 library fallback has no Rust core).
class NoDeviceSync implements DeviceSync {
  const NoDeviceSync();

  static const _off = SyncException(SyncErrorCode.unavailable);

  @override
  bool get available => false;
  @override
  List<SyncDevice> devices() => const [];
  @override
  bool get paired => false;
  @override
  String get fingerprint => '';
  @override
  Future<bool> start() async => false;
  @override
  Future<SyncInvite> invite() async => throw _off;
  @override
  Future<void> cancelInvite() async {}
  @override
  Future<SyncDevice> join(String input) async => throw _off;
  @override
  Future<void> unpair(String deviceId) async => throw _off;
  @override
  Stream<SyncEvent> get events => const Stream.empty();
}

/// [DeviceSync] on the Rust core.
class CoreDeviceSync implements DeviceSync {
  CoreDeviceSync._(this._core) {
    _sub = _core.events().listen((e) => _events.add(_event(e)));
  }

  final CoreSync _core;
  final _events = StreamController<SyncEvent>.broadcast();
  StreamSubscription<CoreSyncEvent>? _sub;

  /// Opens the sync service over [library] (the core library of the app),
  /// keeping the family roster in [dataDir].
  static Future<CoreDeviceSync> open({
    required CoreLibrary library,
    required String dataDir,
    required String deviceName,
    required CoreDeviceClass deviceClass,
  }) async {
    try {
      return CoreDeviceSync._(
        await CoreSync.open(
          library_: library,
          config: CoreSyncConfig(
            dataDir: dataDir,
            deviceName: deviceName,
            deviceClass: deviceClass,
            loopback: false,
          ),
        ),
      );
    } on CoreSyncError catch (e) {
      throw SyncException(SyncException.codeOf(e.code), e.message);
    }
  }

  static SyncEvent _event(CoreSyncEvent e) => SyncEvent(
    switch (e.kind) {
      CoreSyncEventKind.libraryChanged => SyncEventKind.libraryChanged,
      CoreSyncEventKind.devicesChanged => SyncEventKind.devicesChanged,
      CoreSyncEventKind.paired => SyncEventKind.paired,
      CoreSyncEventKind.pairingFailed => SyncEventKind.pairingFailed,
      CoreSyncEventKind.inviteClosed => SyncEventKind.inviteClosed,
      CoreSyncEventKind.playback => SyncEventKind.playback,
      CoreSyncEventKind.takeover => SyncEventKind.takeover,
    },
    deviceId: e.deviceId,
    name: e.name,
    payload: e.payload,
  );

  static SyncDevice _device(CoreSyncDevice d) => SyncDevice(
    id: d.deviceId,
    name: d.name,
    kind: _kind(d.kind),
    fingerprint: d.fingerprint,
    thisDevice: d.thisDevice,
    online: d.online,
  );

  Future<T> _call<T>(Future<T> Function() f) async {
    try {
      return await f();
    } on CoreSyncError catch (e) {
      throw SyncException(SyncException.codeOf(e.code), e.message);
    }
  }

  @override
  bool get available => true;

  @override
  List<SyncDevice> devices() => _core.devices().map(_device).toList();

  @override
  bool get paired => _core.isPaired();

  @override
  String get fingerprint => _core.fingerprint();

  @override
  Future<bool> start() => _call(_core.start);

  @override
  Future<SyncInvite> invite() => _call(() async {
    final i = await _core.invite();
    return SyncInvite(
      code: i.code,
      qr: i.qr,
      expiresAt: DateTime.fromMillisecondsSinceEpoch(i.expiresMs.toInt()),
    );
  });

  @override
  Future<void> cancelInvite() => _call(_core.cancelInvite);

  @override
  Future<SyncDevice> join(String input) =>
      _call(() async => _device(await _core.join(input: input)));

  @override
  Future<void> unpair(String deviceId) =>
      _call(() => _core.unpair(deviceId: deviceId));

  @override
  Stream<SyncEvent> get events => _events.stream;

  Future<void> dispose() async {
    await _sub?.cancel();
    await _core.stop();
    await _events.close();
  }
}
