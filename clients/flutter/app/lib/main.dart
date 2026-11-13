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
    yield const LicenseEntryWithLineBreaks(
      ['Inter'],
      'Inter Variable font. Copyright (c) 2016 The Inter Project Authors '
      '(https://github.com/rsms/inter). Licensed under the SIL Open Font '
      'License, Version 1.1 (https://openfontlicense.org).',
    );
    yield const LicenseEntryWithLineBreaks(
      ['libmpv', 'FFmpeg'],
      '$kAppName plays media with libmpv (https://mpv.io) and FFmpeg '
      '(https://ffmpeg.org), used as separate shared libraries under the GNU '
      'Lesser General Public License v2.1 or later. Only LGPL builds are used. '
      'You may replace these libraries with your own builds. Source code: '
      'https://github.com/mpv-player/mpv and https://git.ffmpeg.org/ffmpeg.git; '
      'build scripts: $kRepoUrl.',
    );
  });

  final formFactor = await detectFormFactor();
  final Library library;
  if (AppConfig.demo) {
    library = FakeLibrary.demo();
  } else {
    final dir = await getApplicationSupportDirectory();
    library = await Library.open('${dir.path}/kinolune.db');
  }
  final services = AppServices(
    library: library,
    secureStore: PlatformSecureStore(),
  );
  await services.init();
  runApp(KinoluneApp(services: services, formFactor: formFactor));
}
