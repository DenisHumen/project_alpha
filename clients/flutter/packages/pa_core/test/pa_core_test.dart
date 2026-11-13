// SPDX-License-Identifier: MPL-2.0
// Calls into the real Rust core (libpa_frb) on the host. `dart test` runs the
// Native Assets hook first, which builds core/crates/pa-frb with cargo.
import 'package:pa_core/pa_core.dart';
import 'package:test/test.dart';

import 'native_library.dart';

void main() {
  setUpAll(() async {
    await RustLib.init(externalLibrary: openBuiltCoreLibrary());
  });

  tearDownAll(RustLib.dispose);

  group('coreVersion', () {
    test('returns MAJOR.MINOR.PATCH of the core', () {
      final version = coreVersion();
      expect(version, matches(RegExp(r'^\d+\.\d+\.\d+$')));
    });

    test('matches the pa-ffi crate version', () {
      expect(coreVersion(), coreCrateVersion());
    });
  });

  group('tickStream', () {
    test('emits 0..count-1 and closes', () async {
      expect(await tickStream(count: 5).toList(), [0, 1, 2, 3, 4]);
    });

    test('empty stream for count 0', () async {
      expect(await tickStream(count: 0).toList(), isEmpty);
    });

    test('subscriber can cancel early', () async {
      expect(await tickStream(count: 1000).take(3).toList(), [0, 1, 2]);
    });
  });
}
