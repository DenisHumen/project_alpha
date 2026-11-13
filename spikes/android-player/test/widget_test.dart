// SPDX-License-Identifier: MPL-2.0
import 'package:android_player_spike/main.dart';
import 'package:android_player_spike/osd.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('home lists all four variants and sends options to the native side', (tester) async {
    const channel = MethodChannel('pa_spike/main');
    final calls = <MethodCall>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return null;
    });

    await tester.pumpWidget(const MaterialApp(home: HomePage()));
    for (final v in Variant.values) {
      expect(find.text(v.label), findsOneWidget);
    }

    await tester.tap(find.text('surface'));
    await tester.tap(find.text(Variant.activityFlutterOsd.label));
    await tester.pump();

    expect(calls.single.method, 'openActivity');
    final args = calls.single.arguments as Map<Object?, Object?>;
    expect(args['variant'], 'activity_flutter_osd');
    expect(args['afr'], 'surface');
    expect(args['url'], defaultUrl);
  });

  testWidgets('OSD renders stats and forwards transport commands', (tester) async {
    const channel = MethodChannel('pa_spike/test_osd');
    final methods = <String>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      methods.add(call.method);
      return call.method == 'stats' ? {'decoder': 'c2.test.hevc.decoder', 'is_playing': true} : null;
    });

    await tester.pumpWidget(const MaterialApp(home: Osd(channel: channel)));
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(find.textContaining('c2.test.hevc.decoder'), findsOneWidget);

    await tester.tap(find.byIcon(Icons.pause));
    await tester.tap(find.byIcon(Icons.forward_10));
    expect(methods, containsAllInOrder(['stats', 'toggle', 'seekBy']));
    await tester.pumpWidget(const SizedBox());
  });
}
