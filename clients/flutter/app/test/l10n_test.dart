// SPDX-License-Identifier: MPL-2.0
//
// Localisation checks (UI-020). They run in CI with `flutter test`:
// * every key of app_en.arb is translated in app_ru.arb (and nothing extra),
//   placeholders match, Russian plurals have one/few/many;
// * app_en_XA.arb (pseudo-locale) is up to date with app_en.arb;
// * no hardcoded user-facing strings in lib/ (heuristic, see _hardcoded);
// * Russian plural forms and number formats.

import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/l10n/l10n.dart';

import '../tool/pseudo_l10n.dart';

Map<String, dynamic> _arb(String locale) =>
    jsonDecode(File('lib/l10n/app_$locale.arb').readAsStringSync())
        as Map<String, dynamic>;

Set<String> _keys(Map<String, dynamic> arb) =>
    arb.keys.where((k) => !k.startsWith('@')).toSet();

/// Placeholder names used in an ICU message (`{name}` and `{name, plural…`).
Set<String> _placeholders(String message) => {
  for (final m in RegExp(r'\{(\w+)[,}]').allMatches(message)) m.group(1)!,
};

// ------------------------------------------------------------ hardcoded text

/// Files that legitimately contain English literals: generated l10n, demo
/// content (fictional titles), licence notices and product constants.
const _exempt = [
  'lib/l10n/',
  'lib/fakes/',
  'lib/branding.dart',
  'lib/licenses.dart',
  'lib/main.dart',
];

/// Widget arguments that are shown to the user.
final _uiArg = RegExp(
  r'''\b(Text|SelectableText|Tooltip)\(\s*(['"])((?:(?!\2).)*)\2'''
  r'''|\b(tooltip|label|labelText|hintText|helperText|errorText|title|subtitle|message|semanticLabel|semanticsLabel|dialogTitle|content)\s*:\s*(['"])((?:(?!\5).)*)\5''',
  dotAll: true,
);

/// Sentence-like literals anywhere: `'Capitalised lower…'` (not `'Noto Sans'`).
final _sentence = RegExp(r'''(['"])([A-Z][a-z]* [a-z][^'"]*)\1''');

/// Replaces [pattern] matches by their line breaks only, so line numbers stay.
String _blank(String code, RegExp pattern) => code.replaceAllMapped(
  pattern,
  (m) => '\n' * '\n'.allMatches(m.group(0)!).length,
);

bool _hasWords(String literal) {
  // Interpolations and escapes are not text.
  final text = literal
      .replaceAll(RegExp(r'\$\{[^}]*\}'), '')
      .replaceAll(RegExp(r'\$\w+'), '')
      .replaceAll(RegExp(r'\\.'), '');
  return RegExp('[A-Za-zА-Яа-яЁё]{2,}').hasMatch(text);
}

