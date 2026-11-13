// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path_provider/path_provider.dart';

import 'app.dart';
import 'branding.dart';
import 'config.dart';
import 'fakes/fake_library.dart';
import 'integration/library.dart';
import 'platform/form_factor.dart';
import 'services/app_services.dart';
import 'services/secure_store.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  LicenseRegistry.addLicense(() async* {
    yield const LicenseEntryWithLineBreaks(
      [kAppName],
      '$kAppName is licensed under the Mozilla Public License, v. 2.0. '
      'You can obtain a copy at https://mozilla.org/MPL/2.0/.\n\n'
      'Source code: $kRepoUrl',
    );
    yield const LicenseEntryWithLineBreaks(['TMDB'], kTmdbNotice);
  });

  final formFactor = await detectFormFactor();
  final Library library;
  if (AppConfig.demo) {
    library = FakeLibrary.demo();
  } else {
    final dir = await getApplicationSupportDirectory();
    library = await Library.open('${dir.path}/kinolune.db');
  }
  final services = AppServices(library: library, secureStore: PlatformSecureStore());
  await services.init();
  runApp(KinoluneApp(services: services, formFactor: formFactor));
}
