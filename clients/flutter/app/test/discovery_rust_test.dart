// SPDX-License-Identifier: MPL-2.0
//
// NET-020 on the real Rust core: an SMB share announced over mDNS shows up
// in "On this network" of the add-source flow and prefills the form with
// the address the core resolved.
//
// By default everything stays on loopback and a private UDP port. With
// PA_LAN_SMB_INSTANCE set the app browses the real network (port 5353) and
// waits for that instance, e.g. the Samba/Avahi container of
// core/crates/pa-discovery/tests/servers:
//   PA_LAN_SMB_INSTANCE='NET-020 Samba' KINOLUNE_SCREENSHOTS_DIR=/tmp/shots \
//     flutter test test/discovery_rust_test.dart

import 'dart:io';
import 'dart:math';

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/l10n/l10n.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/sources/core_discovery.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_core/pa_core.dart';

import 'support/native_library.dart';
import 'support/test_app.dart';

final en = lookupAppLocalizations(const Locale('en'));

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  final lan = Platform.environment['PA_LAN_SMB_INSTANCE'];
  final port = lan == null ? 30000 + Random().nextInt(20000) : null;
  final want = lan ?? 'Loopback NAS';
  late CoreSourceDiscovery discovery;
  CoreDiscovery? announcer;

  // The core is opened outside the widget test's fake-async zone.
  setUpAll(() async {
    await loadAppFonts();
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
    discovery = await CoreSourceDiscovery.open(loopbackPort: port);
    if (port != null) {
      announcer = await CoreDiscovery.open(
        config: CoreDiscoveryConfig(loopbackPort: port, ownIds: const []),
        multicastLock: (_) {},
      );
      await announcer!.announce(
        kind: CoreServiceKind.smb,
        instance: want,
        port: 4450,
        txt: const [],
      );
    }
  });

  // Closing ends the core's streams; an open stream keeps the test
  // process alive.
  tearDownAll(() async {
    await discovery.close();
    await announcer?.close();
  });

  testWidgets('a share on the network is offered and prefills the form', (
    tester,
  ) async {
    setSurface(tester, const Size(1280, 900));
    final services = await demoServices(discovery: discovery);
    await tester.pumpWidget(testApp(services, section: Section.settings));
    await settle(tester);
    await tester.tap(find.text(en.addSourceTitle).first);
    await settle(tester, 4);
    expect(find.text(en.sourcesAddDiscovered), findsOneWidget);

    // Browsing runs on core threads: give real time between frames.
    for (var i = 0; i < 150 && find.text(want).evaluate().isEmpty; i++) {
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 200)),
      );
      await tester.pump();
    }
    expect(find.text(want), findsOneWidget);
    await capture(tester, 'net020-add-source-discovered');

    await tester.tap(find.text(want));
    await settle(tester, 4);
    final host = tester
        .widget<TextField>(find.byKey(const ValueKey('source-host')))
        .controller!
        .text;
    if (lan == null) {
      expect(host, '127.0.0.1:4450');
    } else {
      expect(host, matches(RegExp(r'^\d+\.\d+\.\d+\.\d+(:\d+)?$')));
    }
    await capture(tester, 'net020-add-source-prefilled');
  });
}
