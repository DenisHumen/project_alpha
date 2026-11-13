// SPDX-License-Identifier: MPL-2.0
// Runs the example as a real app (bundled libpa_frb, loaded by name):
//   flutter test integration_test -d linux     (or an Android device/emulator)
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pa_core/pa_core.dart';
import 'package:pa_core_example/main.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async => RustLib.init());

  testWidgets('shows the core version and stream ticks', (tester) async {
    await tester.pumpWidget(const PaCoreExampleApp());
    await tester.pumpAndSettle();

    expect(coreVersion(), matches(RegExp(r'^\d+\.\d+\.\d+$')));
    expect(find.text('core ${coreVersion()}'), findsOneWidget);
    expect(find.text('ticks 0,1,2'), findsOneWidget);
  });
}
