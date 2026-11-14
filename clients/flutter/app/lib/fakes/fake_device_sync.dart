// SPDX-License-Identifier: MPL-2.0
//
// In-memory [DeviceSync] for the demo mode and tests (UI-019): invitations,
// joining by code or QR payload between fakes registered in one [hub], and
// unpairing. Progress itself is not moved: tests call [remoteChange] to
// simulate ops arriving from another device. The playback session
// (SYNC-003) is: heartbeats and takeovers go to the paired fakes of the
// same [FakeSyncLan], and tests inject another device's heartbeat with
// [receiveHeartbeat] / [receiveTakeover].

import 'dart:async';
import 'dart:math';

import '../sync/device_sync.dart';

/// Fakes that can find each other's invitations ("the LAN").
class FakeSyncLan {
  final List<FakeDeviceSync> devices = [];
}

class FakeDeviceSync implements DeviceSync {
  FakeDeviceSync({
    required this.id,
    required this.name,
    this.kind = SyncDeviceKind.desktop,
    FakeSyncLan? lan,
    this.available = true,
  }) : lan = lan ?? FakeSyncLan() {
    this.lan.devices.add(this);
  }

  /// A demo device already paired with a TV (online) and a phone (away).
  factory FakeDeviceSync.demo() {
    final me = FakeDeviceSync(
      id: 'kdemo-desktop',
      name: 'Workstation',
    ); // l10n-ignore
    me._family = [
      me._self,
      const SyncDevice(
        id: 'kdemo-tv',
        name: 'Living room TV', // l10n-ignore
        kind: SyncDeviceKind.tv,
        fingerprint: '7f3a 91c2 0b44',
        online: true,
      ),
      const SyncDevice(
        id: 'kdemo-phone',
        name: 'Pixel 8',
        kind: SyncDeviceKind.phone,
        fingerprint: 'c81e 55d0 a9f7',
      ),
    ];
    return me;
  }

  final String id;
  final String name;
  final SyncDeviceKind kind;
  final FakeSyncLan lan;

  @override
  final bool available;

  /// Codes of invitations this fake answers, and the open one.
  SyncInvite? openInvite;
  bool running = false;
  int starts = 0;

  List<SyncDevice> _family = [];
  final _events = StreamController<SyncEvent>.broadcast();

  SyncDevice get _self => SyncDevice(
    id: id,
    name: name,
    kind: kind,
    fingerprint: fingerprint,
    thisDevice: true,
    online: true,
  );

  @override
  String get fingerprint =>
      id.codeUnits.fold(0, (a, b) => (a * 31 + b) & 0xffffff).toRadixString(16);

  @override
  List<SyncDevice> devices() => List.unmodifiable(_family);

  @override
  bool get paired => _family.length > 1;

  @override
  Future<bool> start() async {
    starts++;
    running = paired;
    return running;
  }

  @override
  Future<SyncInvite> invite() async {
    if (!available) throw const SyncException(SyncErrorCode.unavailable);
    if (_family.isEmpty) _family = [_self];
    final code = (Random().nextInt(900000) + 100000).toString();
    final invite = SyncInvite(
      code: code,
      qr: 'projectalpha://pair?v=1&c=$code&k=${id.hashCode}',
      expiresAt: DateTime.now().add(const Duration(minutes: 5)),
    );
    openInvite = invite;
    return invite;
  }

  @override
  Future<void> cancelInvite() async {
    openInvite = null;
  }

  @override
  Future<SyncDevice> join(String input) async {
    if (!available) throw const SyncException(SyncErrorCode.unavailable);
    final text = input.trim();
    final code = text.startsWith('projectalpha://')
        ? RegExp(r'c=(\d{6})').firstMatch(text)?.group(1)
        : text.replaceAll(' ', '');
    if (code == null || !RegExp(r'^\d{6}$').hasMatch(code)) {
      throw const SyncException(SyncErrorCode.invalidCode);
    }
    if (paired) throw const SyncException(SyncErrorCode.alreadyPaired);
    final hosts = lan.devices.where((d) => d != this && d.openInvite != null);
    if (hosts.isEmpty) throw const SyncException(SyncErrorCode.notFound);
    final host = hosts.where((d) => d.openInvite!.code == code).firstOrNull;
    if (host == null) throw const SyncException(SyncErrorCode.wrongCode);
    host.openInvite = null;
    final me = _self;
    host._family = [...host._family, _asPeer(me)];
    _family = [
      me,
      for (final d in host._family)
        if (d.id != id) _asPeer(d),
    ];
    running = true;
    host.running = true;
    host._events.add(SyncEvent(SyncEventKind.paired, deviceId: id, name: name));
    host._events.add(const SyncEvent(SyncEventKind.inviteClosed));
    host._events.add(const SyncEvent(SyncEventKind.devicesChanged));
    _events.add(const SyncEvent(SyncEventKind.devicesChanged));
    return _asPeer(host._self);
  }

