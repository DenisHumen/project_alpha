// SPDX-License-Identifier: MPL-2.0
//
// Handoff between devices (SYNC-003, docs/api/sync-protocol.md §7): the
// ephemeral playback session of this device. While a video plays it sends
// heartbeats (every 5 s with the progress saves of the player, and at once
// on pause, seek, stop and going to the background), announces a takeover
// when it starts, pauses here when another device of the profile takes the
// same title over (Settings, on by default), offers "Jump to 41:11?" when
// another device turns out to be further along, and keeps the list of
// titles other devices play right now for the Home banner.

import 'dart:async';

import 'package:material_ui/material_ui.dart';
import 'package:pa_player/pa_player.dart'
    show MediaKitEngine, PlayerEngine, PlayerEngineFactory;

import '../integration/library.dart';
import 'device_sync.dart';

/// How long the app waits for its pull before a title starts (500–800 ms
/// by the protocol; `pa_sync::handoff::PULL_TIMEOUT_MS`).
const handoffPullTimeout = Duration(milliseconds: 600);

/// A newer position of another device at least this far ahead is offered
/// (`pa_sync::handoff::OFFER_MIN_GAP_MS`).
const handoffOfferGapMs = 5000;

/// "Playing elsewhere" is re-read this often while it is not empty, so a
/// device that stopped sending drops out (its heartbeats live 15 s).
const handoffRecheck = Duration(seconds: 5);

/// "Jump to [positionMs]?" for the title playing here.
class HandoffOffer {
  const HandoffOffer({
    required this.positionMs,
    required this.deviceId,
    this.deviceName,
  });

  final int positionMs;
  final String deviceId;
  final String? deviceName;
}

class HandoffController extends ChangeNotifier {
  HandoffController(this.sync);

  final DeviceSync sync;

  /// Pause here when another device of the profile starts the title that
  /// plays here (Settings; on by default).
  final ValueNotifier<bool> pauseOnTakeover = ValueNotifier(true);

  /// The offer the player shows now, if any.
  final ValueNotifier<HandoffOffer?> offer = ValueNotifier(null);

  /// Name of the device that took the title over and paused it here (the
  /// player shows it once), or null.
  final ValueNotifier<String?> pausedFor = ValueNotifier(null);

  /// Titles other devices play right now, newest first (Home banner).
  List<RemotePlayback> get elsewhere => _elsewhere;
  List<RemotePlayback> _elsewhere = const [];

  StreamSubscription<SyncEvent>? _sub;
  Timer? _recheck;
  bool _disposed = false;

  // The session of this device.
  String? _itemId;
  PlayerEngine? _engine;
  ResumeHint? _baseline;
  int _lastPos = 0;
  int _lastDur = 0;
  final Set<String> _declined = {};

  /// Item playing here, if any.
  String? get playingItemId => _itemId;

  /// Follows the sync events. Safe to call again.
  void start() {
    _sub ??= sync.events.listen(_onEvent);
    unawaited(refreshElsewhere());
  }

  void _onEvent(SyncEvent e) {
    switch (e.kind) {
      case SyncEventKind.playback:
        unawaited(refreshElsewhere());
        if (e.itemId != null && e.itemId == _itemId) unawaited(_checkOffer());
      case SyncEventKind.takeover:
        _onTakeover(e);
        unawaited(refreshElsewhere());
      case SyncEventKind.libraryChanged:
        // A `progress.upsert` of another device may be newer.
        if (_itemId != null) unawaited(_checkOffer());
      case SyncEventKind.devicesChanged:
        unawaited(refreshElsewhere());
      case SyncEventKind.paired ||
          SyncEventKind.pairingFailed ||
          SyncEventKind.inviteClosed:
        break;
    }
  }

  /// Re-reads what other devices play now.
  Future<void> refreshElsewhere() async {
    List<RemotePlayback> list;
    try {
      list = await sync.playingElsewhere();
    } catch (e) {
      debugPrint('Handoff: playing elsewhere not read: $e');
      list = const [];
    }
    if (_disposed) return;
    _recheck?.cancel();
    _recheck = list.isEmpty ? null : Timer(handoffRecheck, refreshElsewhere);
    if (!_same(list, _elsewhere)) {
      _elsewhere = list;
      notifyListeners();
    }
  }

  static bool _same(List<RemotePlayback> a, List<RemotePlayback> b) =>
      a.length == b.length &&
      [
        for (var i = 0; i < a.length; i++)
          a[i].itemId == b[i].itemId &&
              a[i].deviceId == b[i].deviceId &&
              a[i].stampMs == b[i].stampMs,
      ].every((x) => x);

