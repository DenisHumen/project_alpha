// SPDX-License-Identifier: MPL-2.0
// Smoke app for pa_core: loads the bundled libpa_frb by name (as the real app
// does), shows coreVersion() and the values of a Rust → Dart stream.
import 'package:flutter/widgets.dart';
import 'package:pa_core/pa_core.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const PaCoreExampleApp());
}

/// Shows the core version and the ticks of `tickStream`.
class PaCoreExampleApp extends StatelessWidget {
  /// Creates the smoke app.
  const PaCoreExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return WidgetsApp(
      color: const Color(0xFF101014),
      textStyle: const TextStyle(fontSize: 20, color: Color(0xFFFFFFFF)),
      builder: (context, _) => ColoredBox(
        color: const Color(0xFF101014),
        child: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text('core ${coreVersion()}', key: const Key('core-version')),
              StreamBuilder<List<int>>(
                stream: _collect(tickStream(count: 3)),
                builder: (context, snapshot) => Text(
                  'ticks ${snapshot.data?.join(',') ?? ''}',
                  key: const Key('ticks'),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  static Stream<List<int>> _collect(Stream<int> ticks) async* {
    final seen = <int>[];
    await for (final tick in ticks) {
      seen.add(tick);
      yield List.unmodifiable(seen);
    }
  }
}
