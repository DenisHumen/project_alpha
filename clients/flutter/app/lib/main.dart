// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';
import 'package:pa_platform_secrets/pa_platform_secrets.dart';
import 'package:path_provider/path_provider.dart';

import 'app.dart';
import 'config.dart';
import 'fakes/fake_library.dart';
import 'integration/library.dart';
import 'integration/player.dart';
import 'licenses.dart';
import 'platform/form_factor.dart';
import 'platform/libmpv_check.dart';
import 'services/app_services.dart';
import 'services/preferences.dart';
import 'services/secure_store.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  ensurePaPlayerInitialized();
  registerAppLicenses();

  final formFactor = await detectFormFactor();
  final dir = await getApplicationSupportDirectory();
  final Library library;
  var rescan = false;
  if (AppConfig.demo) {
    library = FakeLibrary.demo();
  } else {
    // Rust core by default (UI-024); the v0 database is imported on the first
    // start and kept as kinolune.db.v0.bak. v0 is the fallback.
    final opened = await openAppLibrary(dir.path, rustCore: AppConfig.rustCore);
    library = opened.library;
    rescan = opened.importedV0;
    if (opened.fallbackReason != null) {
      debugPrint('Library on the v0 fallback: ${opened.fallbackReason}');
    }
    // CORE-015: on Android the core keeps passwords and tokens in the
    // Keystore through this callback; desktop uses the OS keychain itself.
    if (opened.onRustCore) await _openCoreSecrets(dir.path);
  }
  final services = AppServices(
    library: library,
    secureStore: PlatformSecureStore(),
    preferences: FilePreferences('${dir.path}/preferences.json'),
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

Future<void> _openCoreSecrets(String dataDir) async {
  try {
    final info = await openPlatformSecrets(dataDir: dataDir);
    if (info != null) debugPrint('Core secrets: ${info.backend}');
  } catch (e) {
    // Sources with passwords fail with a `secrets` error until the next start.
    debugPrint('Core secret store unavailable: $e');
  }
}