  static SyncDevice _asPeer(SyncDevice d) => SyncDevice(
    id: d.id,
    name: d.name,
    kind: d.kind,
    fingerprint: d.fingerprint,
    online: true,
  );

  @override
  Future<void> unpair(String deviceId) async {
    if (deviceId == id) {
      _family = [];
    } else {
      _family = _family.where((d) => d.id != deviceId).toList();
    }
    running = paired;
    _events.add(const SyncEvent(SyncEventKind.devicesChanged));
  }

  /// Simulates ops from another device landing in the library.
  void remoteChange() =>
      _events.add(const SyncEvent(SyncEventKind.libraryChanged));

  @override
  Stream<SyncEvent> get events => _events.stream;

  // Playback session (SYNC-003).

  /// Heartbeats this fake sent, oldest first.
  final List<({String itemId, int positionMs, SyncPlayState state})>
  heartbeats = [];

  /// Items this fake started (takeovers it sent).
  final List<String> started = [];

  /// Pull timeouts asked for by [resumeHint].
  final List<Duration> pulls = [];

  /// The newest point of each item known here (local or another device's).
  final Map<String, ResumeHint> points = {};

  /// Clock of the fake (stamps, "playing now").
  DateTime Function() clock = DateTime.now;
  int _counter = 0;

  /// How long a playing heartbeat counts as "playing now".
  static const playingTtl = Duration(seconds: 15);

  Iterable<FakeDeviceSync> get _peers =>
      lan.devices.where((d) => d != this && d._family.any((m) => m.id == id));

  @override
  Future<void> heartbeat(
    String itemId, {
    required int positionMs,
    required int durationMs,
    required SyncPlayState state,
  }) async {
    heartbeats.add((itemId: itemId, positionMs: positionMs, state: state));
    final stamp = clock().millisecondsSinceEpoch;
    for (final peer in _peers) {
      peer.receiveHeartbeat(
        deviceId: id,
        deviceName: name,
        itemId: itemId,
        positionMs: positionMs,
        durationMs: durationMs,
        state: state,
        stampMs: stamp,
        counter: ++_counter,
      );
    }
  }

  @override
  Future<void> startPlaying(String itemId) async {
    started.add(itemId);
    for (final peer in _peers) {
      peer.receiveTakeover(deviceId: id, itemId: itemId);
    }
  }

  @override
  Future<ResumeHint?> resumeHint(
    String itemId, {
    Duration timeout = Duration.zero,
  }) async {
    if (timeout > Duration.zero) pulls.add(timeout);
    return points[itemId];
  }

  @override
  Future<List<RemotePlayback>> playingElsewhere() async {
    final now = clock().millisecondsSinceEpoch;
    final list = [
      for (final e in points.entries)
        if (e.value.remote &&
            e.value.playing &&
            now - e.value.stampMs < playingTtl.inMilliseconds)
          RemotePlayback(
            itemId: e.key,
            deviceId: e.value.deviceId,
            deviceName: e.value.deviceName,
            positionMs: e.value.positionMs,
            durationMs: e.value.durationMs,
            stampMs: e.value.stampMs,
          ),
    ]..sort((a, b) => b.stampMs.compareTo(a.stampMs));
    return list;
  }

  /// Another device's heartbeat arrives (as `pb.update`).
  void receiveHeartbeat({
    required String deviceId,
    String? deviceName,
    required String itemId,
    required int positionMs,
    int durationMs = 0,
    SyncPlayState state = SyncPlayState.playing,
    int? stampMs,
    int counter = 0,
  }) {
    final hint = ResumeHint(
      positionMs: positionMs,
      durationMs: durationMs,
      deviceId: deviceId,
      deviceName: deviceName,
      remote: true,
      playing: state == SyncPlayState.playing,
      stampMs: stampMs ?? clock().millisecondsSinceEpoch,
      stampCounter: counter,
    );
    if (hint.isNewerThan(points[itemId])) points[itemId] = hint;
    _events.add(
      SyncEvent(
        SyncEventKind.playback,
        deviceId: deviceId,
        name: deviceName,
        itemId: itemId,
      ),
    );
  }

  /// Another device started [itemId] (as `pb.takeover`).
  void receiveTakeover({required String deviceId, required String itemId}) =>
      _events.add(
        SyncEvent(SyncEventKind.takeover, deviceId: deviceId, itemId: itemId),
      );
}