List<String> _hardcoded() {
  final found = <String>[];
  final files = Directory('lib')
      .listSync(recursive: true)
      .whereType<File>()
      .where((f) => f.path.endsWith('.dart'));
  for (final f in files) {
    final path = f.path.replaceAll(r'\', '/');
    if (_exempt.any(path.startsWith)) continue;
    final lines = f.readAsLinesSync();
    // Drop comments and lines marked `// l10n-ignore`.
    var code = [
      for (final line in lines)
        line.contains('l10n-ignore') || line.trimLeft().startsWith('//')
            ? ''
            : line,
    ].join('\n');
    // Interpolations (may hold nested quotes) and log calls are not UI text.
    var prev = '';
    while (prev != code) {
      prev = code;
      code = _blank(code, RegExp(r'\$\{[^{}]*\}'));
    }
    code = _blank(code, RegExp(r'debugPrint\([^;]*\);', dotAll: true));
    int lineOf(int offset) => '\n'.allMatches(code.substring(0, offset)).length;
    for (final m in _uiArg.allMatches(code)) {
      final literal = m.group(3) ?? m.group(6)!;
      if (_hasWords(literal)) {
        found.add('$path:${lineOf(m.start) + 1}: ${m.group(0)!.trim()}');
      }
    }
    for (final m in _sentence.allMatches(code)) {
      if (lines[lineOf(m.start)].contains('Exception(')) continue;
      found.add('$path:${lineOf(m.start) + 1}: ${m.group(0)}');
    }
  }
  return found.toSet().toList()..sort();
}

void main() {
  group('ARB files', () {
    final en = _arb('en');
    final ru = _arb('ru');

    test('ru has exactly the keys of en', () {
      final missing = _keys(en).difference(_keys(ru));
      final extra = _keys(ru).difference(_keys(en));
      expect(missing, isEmpty, reason: 'Untranslated keys in app_ru.arb');
      expect(extra, isEmpty, reason: 'Keys missing from app_en.arb');
    });

    test('translations are non-empty and keep placeholders', () {
      for (final k in _keys(en)) {
        final r = ru[k] as String?;
        if (r == null) continue;
        expect(r.trim(), isNotEmpty, reason: k);
        expect(
          _placeholders(r),
          _placeholders(en[k] as String),
          reason: 'Placeholders differ in "$k"',
        );
      }
    });

    test('Russian plurals have one, few and many', () {
      for (final k in _keys(en)) {
        if (!(en[k] as String).contains(', plural,')) continue;
        final r = ru[k] as String;
        for (final form in ['one{', 'few{', 'many{', 'other{']) {
          expect(r, contains(form), reason: '"$k" lacks $form…}');
        }
      }
    });

    test('keys carry a feature prefix', () {
      const prefixes = [
        'common',
        'nav',
        'app',
        'shell',
        'home',
        'library',
        'music',
        'search',
        'detail',
        'show',
        'format',
        'fixMatch',
        'addSource',
        'settings',
        'about',
        'onboarding',
      ];
      for (final k in _keys(en)) {
        expect(
          prefixes.any((p) => k.startsWith(p)),
          isTrue,
          reason: '"$k" has no known feature prefix (add one to the list)',
        );
      }
    });

    test('pseudo-locale app_en_XA.arb is up to date', () {
      final expected = buildPseudoArb(File(enArbPath).readAsStringSync());
      expect(
        File(pseudoArbPath).readAsStringSync(),
        expected,
        reason: 'Run `dart run tool/pseudo_l10n.dart` in clients/flutter/app',
      );
    });
  });

  test('no hardcoded user-facing strings in lib/', () {
    final found = _hardcoded();
    expect(
      found,
      isEmpty,
      reason:
          'Move these strings to lib/l10n/app_en.arb and app_ru.arb, or mark '
          'a non-translatable literal with `// l10n-ignore`:\n'
          '${found.join('\n')}',
    );
  });

  group('messages', () {
    final ru = lookupAppLocalizations(const Locale('ru'));
    final en = lookupAppLocalizations(const Locale('en'));

    test('Russian plural forms', () {
      expect(ru.libraryMovieCount(1), '1 фильм');
      expect(ru.libraryMovieCount(3), '3 фильма');
      expect(ru.libraryMovieCount(5), '5 фильмов');
      expect(ru.libraryMovieCount(11), '11 фильмов');
      expect(ru.libraryMovieCount(21), '21 фильм');
      expect(ru.libraryMovieCount(0), '0 фильмов');
      expect(ru.showEpisodeCount(2), '2 серии');
      expect(ru.showEpisodeCount(25), '25 серий');
      expect(ru.musicTrackCount(1), '1 трек');
      expect(ru.settingsScanFiles(3, 21), 'Сканирование… 3 из 21 файла');
      expect(ru.settingsScanFiles(3, 40), 'Сканирование… 3 из 40 файлов');
    });

    test('English plural forms', () {
      expect(en.libraryMovieCount(1), '1 title');
      expect(en.libraryMovieCount(14), '14 titles');
      expect(en.showSeasonCount(1), '1 season');
      expect(en.showSeasonCount(3), '3 seasons');
    });

    test('percentages follow the locale', () {
      expect(en.detailMatched(.92), 'Matched 92%');
      expect(ru.detailMatched(.92), 'Распознано на 92 %');
    });

    test('pseudo-locale accents and pads English', () {
      final xa = lookupAppLocalizations(kPseudoLocale);
      expect(xa.navHome, startsWith('[Ĥóɱé'));
      expect(xa.navHome.length, greaterThan(en.navHome.length));
      expect(xa.libraryMovieCount(2), contains('2'));
    });
  });
}
