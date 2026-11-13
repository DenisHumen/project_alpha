// SPDX-License-Identifier: MPL-2.0
//
// UI strings (UI-020). Messages live in app_<locale>.arb next to this file;
// `flutter pub get` generates app_localizations*.dart (not committed).
// Use `context.l10n.someKey` in widgets; never hardcode user-facing text
// (test/l10n_test.dart scans lib/ for it).

import 'package:material_ui/material_ui.dart';

import '../integration/player.dart';
import 'app_localizations.dart';

export 'app_localizations.dart';

extension L10nContext on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);
}

/// Android's standard pseudo-locale: accented, ~40 % longer English generated
/// from app_en.arb by `dart run tool/pseudo_l10n.dart`. Offered in Settings in
/// debug builds; also picked when the device itself is set to en-XA.
const kPseudoLocale = Locale('en', 'XA');

/// Delegates for MaterialApp: our strings, the player's own strings
/// (pa_player, ru/en, UI-023) plus Material/Cupertino/Widgets strings from
/// material_ui (the generated `localizationsDelegates` list points at the
/// flutter_localizations copy of the Material delegate, which the
/// material_ui widgets do not read).
const List<LocalizationsDelegate<Object?>> kLocalizationsDelegates = [
  AppLocalizations.delegate,
  PaPlayerStrings.delegate,
  ...GlobalMaterialLocalizations.delegates,
];

/// `en`, `ru`, `en-XA`.
String localeTag(Locale l) => l.toLanguageTag();

/// Parses [localeTag]; null for empty or unsupported tags.
Locale? parseLocaleTag(String? tag) {
  if (tag == null || tag.isEmpty) return null;
  for (final l in AppLocalizations.supportedLocales) {
    if (l.toLanguageTag() == tag) return l;
  }
  return null;
}
