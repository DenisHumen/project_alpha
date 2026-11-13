// SPDX-License-Identifier: MPL-2.0

/// Product name shown in the UI. Working name per ADR-0011 (DEC-004); change it
/// here only. Native launch labels (AndroidManifest `android:label`, GTK window
/// title in linux/runner/my_application.cc) must be kept in sync.
const kAppName = 'Kinolune';

/// Shown in About. Keep in sync with `version:` in pubspec.yaml.
const kAppVersion = '0.1.0';

const kRepoUrl = 'https://github.com/DenisHumen/project_alpha';
const kTmdbUrl = 'https://www.themoviedb.org';

/// Attribution required by the TMDB API terms of use.
const kTmdbNotice =
    'This product uses the TMDB API but is not endorsed or certified by TMDB.';
