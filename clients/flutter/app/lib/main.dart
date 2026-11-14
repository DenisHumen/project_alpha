// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'app.dart';
import 'config.dart';
import 'fakes/fake_library.dart';
import 'fakes/fake_source_manager.dart';
import 'integration/library.dart';
import 'integration/player.dart';
import 'licenses.dart';
import 'platform/form_factor.dart';
import 'platform/libmpv_check.dart';
import 'services/app_services.dart';
import 'services/preferences.dart';
import 'services/secure_store.dart';
import 'sources/core_source_manager.dart';
import 'sources/source_manager.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  ensurePaPlayerInitialized();
  registerAppLicenses();

  final formFactor = await detectFormFactor();
  final dir = await getApplicationSupportDirectory();
  final Library library;
  SourceManager? sourceManager;
  var rescan = false;
  if (AppConfig.demo) {
    library = FakeLibrary.demo();
    sourceManager = FakeSourceManager(library);
  } else {
    // Rust core by default (UI-024); the v0 database is imported on the first
    // start and kept as kinolune.db.v0.bak. v0 is the fallback.
    final opened = await openAppLibrary(dir.path, rustCore: AppConfig.rustCore);
    library = opened.library;
    rescan = opened.importedV0;
    if (opened.fallbackReason != null) {
      debugPrint('Library on the v0 fallback: ${opened.fallbackReason}');
    }
    // UI-005: the Rust source manager over the same database. Without it
    // (v0 fallback, or the secret store cannot open) the Sources screen
    // runs reduced: local folders only.
    if (opened.onRustCore) {
      try {
        sourceManager = await CoreSourceManager.open(
          dbPath: '${dir.path}/$coreDbFileName',
          dataDir: dir.path,
        );
      } catch (e) {
        debugPrint('Source manager unavailable: $e');
      }
    }
  }
  final services = AppServices(
    library: library,
    secureStore: PlatformSecureStore(),
    preferences: FilePreferences('${dir.path}/preferences.json'),
    sourceManager: sourceManager,
  );
  await services.init();
  // Music tags and network-share items of an imported v0 library come with
  // a scan.
  if (rescan) unawaited(services.scan());
  runApp(
    KinoluneApp(
      services: services,
      formFactor: formFactor,
      libmpvMissing: libmpvMissing(),
    ),
  );
}