  /// Where [item] starts here: [requestedMs] (what the caller resumes from
  /// the local progress), unless another device holds a newer position by
  /// HLC after a short pull. With [fromStart] (the user chose "from the
  /// beginning") the requested position stays.
  Future<int> startPosition(
    LibraryItem item,
    int requestedMs, {
    bool fromStart = false,
  }) async {
    ResumeHint? hint;
    try {
      hint = await sync.resumeHint(item.id, timeout: handoffPullTimeout);
    } catch (e) {
      debugPrint('Handoff: no resume hint for ${item.id}: $e');
    }
    _baseline = hint;
    if (fromStart || hint == null || !hint.remote) return requestedMs;
    return resumePositionMs(
      Progress(
        itemId: item.id,
        positionMs: hint.positionMs,
        durationMs: hint.durationMs > 0
            ? hint.durationMs
            : item.durationMs ?? 0,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(hint.stampMs),
      ),
    );
  }

  /// Wraps the player's engine factory to reach the engine (pause on a
  /// takeover, jump on an accepted offer).
  PlayerEngineFactory engineFactory(PlayerEngineFactory? base) =>
      ({bool video = true}) {
        final engine = (base ?? MediaKitEngine.create)(video: video);
        _engine = engine;
        return engine;
      };

  /// [itemId] starts here at [startMs]: other devices of the profile pause
  /// it (takeover) and see it playing.
  void begin(String itemId, int startMs, {int durationMs = 0}) {
    if (_itemId != null && _itemId != itemId) _stop();
    _itemId = itemId;
    _lastPos = startMs;
    _lastDur = durationMs;
    _declined.clear();
    offer.value = null;
    pausedFor.value = null;
    unawaited(_quiet(() => sync.startPlaying(itemId)));
    _beat(SyncPlayState.playing);
  }

  /// A progress report of the player for [itemId] (every 5 s while playing,
  /// on pause, seek and exit): the heartbeat. An Up Next item starts a new
  /// session.
  void report(String itemId, int positionMs, int durationMs) {
    // A late report after the player closed starts nothing.
    if (_itemId == null) return;
    if (_itemId != itemId) {
      begin(itemId, positionMs, durationMs: durationMs);
      return;
    }
    _lastPos = positionMs;
    _lastDur = durationMs;
    _beat(_state);
  }

  /// A heartbeat now (the app goes to the background).
  void beatNow() {
    if (_itemId == null) return;
    final s = _engine?.state.value;
    if (s != null && s.duration > Duration.zero) {
      _lastPos = s.position.inMilliseconds;
      _lastDur = s.duration.inMilliseconds;
    }
    _beat(_state);
  }

  /// The player closed.
  void end() {
    if (_itemId == null) return;
    _stop();
    _engine = null;
    _baseline = null;
    offer.value = null;
    pausedFor.value = null;
  }

  void _stop() {
    _beat(SyncPlayState.stopped);
    _itemId = null;
  }

  SyncPlayState get _state => (_engine?.state.value.playing ?? true)
      ? SyncPlayState.playing
      : SyncPlayState.paused;

  void _beat(SyncPlayState state) {
    final id = _itemId;
    if (id == null) return;
    unawaited(
      _quiet(
        () => sync.heartbeat(
          id,
          positionMs: _lastPos,
          durationMs: _lastDur,
          state: state,
        ),
      ),
    );
  }

  static Future<void> _quiet(Future<void> Function() f) async {
    try {
      await f();
    } catch (e) {
      debugPrint('Handoff: $e');
    }
  }

  void _onTakeover(SyncEvent e) {
    final engine = _engine;
    if (_itemId == null || e.itemId != _itemId || engine == null) return;
    if (!pauseOnTakeover.value || !engine.state.value.playing) return;
    unawaited(engine.pause());
    pausedFor.value = _nameOf(e.deviceId);
  }

  String? _nameOf(String? deviceId) =>
      sync.devices().where((d) => d.id == deviceId).firstOrNull?.name;

  int get _position => _engine?.state.value.position.inMilliseconds ?? _lastPos;

  Future<void> _checkOffer() async {
    final id = _itemId;
    if (id == null) return;
    ResumeHint? hint;
    try {
      hint = await sync.resumeHint(id);
    } catch (_) {
      return;
    }
    if (hint == null || id != _itemId || !hint.remote) return;
    if (!hint.isNewerThan(_baseline) || _declined.contains(hint.deviceId)) {
      return;
    }
    if (hint.positionMs - _position < handoffOfferGapMs) return;
    offer.value = HandoffOffer(
      positionMs: hint.positionMs,
      deviceId: hint.deviceId,
      deviceName: hint.deviceName ?? _nameOf(hint.deviceId),
    );
    _baseline = hint;
  }

  /// Jumps to the offered position.
  Future<void> acceptOffer() async {
    final o = offer.value;
    offer.value = null;
    final engine = _engine;
    if (o == null || engine == null) return;
    await engine.seek(Duration(milliseconds: o.positionMs));
    _lastPos = o.positionMs;
    _beat(_state);
  }

  /// Keeps the position here; that device is not offered again in this
  /// session.
  void dismissOffer() {
    final o = offer.value;
    if (o != null) _declined.add(o.deviceId);
    offer.value = null;
  }

  @override
  void dispose() {
    _disposed = true;
    _recheck?.cancel();
    unawaited(_sub?.cancel());
    pauseOnTakeover.dispose();
    offer.dispose();
    pausedFor.dispose();
    super.dispose();
  }
}
