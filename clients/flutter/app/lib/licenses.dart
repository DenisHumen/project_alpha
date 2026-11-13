// SPDX-License-Identifier: MPL-2.0
//
// Our own licence notices for the licence page (About → Licenses). The
// Flutter tool adds the pub packages' licences itself (NOTICES asset).
// Registered from KinoluneApp, not only from main(), so every way of
// building the app (tests, screenshot tooling) shows them (UI-023).
// Licence texts stay in English (legal notices, shown verbatim).

import 'package:flutter/foundation.dart';

import 'branding.dart';

bool _registered = false;

/// Adds the app's licence entries to [LicenseRegistry]; safe to call twice.
void registerAppLicenses() {
  if (_registered) return;
  _registered = true;
  LicenseRegistry.addLicense(_appLicenses);
}

Stream<LicenseEntry> _appLicenses() async* {
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
}
