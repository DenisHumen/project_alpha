// SPDX-License-Identifier: MPL-2.0

import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/app.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:material_ui/material_ui.dart';

bool _fontsLoaded = false;

/// Loads Roboto and Material Icons from the Flutter SDK cache so screenshots
/// show real text instead of the test font boxes.
Future<void> loadAppFonts() async {
  if (_fontsLoaded) return;
  _fontsLoaded = true;
  final dir = _materialFontsDir();
  if (dir == null) return;
  Future<void> load(String family, List<String> files) async {
    final loader = FontLoader(family);
    for (final f in files) {
      final file = File('${dir.path}/$f');
      if (file.existsSync()) loader.addFont(Future.value(ByteData.sublistView(file.readAsBytesSync())));
    }
    await loader.load();
  }

  await load('Roboto', ['Roboto-Regular.ttf', 'Roboto-Medium.ttf', 'Roboto-Bold.ttf', 'Roboto-Black.ttf', 'Roboto-Light.ttf']);
  await load('MaterialIcons', ['MaterialIcons-Regular.otf']);
}

Directory? _materialFontsDir() {
  final candidates = <String>[
    if (Platform.environment['FLUTTER_ROOT'] != null) '${Platform.environment['FLUTTER_ROOT']}/bin/cache/artifacts/material_fonts',
  ];
  // flutter_tester lives in <flutter>/bin/cache/artifacts/engine/<platform>/.
  var d = File(Platform.resolvedExecutable).parent;
  for (var i = 0; i < 4; i++) {
    candidates.add('${d.path}/material_fonts');
    candidates.add('${d.path}/artifacts/material_fonts');
    d = d.parent;
  }
  for (final c in candidates) {
    final dir = Directory(c);
    if (dir.existsSync()) return dir;
  }
  return null;
}

Future<AppServices> demoServices({bool empty = false}) async {
  final s = AppServices(
    library: empty ? FakeLibrary(scanStepDelay: Duration.zero) : FakeLibrary.demo(scanStepDelay: Duration.zero),
    secureStore: MemorySecureStore(),
  );
  await s.init();
  return s;
}

final screenshotKey = GlobalKey();

Widget testApp(AppServices services, {FormFactor formFactor = FormFactor.desktop, Section section = Section.home}) {
  return RepaintBoundary(
    key: screenshotKey,
    child: KinoluneApp(services: services, formFactor: formFactor, initialSection: section),
  );
}

void setSurface(WidgetTester tester, Size logical, {double dpr = 1}) {
  tester.view.devicePixelRatio = dpr;
  tester.view.physicalSize = logical * dpr;
  addTearDown(tester.view.reset);
}

/// Pumps a few frames without waiting for repeating animations to settle.
Future<void> settle(WidgetTester tester, [int frames = 12]) async {
  for (var i = 0; i < frames; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

Future<void> pressKey(WidgetTester tester, LogicalKeyboardKey key, {int times = 1}) async {
  for (var i = 0; i < times; i++) {
    await tester.sendKeyEvent(key);
    await tester.pump(const Duration(milliseconds: 80));
  }
  await settle(tester, 4);
}

/// Writes the current frame to `$KINOLUNE_SCREENSHOTS_DIR/<name>.png`.
Future<void> capture(WidgetTester tester, String name) async {
  final dir = Platform.environment['KINOLUNE_SCREENSHOTS_DIR'];
  if (dir == null || dir.isEmpty) return;
  final boundary = screenshotKey.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  final dpr = tester.view.devicePixelRatio;
  final bytes = await tester.runAsync(() async {
    final ui.Image image = await boundary.toImage(pixelRatio: dpr);
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    return data!.buffer.asUint8List();
  });
  final file = File('$dir/$name.png');
  file.parent.createSync(recursive: true);
  file.writeAsBytesSync(bytes! as Uint8List);
}
