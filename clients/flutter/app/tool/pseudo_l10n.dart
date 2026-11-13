// SPDX-License-Identifier: MPL-2.0
//
// Generates the pseudo-locale lib/l10n/app_en_XA.arb from app_en.arb
// (UI-020): every letter gets an accent and every text run is padded by
// ~40 %, so clipped or hardcoded strings stand out. ICU syntax (placeholders,
// plural/select keywords) is left intact. Run from clients/flutter/app after
// changing app_en.arb:
//
//   dart run tool/pseudo_l10n.dart
//
// test/l10n_test.dart fails when the committed file is stale.

import 'dart:convert';
import 'dart:io';

const enArbPath = 'lib/l10n/app_en.arb';
const pseudoArbPath = 'lib/l10n/app_en_XA.arb';

const _accents = {
  'a': 'á', 'b': 'ƀ', 'c': 'ç', 'd': 'ð', 'e': 'é', 'f': 'ƒ', 'g': 'ĝ', //
  'h': 'ĥ', 'i': 'í', 'j': 'ĵ', 'k': 'ķ', 'l': 'ļ', 'm': 'ɱ', 'n': 'ñ', //
  'o': 'ó', 'p': 'þ', 'q': 'ǫ', 'r': 'ŕ', 's': 'š', 't': 'ţ', 'u': 'ú', //
  'v': 'ṽ', 'w': 'ŵ', 'x': 'ẋ', 'y': 'ý', 'z': 'ž', //
  'A': 'Å', 'B': 'Ɓ', 'C': 'Ç', 'D': 'Ð', 'E': 'É', 'F': 'Ƒ', 'G': 'Ĝ', //
  'H': 'Ĥ', 'I': 'Í', 'J': 'Ĵ', 'K': 'Ķ', 'L': 'Ļ', 'M': 'Ṁ', 'N': 'Ñ', //
  'O': 'Ó', 'P': 'Þ', 'Q': 'Ǫ', 'R': 'Ŕ', 'S': 'Š', 'T': 'Ţ', 'U': 'Û', //
  'V': 'Ṽ', 'W': 'Ŵ', 'X': 'Ẋ', 'Y': 'Ý', 'Z': 'Ž',
};

String _pseudoText(String text) {
  if (!text.contains(RegExp('[A-Za-z]'))) return text;
  final b = StringBuffer();
  for (final ch in text.split('')) {
    b.write(_accents[ch] ?? ch);
  }
  // Pad by ~40 % (at least one char) so longer translations are simulated.
  final pad = (text.trim().length * 0.4).ceil().clamp(1, 1 << 20);
  final out = b.toString();
  final trailing = out.length - out.trimRight().length;
  return '${out.trimRight()}${'·' * pad}${out.substring(out.length - trailing)}';
}

/// Pseudo-localises one ICU message: text at even brace depth is message text,
/// odd depth is syntax (`{count, plural, one` / `{name}`).
String pseudoMessage(String message) {
  final out = StringBuffer();
  final run = StringBuffer();
  var depth = 0;
  void flush() {
    if (run.isEmpty) return;
    out.write(depth.isEven ? _pseudoText(run.toString()) : run.toString());
    run.clear();
  }

  for (final ch in message.split('')) {
    if (ch == '{' || ch == '}') {
      flush();
      depth += ch == '{' ? 1 : -1;
      out.write(ch);
    } else {
      run.write(ch);
    }
  }
  flush();
  final s = out.toString();
  // Brackets mark both ends of plain messages; whole-message plurals keep
  // their ICU form untouched at the top level.
  return s.startsWith('{') && s.endsWith('}') ? s : '[$s]';
}

/// The pseudo ARB as a JSON string (stable key order = app_en.arb order).
String buildPseudoArb(String enArb) {
  final en = jsonDecode(enArb) as Map<String, dynamic>;
  final out = <String, Object?>{'@@locale': 'en_XA'};
  for (final e in en.entries) {
    if (e.key.startsWith('@')) continue;
    out[e.key] = pseudoMessage(e.value as String);
  }
  return '${const JsonEncoder.withIndent('  ').convert(out)}\n';
}

void main() {
  final pseudo = buildPseudoArb(File(enArbPath).readAsStringSync());
  File(pseudoArbPath).writeAsStringSync(pseudo);
  stdout.writeln('Wrote $pseudoArbPath');
}
