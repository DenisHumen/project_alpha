// SPDX-License-Identifier: MPL-2.0
//
// Renders the placeholder logo (lib/widgets/logo.dart) into launcher icons,
// the Android TV banner and the Linux icon. Run from clients/flutter/app:
//   KINOLUNE_GENERATE_ICONS=1 flutter test test/tool/generate_icons_test.dart

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/branding.dart';
import 'package:kinolune/widgets/logo.dart';
import 'package:material_ui/material_ui.dart';

import '../support/test_app.dart';

final _enabled = Platform.environment['KINOLUNE_GENERATE_ICONS'] == '1';

Future<void> _render(
  WidgetTester tester,
  Widget child,
  Size size,
  String path,
) async {
  final key = GlobalKey();
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    Directionality(
      textDirection: TextDirection.ltr,
      child: Center(
        child: RepaintBoundary(
          key: key,
          child: SizedBox.fromSize(size: size, child: child),
        ),
      ),
    ),
  );
  final boundary =
      key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  final bytes = await tester.runAsync(() async {
    final image = await boundary.toImage();
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    return data!.buffer.asUint8List();
  });
  File(path)
    ..parent.createSync(recursive: true)
    ..writeAsBytesSync(bytes!);
}

void main() {
  setUpAll(loadAppFonts);

  testWidgets('generate icons', (tester) async {
    const res = 'android/app/src/main/res';
    const legacy = {
      'mdpi': 48.0,
      'hdpi': 72.0,
      'xhdpi': 96.0,
      'xxhdpi': 144.0,
      'xxxhdpi': 192.0,
    };
    for (final e in legacy.entries) {
      await _render(
        tester,
        const CustomPaint(painter: LogoPainter()),
        Size.square(e.value),
        '$res/mipmap-${e.key}/ic_launcher.png',
      );
      // Adaptive icon foreground: 108 dp canvas, mark inside the 66 dp safe zone.
      await _render(
        tester,
        const CustomPaint(
          painter: LogoPainter(background: false, markScale: .62),
        ),
        Size.square(e.value * 108 / 48),
        '$res/mipmap-${e.key}/ic_launcher_foreground.png',
      );
    }
    // Android TV banner: 320x180 dp → xhdpi 640x360 px.
    await _render(
      tester,
      DecoratedBox(
        decoration: const BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [kLogoBgTop, kLogoBgBottom],
          ),
        ),
        child: Center(
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const SizedBox.square(
                dimension: 150,
                child: CustomPaint(painter: LogoPainter(background: false)),
              ),
              const SizedBox(width: 18),
              Text(
                kAppName,
                style: const TextStyle(
                  fontFamily: 'Roboto',
                  fontSize: 78,
                  fontWeight: FontWeight.w700,
                  color: Colors.white,
                  letterSpacing: 1,
                ),
              ),
            ],
          ),
        ),
      ),
      const Size(640, 360),
      '$res/drawable-xhdpi/banner.png',
    );
    // Linux / packaging icon.
    await _render(
      tester,
      const CustomPaint(painter: LogoPainter()),
      const Size.square(512),
      'assets/icon/kinolune.png',
    );
  }, skip: !_enabled);
}
