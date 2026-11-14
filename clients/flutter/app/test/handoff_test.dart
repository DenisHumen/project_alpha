// SPDX-License-Identifier: MPL-2.0
//
// SYNC-003 in the app: the Home banner "Continue on this device" for a
// title another paired device plays, resuming from that device's position;
// heartbeats and the takeover of the playback session; a takeover by
// another device pausing the player (and the Settings switch that turns it
// off); the "Jump to 41:11?" toast when another device is further along.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_device_sync.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/screens/playback.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/preferences.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/sync/device_sync.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_player/pa_player.dart' show PlayerScreen;
import 'package:pa_player/testing.dart';

import 'support/test_app.dart';

const _movie = 'movie-harbor-lights';
const _tv = 'kdemo-tv';
const _tvName = 'Living room TV'; // l10n-ignore
const _duration = 6000000;

/// 41:11.
const _at = 41 * 60000 + 11000;

Future<(AppServices, FakeDeviceSync, FakePlayerEngine)> _services({
  Locale? locale,
  Preferences? preferences,
}) async {
  final engine = FakePlayerEngine(
    loadedDuration: const Duration(milliseconds: _duration),
  );
  final sync = FakeDeviceSync.demo();
  final s = AppServices(
    library: FakeLibrary.demo(scanStepDelay: Duration.zero),
    secureStore: MemorySecureStore(),
    preferences: preferences,
    playerEngineFactory: ({bool video = true}) => engine,
    sync: sync,
    clock: () => demoNow,
  );
  await s.init();
  if (locale != null) s.locale.value = locale;
  return (s, sync, engine);
}

/// Opens the player on [_movie] from Home.
Future<void> _play(WidgetTester t, {int startMs = 0}) async {
  final context = t.element(find.byType(Scaffold).first);
  final item = (await Services.read(context).library.item(_movie))!;
  playVideo(context, item, startPositionMs: startMs);
  await settle(t);
  expect(find.byType(PlayerScreen), findsOneWidget);
}

Future<void> _close(WidgetTester t) async {
  await t.pumpWidget(const SizedBox());
  await settle(t, 2);
}

