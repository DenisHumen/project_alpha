// SPDX-License-Identifier: MPL-2.0
// Test helper (copy of pa_core/test/native_library.dart, as in pa_library): find the libpa_frb
// that the Native Assets hook of pa_core built for the host. In an app the
// library is bundled next to the executable (Linux) or in jniLibs (Android)
// and `RustLib.init()` finds it by name; in `dart test` it stays in the hook
// output directory, so tests open it by path.
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

/// Override for the library path, e.g. a `cargo build -p pa-frb` output.
const _envOverride = 'PA_CORE_LIB';

String get _libraryFileName {
  if (Platform.isWindows) return 'pa_frb.dll';
  if (Platform.isMacOS) return 'libpa_frb.dylib';
  return 'libpa_frb.so';
}

/// Opens the most recently built host libpa_frb.
ExternalLibrary openBuiltCoreLibrary() {
  final override = Platform.environment[_envOverride];
  if (override != null && override.isNotEmpty) {
    return ExternalLibrary.open(override);
  }
  final candidates = <File>[];
  for (final root in ['.dart_tool/hooks_runner', 'build/native_assets']) {
    final dir = Directory(root);
    if (!dir.existsSync()) continue;
    candidates.addAll(
      dir
          .listSync(recursive: true, followLinks: false)
          .whereType<File>()
          .where((f) => f.uri.pathSegments.last == _libraryFileName)
          // Skip cargo's intermediate copy in target/<triple>/<mode>/deps.
          .where(
            (f) => !f.parent.path.endsWith('${Platform.pathSeparator}deps'),
          ),
    );
  }
  if (candidates.isEmpty) {
    throw StateError(
      '$_libraryFileName not found under .dart_tool/hooks_runner or '
      'build/native_assets. Run the tests with `dart test` (it runs the '
      'Native Assets hook) or set $_envOverride.',
    );
  }
  candidates.sort(
    (a, b) => b.statSync().modified.compareTo(a.statSync().modified),
  );
  return ExternalLibrary.open(candidates.first.path);
}
