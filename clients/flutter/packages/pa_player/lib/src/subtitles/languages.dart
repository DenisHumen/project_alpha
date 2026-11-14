// SPDX-License-Identifier: MPL-2.0

/// Track languages come as ISO 639-1 (`ru`), 639-2/B (`ger`), 639-2/T
/// (`deu`), BCP 47 (`pt-BR`) or a name (`Russian`). The core's folder
/// analysis writes 639-2/B (pa-scanner `language`); this table maps the same
/// languages so a sidecar and an embedded track of one language compare
/// equal.
const Map<String, List<String>> _languages = {
  'rus': ['ru', 'rus', 'russian', 'русский'],
  'eng': ['en', 'eng', 'english', 'английский'],
  'ukr': ['uk', 'ukr', 'ukrainian', 'українська', 'украинский'],
  'bel': ['be', 'bel', 'belarusian'],
  'kaz': ['kk', 'kaz', 'kazakh'],
  'jpn': ['ja', 'jpn', 'japanese'],
  'ger': ['de', 'ger', 'deu', 'german', 'deutsch'],
  'fre': ['fr', 'fre', 'fra', 'french', 'français', 'francais'],
  'spa': ['es', 'spa', 'spanish', 'español', 'espanol'],
  'ita': ['it', 'ita', 'italian'],
  'por': ['pt', 'por', 'portuguese'],
  'chi': ['zh', 'chi', 'zho', 'chinese'],
  'kor': ['ko', 'kor', 'korean'],
  'pol': ['pl', 'pol', 'polish'],
  'cze': ['cs', 'cze', 'ces', 'czech'],
  'dut': ['nl', 'dut', 'nld', 'dutch'],
  'swe': ['sv', 'swe', 'swedish'],
  'nor': ['no', 'nb', 'nn', 'nor', 'nob', 'nno', 'norwegian'],
  'dan': ['da', 'dan', 'danish'],
  'fin': ['fi', 'fin', 'finnish'],
  'tur': ['tr', 'tur', 'turkish'],
  'ara': ['ar', 'ara', 'arabic'],
  'heb': ['he', 'heb', 'hebrew'],
  'hin': ['hi', 'hin', 'hindi'],
  'gre': ['el', 'gre', 'ell', 'greek'],
  'hun': ['hu', 'hun', 'hungarian'],
  'rum': ['ro', 'rum', 'ron', 'romanian'],
  'bul': ['bg', 'bul', 'bulgarian'],
  'srp': ['sr', 'srp', 'serbian'],
  'hrv': ['hr', 'hrv', 'croatian'],
  'tha': ['th', 'tha', 'thai'],
  'vie': ['vi', 'vie', 'vietnamese'],
  'ind': ['id', 'ind', 'indonesian'],
};

final Map<String, String> _byAlias = {
  for (final e in _languages.entries)
    for (final alias in e.value) alias: e.key,
};

/// ISO 639-2/B code of a track language tag, or null if it is empty or
/// unknown (`und`, `mis`, an unknown name). Region and script subtags are
/// dropped: `pt-BR` → `por`, `zh_Hant` → `chi`.
String? normalizeLanguage(String? tag) {
  if (tag == null) return null;
  var t = tag.trim().toLowerCase();
  if (t.isEmpty) return null;
  final cut = t.indexOf(RegExp('[-_]'));
  if (cut > 0) t = t.substring(0, cut);
  return _byAlias[t];
}