void main() {
  setUpAll(loadAppFonts);

  testWidgets('Home banner: continue a title the TV plays, from its position', (
    t,
  ) async {
    setSurface(t, const Size(1280, 800));
    final (services, sync, engine) = await _services();
    await t.pumpWidget(testApp(services));
    await settle(t);
    expect(find.byKey(const ValueKey('handoff-banner')), findsNothing);

    // The TV plays the movie: the banner appears with its name and position.
    sync.receiveHeartbeat(
      deviceId: _tv,
      deviceName: _tvName,
      itemId: _movie,
      positionMs: _at,
      durationMs: _duration,
    );
    await settle(t, 4);
    final banner = find.byKey(const ValueKey('handoff-banner'));
    await scrollHomeTo(t, banner);
    expect(banner, findsOneWidget);
    expect(find.text('Playing on $_tvName'), findsOneWidget);
    expect(find.textContaining('41:11'), findsOneWidget);
    expect(find.text('Continue on this device'), findsOneWidget);

    // Continue here: a short pull, the TV's position, a takeover.
    await t.tap(find.byKey(const ValueKey('handoff-continue')));
    await settle(t);
    expect(find.byType(PlayerScreen), findsOneWidget);
    expect(engine.calls.first, 'open:1:0:$_at:true');
    expect(sync.pulls, [const Duration(milliseconds: 600)]);
    expect(sync.started, [_movie]);
    expect(sync.heartbeats.first.positionMs, _at);
    expect(sync.heartbeats.first.state, SyncPlayState.playing);

    // Heartbeats every 5 s while playing and at once on pause.
    for (var i = 0; i < 6; i++) {
      engine.advance(const Duration(seconds: 1));
      await t.pump(const Duration(seconds: 1));
    }
    expect(sync.heartbeats.length, greaterThanOrEqualTo(2));
    expect(sync.heartbeats.last.state, SyncPlayState.playing);
    expect(sync.heartbeats.last.positionMs, greaterThan(_at));
    await engine.pause();
    await t.pump();
    expect(sync.heartbeats.last.state, SyncPlayState.paused);

    // The TV stops: no more banner. Closing the player sends "stopped".
    sync.receiveHeartbeat(
      deviceId: _tv,
      deviceName: _tvName,
      itemId: _movie,
      positionMs: _at,
      state: SyncPlayState.stopped,
    );
    await t.pump();
    expect(services.handoff.elsewhere, isEmpty);
    await _close(t);
    expect(sync.heartbeats.last.state, SyncPlayState.stopped);
  });

  testWidgets('banner in Russian', (t) async {
    setSurface(t, const Size(1280, 800));
    final (services, sync, _) = await _services(locale: const Locale('ru'));
    await t.pumpWidget(testApp(services));
    await settle(t);
    sync.receiveHeartbeat(
      deviceId: _tv,
      deviceName: _tvName,
      itemId: _movie,
      positionMs: _at,
      durationMs: _duration,
    );
    await settle(t, 4);
    await scrollHomeTo(t, find.byKey(const ValueKey('handoff-banner')));
    expect(find.text('Играет на $_tvName'), findsOneWidget);
    expect(find.text('Продолжить на этом устройстве'), findsOneWidget);
    sync.receiveHeartbeat(
      deviceId: _tv,
      itemId: _movie,
      positionMs: _at,
      state: SyncPlayState.paused,
    );
    await settle(t, 2);
    expect(find.byKey(const ValueKey('handoff-banner')), findsNothing);
  });

  testWidgets('toast: another device is ahead — jump or dismiss', (t) async {
    setSurface(t, const Size(1280, 800));
    final (services, sync, engine) = await _services();
    await t.pumpWidget(testApp(services));
    await settle(t);
    await _play(t, startMs: 600000);
    expect(engine.calls.first, 'open:1:0:600000:true');
    expect(find.byKey(const ValueKey('handoff-offer')), findsNothing);

    // A late heartbeat of the TV, far ahead.
    sync.receiveHeartbeat(
      deviceId: _tv,
      deviceName: _tvName,
      itemId: _movie,
      positionMs: _at,
      durationMs: _duration,
      state: SyncPlayState.paused,
    );
    await settle(t, 2);
    expect(find.byKey(const ValueKey('handoff-offer')), findsOneWidget);
    expect(find.text('Jump to 41:11?'), findsOneWidget);
    expect(find.text('$_tvName is further along'), findsOneWidget);
    await t.tap(find.byKey(const ValueKey('handoff-action')));
    await settle(t, 2);
    expect(engine.calls, contains('seek:$_at'));
    expect(find.byKey(const ValueKey('handoff-offer')), findsNothing);
    expect(sync.heartbeats.last.positionMs, _at);

    // A newer position again: dismissed, and not offered again from the TV.
    sync.receiveHeartbeat(
      deviceId: _tv,
      itemId: _movie,
      positionMs: _at + 600000,
      state: SyncPlayState.paused,
    );
    await settle(t, 2);
    expect(find.byKey(const ValueKey('handoff-offer')), findsOneWidget);
    await t.tap(find.byKey(const ValueKey('handoff-close')));
    await settle(t, 2);
    expect(find.byKey(const ValueKey('handoff-offer')), findsNothing);
    sync.receiveHeartbeat(
      deviceId: _tv,
      itemId: _movie,
      positionMs: _at + 900000,
      state: SyncPlayState.paused,
    );
    await settle(t, 2);
    expect(find.byKey(const ValueKey('handoff-offer')), findsNothing);
    expect(engine.calls.where((c) => c.startsWith('seek')).length, 1);
    await _close(t);
  });

  testWidgets('toast in Russian', (t) async {
    setSurface(t, const Size(1280, 800));
    final (services, sync, _) = await _services(locale: const Locale('ru'));
    await t.pumpWidget(testApp(services));
    await settle(t);
    await _play(t, startMs: 600000);
    sync.receiveHeartbeat(
      deviceId: _tv,
      deviceName: _tvName,
      itemId: _movie,
      positionMs: _at,
      state: SyncPlayState.paused,
    );
    await settle(t, 2);
    expect(find.text('Перейти к 41:11?'), findsOneWidget);
    expect(find.text('Перейти'), findsOneWidget);
    // The offer hides by itself.
    await t.pump(const Duration(seconds: 13));
    expect(find.byKey(const ValueKey('handoff-offer')), findsNothing);
    await _close(t);
  });

  testWidgets('takeover: the TV starts the title, the player pauses here', (
    t,
  ) async {
    setSurface(t, const Size(1280, 800));
    final (services, sync, engine) = await _services();
    await t.pumpWidget(testApp(services));
    await settle(t);
    await _play(t, startMs: 600000);
    expect(engine.state.value.playing, isTrue);

    // Another title on the TV does not touch this one.
    sync.receiveTakeover(deviceId: _tv, itemId: 'movie-the-quiet-orbit');
    await settle(t, 2);
    expect(engine.calls, isNot(contains('pause')));

    sync.receiveTakeover(deviceId: _tv, itemId: _movie);
    await settle(t, 2);
    expect(engine.calls, contains('pause'));
    expect(engine.state.value.playing, isFalse);
    expect(sync.heartbeats.last.state, SyncPlayState.paused);
    expect(find.text('Paused: continues on $_tvName'), findsOneWidget);
    await t.pump(const Duration(seconds: 5));
    expect(find.byKey(const ValueKey('handoff-paused')), findsNothing);
    await _close(t);
  });

  testWidgets('takeover pause can be turned off in Settings', (t) async {
    setSurface(t, const Size(1280, 800));
    final prefs = MemoryPreferences();
    final (services, sync, engine) = await _services(preferences: prefs);
    expect(services.handoff.pauseOnTakeover.value, isTrue);
    await t.pumpWidget(testApp(services, section: Section.settings));
    await settle(t);
    final tile = find.byKey(const ValueKey('settings-takeover-pause'));
    await t.scrollUntilVisible(
      tile,
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Pause when another device takes over'), findsOneWidget);
    await t.tap(tile);
    await settle(t, 2);
    expect(services.handoff.pauseOnTakeover.value, isFalse);
    expect(await prefs.read(Preferences.takeoverPause), 'false');

    await _play(t, startMs: 600000);
    sync.receiveTakeover(deviceId: _tv, itemId: _movie);
    await settle(t, 2);
    expect(engine.calls, isNot(contains('pause')));
    expect(engine.state.value.playing, isTrue);
    await _close(t);
  });

  testWidgets('two fakes: the start on one pauses the other', (t) async {
    setSurface(t, const Size(1280, 800));
    final lan = FakeSyncLan();
    final phone = FakeDeviceSync(id: 'kphone', name: 'Phone', lan: lan);
    final tv = FakeDeviceSync(id: 'ktv', name: 'TV', lan: lan);
    final invite = await phone.invite();
    await tv.join(invite.code);
    final engine = FakePlayerEngine(
      loadedDuration: const Duration(milliseconds: _duration),
    );
    final services = AppServices(
      library: FakeLibrary.demo(scanStepDelay: Duration.zero),
      secureStore: MemorySecureStore(),
      playerEngineFactory: ({bool video = true}) => engine,
      sync: phone,
      clock: () => demoNow,
    );
    await services.init();
    await t.pumpWidget(testApp(services));
    await settle(t);
    await _play(t, startMs: 600000);
    // The phone's heartbeat reached the TV: it would show the banner.
    expect((await tv.playingElsewhere()).single.itemId, _movie);
    expect((await tv.resumeHint(_movie))!.positionMs, 600000);

    // The TV starts the title: the phone pauses and says where it stopped.
    await tv.startPlaying(_movie);
    await settle(t, 2);
    expect(engine.state.value.playing, isFalse);
    final hint = (await tv.resumeHint(_movie))!;
    expect(hint.playing, isFalse);
    expect(hint.deviceId, 'kphone');
    expect(await tv.playingElsewhere(), isEmpty);
    await _close(t);
  });
}
