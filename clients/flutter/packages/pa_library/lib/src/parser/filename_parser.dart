// SPDX-License-Identifier: MPL-2.0
//
// Filename parser v0 (release 0.1). Written from scratch for project_alpha;
// it will be replaced by the Rust parser (META-005) via FFI in 0.2.
//
// Approach: tokenise the base name, then walk the tokens left to right. The
// title is everything before the first "stop" token: an episode marker, a
// release year, or a junk token (quality, codec, source, audio, ...).

import 'package:path/path.dart' as p;

/// Result of recognising a file name.
class ParsedName {
  /// Movie title or show title.
  final String title;

  /// Second title for "Русское / Original" style names.
  final String? altTitle;
  final int? year;

  /// True when the name looks like an episode of a show.
  final bool isEpisode;
  final int? season;
  final int? episode;

  /// Last episode of a multi-episode file (S01E01E02 → 2).
  final int? episodeEnd;
  final String? episodeTitle;
  final String? releaseGroup;

  /// Name looked like an anime fansub release (`[Group] Title - 01`).
  final bool isAnime;
  final String? resolution;

  /// 0..1 how sure the parser is.
  final double confidence;

  const ParsedName({
    required this.title,
    this.altTitle,
    this.year,
    this.isEpisode = false,
    this.season,
    this.episode,
    this.episodeEnd,
    this.episodeTitle,
    this.releaseGroup,
    this.isAnime = false,
    this.resolution,
    this.confidence = 0.5,
  });

  ParsedName copyWith({
    String? title,
    String? altTitle,
    int? year,
    bool? isEpisode,
    int? season,
    int? episode,
    double? confidence,
  }) => ParsedName(
    title: title ?? this.title,
    altTitle: altTitle ?? this.altTitle,
    year: year ?? this.year,
    isEpisode: isEpisode ?? this.isEpisode,
    season: season ?? this.season,
    episode: episode ?? this.episode,
    episodeEnd: episodeEnd,
    episodeTitle: episodeTitle,
    releaseGroup: releaseGroup,
    isAnime: isAnime,
    resolution: resolution,
    confidence: confidence ?? this.confidence,
  );

  @override
  String toString() =>
      'ParsedName("$title"'
      '${altTitle != null ? ' / "$altTitle"' : ''}'
      '${year != null ? ' ($year)' : ''}'
      '${isEpisode ? ' S${season}E$episode${episodeEnd != null ? '-E$episodeEnd' : ''}' : ''}'
      '${episodeTitle != null ? ' "$episodeTitle"' : ''}'
      '${isAnime ? ' anime' : ''}'
      '${releaseGroup != null ? ' [$releaseGroup]' : ''}'
      ', conf: $confidence)';
}

// ---------------------------------------------------------------------------
// Vocabulary

const Set<String> _junkWords = {
  // resolution / quality
  '4k', 'uhd', 'fhd', 'hd', 'sd', 'hq', 'lq', 'hdr', 'hdr10', 'hdr10+',
  'hdr10plus', 'dv', 'dovi', 'sdr', 'hlg', 'imax',
  // sources
  'bluray', 'blu-ray', 'bdrip', 'brrip', 'bdremux', 'remux', 'bd', 'bd25',
  'bd50', 'web', 'web-dl', 'webdl', 'webrip', 'web-rip', 'hdtv', 'hdtvrip',
  'pdtv', 'dsr', 'dvb', 'satrip', 'tvrip', 'dvd', 'dvdrip', 'dvd5', 'dvd9',
  'dvdscr', 'hdrip', 'hddvd', 'camrip', 'cam', 'ts', 'telesync', 'tc',
  'screener', 'scr', 'r5', 'amzn', 'nf', 'dsnp', 'hmax', 'atvp', 'hulu',
  'itunes', 'ivi', 'okko', 'kinopoisk', 'kp', 'wakanim', 'cr', 'crunchyroll',
  'bdmv', 'vhsrip', 'ldrip', 'webdlrip', 'web-dlrip', 'iptvrip',
  // codecs
  'x264', 'x265', 'h264', 'h265', 'hevc', 'avc', 'xvid', 'divx', 'av1',
  'vp9', 'mpeg2', 'mpeg4', 'vc1', 'vc-1', '10bit', '10-bit', '8bit', '8-bit',
  'hi10', 'hi10p', '12bit',
  // audio
  'aac', 'aac2', 'ac3', 'eac3', 'e-ac3', 'dts', 'dts-hd', 'dtshd', 'dts-x',
  'dtsx', 'truehd', 'atmos', 'flac', 'mp3', 'opus', 'lpcm', 'pcm', 'dd',
  'ddp', 'dd+', 'dd5', 'ddp5', 'ma', 'hra', '2ch', '6ch', '8ch', 'stereo',
  'mono',
  // release flags / editions
  'proper', 'repack', 'rerip', 'real', 'internal', 'limited', 'extended',
  'unrated', 'uncut', 'uncensored', 'remastered', 'restored', 'directors',
  "director's", 'dc', 'theatrical', 'criterion', 'complete', 'dubbed',
  'subbed', 'multi', 'multisub', 'multi-sub', 'dual', 'dual-audio',
  'dualaudio', 'mvo', 'dvo', 'avo', 'vo', 'dub', 'sub', 'subs', 'hardsub',
  'softsub', 'rus', 'eng', 'ukr', 'jap', 'jpn', 'ger', 'fre', 'ita', 'spa',
  'kor', 'chi', 'russian', 'english', 'french', 'german', 'japanese', 'nordic',
  'truefrench', 'vostfr', 'vff', 'vfq', 'ita-eng', 'rus-eng', 'hc', 'readnfo',
  'nfofix', 'dirfix', 'syncfix', 'sample', '3d', 'hsbs', 'sbs', 'hou',
  'open-matte', 'openmatte', 'ws', 'fs', 'batch', 'raw', 'rip',
  // Russian release jargon
  'дубляж', 'дублированный', 'многоголосый', 'одноголосый', 'двухголосый',
  'лицензия', 'профессиональный', 'любительский', 'авторский', 'субтитры',
  'рип', 'полная', 'версия', 'режиссёрская', 'режиссерская',
};

/// Multi-word phrases removed before tokenising (lower case).
const List<String> _junkPhrases = [
  "director's cut",
  'directors cut',
  'extended cut',
  'extended edition',
  'special edition',
  'collectors edition',
  'final cut',
  'theatrical cut',
  'dolby vision',
  'dual audio',
  'режиссёрская версия',
  'режиссерская версия',
];

/// Generic folder names that never carry a title.
const Set<String> genericFolderNames = {
  'movies',
  'movie',
  'films',
  'film',
  'video',
  'videos',
  'downloads',
  'download',
  'tv',
  'tv shows',
  'tvshows',
  'shows',
  'series',
  'serials',
  'anime',
  'cartoons',
  'media',
  'library',
  'kids',
  'documentaries',
  'new',
  'torrents',
  'complete',
  'фильмы',
  'фильм',
  'кино',
  'видео',
  'сериалы',
  'сериал',
  'аниме',
  'мультфильмы',
  'мультики',
  'мультсериалы',
  'загрузки',
  'документальные',
  'новое',
  'video_ts',
  'bdmv',
  'stream',
  'mnt',
  'media1',
  'music',
  'музыка',
  'home',
  'public',
  'share',
  'nas',
  'data',
  'volume1',
};

final RegExp _resolutionRe = RegExp(r'^(\d{3,4})[pi]$', caseSensitive: false);
final RegExp _dimensionsRe = RegExp(r'^\d{3,4}x\d{3,4}$', caseSensitive: false);
final RegExp _audioChannelsRe = RegExp(
  r'^(?:dd\+?|ddp|aac|ac3|eac3|dts|truehd|atmos|flac|opus)?[257]\.[01]$',
  caseSensitive: false,
);
final RegExp _junkRegexes = RegExp(
  r'^(?:'
  r'[xh]\.?26[45]|'
  r'(?:dd|ddp|aac|ac3|eac3|dts|flac|opus)\d(?:\.\d)?|'
  r'\d{1,2}bits?|'
  r'(?:web|hd|bd|dvd|tv|sat)[-]?(?:dl|rip)?(?:rip)?|'
  r'\d+(?:\.\d+)?\s?(?:gb|mb|fps)|'
  r'\d{2,3}fps|'
  r'(?:cd|dvd|disc|disk|part|pt)\d{1,2}|'
  r'v\d'
  r')$',
  caseSensitive: false,
);
final RegExp _yearRe = RegExp(r'^(?:19\d\d|20\d\d)$');

/// SxxEyy with optional extra episodes: S01E01, S01E01E02, S01E01-E02,
/// S01E01-02, s1e1.
final RegExp _sxeRe = RegExp(
  r'^s(\d{1,2})[ ._]?e(\d{1,4})(?:(?:-?e|-)(\d{1,4}))?(?:e(\d{1,4}))*$',
  caseSensitive: false,
);
final RegExp _seasonOnlyRe = RegExp(r'^s(\d{1,2})$', caseSensitive: false);
final RegExp _epOnlyRe = RegExp(
  r'^(?:e|ep|ep\.)(\d{1,4})(?:v\d)?$',
  caseSensitive: false,
);
final RegExp _nxnRe = RegExp(r'^(\d{1,2})x(\d{2,3})(?:-(\d{2,3}))?$');
final RegExp _plainNumberRe = RegExp(r'^(\d{1,4})(?:v\d)?$');
final RegExp _seasonWordRe = RegExp(
  r'^(?:season|сезон|series|saison|staffel|temporada)$',
  caseSensitive: false,
);
final RegExp _episodeWordRe = RegExp(
  r'^(?:episode|ep|серия|серии|эпизод|episodio|folge|épisode)$',
  caseSensitive: false,
);
final RegExp _ordinalSeasonRe = RegExp(
  r'^(\d{1,2})(?:st|nd|rd|th)$',
  caseSensitive: false,
);

bool _isYear(String t) {
  if (!_yearRe.hasMatch(t)) return false;
  final y = int.parse(t);
  return y >= 1900 && y <= DateTime.now().year + 2;
}

bool isJunkToken(String token) {
  final t = token.toLowerCase();
  if (t.isEmpty) return false;
  if (_junkWords.contains(t)) return true;
  if (_resolutionRe.hasMatch(t)) return true;
  if (_dimensionsRe.hasMatch(t)) return true;
  if (_audioChannelsRe.hasMatch(t)) return true;
  if (_junkRegexes.hasMatch(t)) return true;
  // x264-GROUP, WEB-DL-GROUP style compound tokens.
  final dash = t.indexOf('-');
  if (dash > 0) {
    final head = t.substring(0, dash);
    if (_junkWords.contains(head) ||
        _resolutionRe.hasMatch(head) ||
        _junkRegexes.hasMatch(head)) {
      return true;
    }
  }
  return false;
}

// ---------------------------------------------------------------------------
// Tokenising

/// Special token that stops the title (from a dropped bracket group).
const String _stop = '\u0000';

class _Pre {
  final List<String> tokens;
  final String? group;
  final bool anime;
  final String? resolution;
  final String? trailingGroup;
  _Pre(
    this.tokens,
    this.group,
    this.anime,
    this.resolution,
    this.trailingGroup,
  );
}

_Pre _tokenise(String base) {
  var s = base.trim();
  String? group;
  var anime = false;
  String? resolution;

  // Leading [Group] → fansub style.
  final lead = RegExp(r'^\[([^\]]+)\]\s*').firstMatch(s);
  if (lead != null) {
    final g = lead.group(1)!.trim();
    if (_isYear(g)) {
      // "[2019] Title" – unusual, treat as year.
      s = '$g ${s.substring(lead.end)}';
    } else {
      group = g;
      anime = true;
      s = s.substring(lead.end);
    }
  }
  // Leading (Group) is sometimes used too, but more often (year): ignore.

  // Square / curly bracket groups: keep a year if present, drop the rest and
  // put a stop marker so the title ends here.
  s = s.replaceAllMapped(RegExp(r'[\[{]([^\]}]*)[\]}]'), (m) {
    final inner = m.group(1)!;
    final r = RegExp(r'(\d{3,4}[pi])', caseSensitive: false).firstMatch(inner);
    if (r != null) resolution ??= r.group(1)!.toLowerCase();
    final y = RegExp(r'(?<!\d)(19\d\d|20\d\d)(?!\d)').firstMatch(inner);
    if (y != null && _isYear(y.group(1)!)) return ' ${y.group(1)} $_stop ';
    return ' $_stop ';
  });

  // Parentheses: year → year token; junk → stop; other text → inline.
  s = s.replaceAllMapped(RegExp(r'\(([^)]*)\)'), (m) {
    final inner = m.group(1)!.trim();
    if (_isYear(inner)) return ' $inner $_stop ';
    final y = RegExp(r'^(19\d\d|20\d\d)[ ,.\-]').firstMatch(inner);
    if (y != null && _isYear(y.group(1)!)) return ' ${y.group(1)} $_stop ';
    final words = inner.split(RegExp(r'[\s._,]+')).where((w) => w.isNotEmpty);
    if (words.isNotEmpty && words.any(isJunkToken)) {
      final r = RegExp(
        r'(\d{3,4}[pi])',
        caseSensitive: false,
      ).firstMatch(inner);
      if (r != null) resolution ??= r.group(1)!.toLowerCase();
      return ' $_stop ';
    }
    return ' $inner ';
  });

  // Multi-word junk phrases.
  for (final ph in _junkPhrases) {
    final re = RegExp(
      RegExp.escape(ph).replaceAll(r'\ ', r'[\s._-]+'),
      caseSensitive: false,
    );
    s = s.replaceAll(re, ' remastered ');
  }

  // Normalise dotted technical tokens before dots become separators.
  s = s.replaceAllMapped(
    RegExp(r'\b([hx])\.(26[45])\b', caseSensitive: false),
    (m) => '${m.group(1)}${m.group(2)}',
  );
  s = s.replaceAllMapped(
    RegExp(
      r'\b(dd\+?|ddp|aac|ac3|eac3|dts|truehd|atmos|flac|opus)[ .]?([257])\.([01])\b',
      caseSensitive: false,
    ),
    (m) => ' ${m.group(1)}${m.group(2)}.${m.group(3)} ',
  );

  // Separators: '_' and '.' (except between digits, e.g. 5.1) → space.
  s = s.replaceAll('_', ' ');
  s = _replaceDots(s);
  s = s.replaceAll(';', ' ');
  // Collapse a lone '+' or ',' / ';' / '!' / '?' that are not part of words.
  s = s.replaceAll(RegExp(r'\s+[,;]\s*'), ' ');
  s = s.replaceAll(RegExp(r'[,;](?=\s|$)'), ' ');
  // " - " and "–" are separators we keep as a '-' token.
  s = s.replaceAll(RegExp(r'\s+[-–—]+\s+'), ' - ');
  s = s.replaceAll(RegExp(r'^[-–—]+\s+|\s+[-–—]+$'), ' ');

  var tokens = s
      .split(RegExp(r'\s+'))
      .where((t) => t.isNotEmpty)
      .toList(growable: true);

  // Trailing "-GROUP" glued to the last junk token: x264-SPARKS.
  String? trailing;
  if (tokens.isNotEmpty) {
    final last = tokens.last;
    final dash = last.lastIndexOf('-');
    if (dash > 0 && dash < last.length - 1) {
      final head = last.substring(0, dash);
      if (isJunkToken(head) || _isYear(head)) {
        trailing = last.substring(dash + 1);
        tokens[tokens.length - 1] = head;
      }
    }
  }
  for (final t in tokens) {
    final r = _resolutionRe.firstMatch(t);
    if (r != null) resolution ??= t.toLowerCase();
  }
  if (tokens.isNotEmpty && tokens.last.toLowerCase() == 'rarbg') {
    tokens.removeLast();
  }
  return _Pre(tokens, group, anime, resolution, trailing);
}

// ---------------------------------------------------------------------------
// Parsing

class _Ep {
  final int index; // first token index of the marker
  final int length; // number of tokens in the marker
  final int? season;
  final int episode;
  final int? episodeEnd;
  final double confidence;
  _Ep(
    this.index,
    this.length,
    this.season,
    this.episode,
    this.episodeEnd,
    this.confidence,
  );
}

int? _int(String? s) => s == null ? null : int.tryParse(s);

/// Finds an explicit episode marker in the token list.
_Ep? _findEpisode(List<String> t) {
  for (var i = 0; i < t.length; i++) {
    final tok = t[i];
    final low = tok.toLowerCase();

    var m = _sxeRe.firstMatch(tok);
    if (m != null) {
      final start = int.parse(m.group(2)!);
      var end = _int(m.group(3));
      final more = RegExp(r'e(\d{1,4})', caseSensitive: false)
          .allMatches(tok.substring(m.start))
          .map((x) => int.parse(x.group(1)!))
          .toList();
      if (more.length > 1) end = more.last;
      if (end != null && end <= start) end = null;
      return _Ep(i, 1, int.parse(m.group(1)!), start, end, 0.95);
    }

    // S01 E02 / S01 - E02
    m = _seasonOnlyRe.firstMatch(tok);
    if (m != null && i + 1 < t.length) {
      final m2 = _epOnlyRe.firstMatch(t[i + 1]);
      if (m2 != null) {
        return _Ep(
          i,
          2,
          int.parse(m.group(1)!),
          int.parse(m2.group(1)!),
          null,
          0.9,
        );
      }
    }

    m = _nxnRe.firstMatch(tok);
    if (m != null) {
      final se = int.parse(m.group(1)!);
      final ep = int.parse(m.group(2)!);
      if (se > 0 || ep > 0) {
        var end = _int(m.group(3));
        if (end != null && end <= ep) end = null;
        return _Ep(i, 1, se, ep, end, 0.9);
      }
    }

    // "Season 1 Episode 2" / "Сезон 1 Серия 2" / "Season 1 - Episode 2"
    if (_seasonWordRe.hasMatch(low) && i + 1 < t.length) {
      final sn = _plainNumberRe.firstMatch(t[i + 1]);
      if (sn != null) {
        var j = i + 2;
        if (j < t.length && t[j] == '-') j++;
        if (j + 1 < t.length && _episodeWordRe.hasMatch(t[j])) {
          final en = _plainNumberRe.firstMatch(t[j + 1]);
          if (en != null) {
            return _Ep(
              i,
              j + 2 - i,
              int.parse(sn.group(1)!),
              int.parse(en.group(1)!),
              null,
              0.9,
            );
          }
        }
      }
    }
    // "1 сезон 2 серия" / "1 season 2 episode"
    if (_plainNumberRe.hasMatch(tok) &&
        !_isYear(tok) &&
        i + 3 < t.length &&
        _seasonWordRe.hasMatch(t[i + 1]) &&
        _plainNumberRe.hasMatch(t[i + 2]) &&
        _episodeWordRe.hasMatch(t[i + 3])) {
      return _Ep(
        i,
        4,
        int.parse(_plainNumberRe.firstMatch(tok)!.group(1)!),
        int.parse(_plainNumberRe.firstMatch(t[i + 2])!.group(1)!),
        null,
        0.9,
      );
    }
    // "Episode 5" / "Серия 5" / "Ep 5"
    if (_episodeWordRe.hasMatch(low) && i + 1 < t.length) {
      final en = _plainNumberRe.firstMatch(t[i + 1]);
      if (en != null && !_isYear(t[i + 1])) {
        return _Ep(i, 2, null, int.parse(en.group(1)!), null, 0.75);
      }
    }
    // "5 серия"
    if (_plainNumberRe.hasMatch(tok) &&
        !_isYear(tok) &&
        i + 1 < t.length &&
        RegExp(
          r'^(?:серия|эпизод)$',
          caseSensitive: false,
        ).hasMatch(t[i + 1])) {
      return _Ep(
        i,
        2,
        null,
        int.parse(_plainNumberRe.firstMatch(tok)!.group(1)!),
        null,
        0.75,
      );
    }
    // E05 / Ep05
    m = _epOnlyRe.firstMatch(tok);
    if (m != null && i > 0) {
      return _Ep(i, 1, null, int.parse(m.group(1)!), null, 0.75);
    }
  }
  return null;
}

/// Anime-style "Title - 05" (or "Title 05" when a fansub group is present).
_Ep? _findAbsoluteEpisode(List<String> t, bool anime) {
  for (var i = 1; i < t.length - 1; i++) {
    if (t[i] != '-') continue;
    final m = _plainNumberRe.firstMatch(t[i + 1]);
    if (m == null) continue;
    final n = int.parse(m.group(1)!);
    if (_isYear(t[i + 1]) && !anime) continue;
    if (n == 0 && !anime) continue;
    return _Ep(i, 2, null, n, null, anime ? 0.85 : 0.7);
  }
  // "- 05-06" ranges
  for (var i = 1; i < t.length - 1; i++) {
    if (t[i] != '-') continue;
    final m = RegExp(r'^(\d{1,4})-(\d{1,4})$').firstMatch(t[i + 1]);
    if (m != null) {
      final a = int.parse(m.group(1)!), b = int.parse(m.group(2)!);
      if (b > a) return _Ep(i, 2, null, a, b, anime ? 0.85 : 0.7);
    }
  }
  if (anime) {
    // [Group] Title 05 [1080p]: the last plain number right before a stop.
    for (var i = 1; i < t.length; i++) {
      final m = _plainNumberRe.firstMatch(t[i]);
      if (m == null || _isYear(t[i])) continue;
      final next = i + 1 < t.length ? t[i + 1] : _stop;
      if (next == _stop || isJunkToken(next)) {
        return _Ep(i, 1, null, int.parse(m.group(1)!), null, 0.75);
      }
    }
  }
  return null;
}

/// Dots become spaces, except in "5.1"-style single-digit decimals.
String _replaceDots(String s) {
  bool digit(int i) =>
      i >= 0 && i < s.length && s.codeUnitAt(i) >= 48 && s.codeUnitAt(i) <= 57;
  final b = StringBuffer();
  for (var i = 0; i < s.length; i++) {
    if (s[i] == '.') {
      final keep =
          digit(i - 1) && !digit(i - 2) && digit(i + 1) && !digit(i + 2);
      b.write(keep ? '.' : ' ');
    } else {
      b.write(s[i]);
    }
  }
  return b.toString();
}

bool _isStop(String t) => t == _stop || isJunkToken(t);

String _join(Iterable<String> tokens) {
  var s = tokens.where((t) => t != _stop).join(' ');
  s = s.replaceAll(RegExp(r'\s+'), ' ').trim();
  // Strip separators at both ends.
  s = s.replaceAll(RegExp(r'^[\s\-–—:,.~+]+|[\s\-–—:,~+]+$'), '');
  return s.trim();
}

/// Season from the tail of the show title: "Title S2", "Title Season 2",
/// "Title 2nd Season", "Title 2 сезон". Returns (cleanTitleTokens, season).
(List<String>, int?) _seasonFromTitleTail(List<String> tokens) {
  if (tokens.isEmpty) return (tokens, null);
  final n = tokens.length;
  final last = tokens[n - 1];
  var m = _seasonOnlyRe.firstMatch(last);
  if (m != null && n > 1) {
    return (tokens.sublist(0, n - 1), int.parse(m.group(1)!));
  }
  if (n > 2 && _seasonWordRe.hasMatch(tokens[n - 2])) {
    final sm = _plainNumberRe.firstMatch(last);
    if (sm != null) return (tokens.sublist(0, n - 2), int.parse(sm.group(1)!));
  }
  if (n > 2 && _seasonWordRe.hasMatch(last)) {
    final prev = tokens[n - 2];
    final om =
        _ordinalSeasonRe.firstMatch(prev) ?? _plainNumberRe.firstMatch(prev);
    if (om != null && !_isYear(prev)) {
      return (tokens.sublist(0, n - 2), int.parse(om.group(1)!));
    }
  }
  return (tokens, null);
}

/// Parses a file (or folder) base name without folder context.
ParsedName parseName(String name) {
  final pre = _tokenise(name);
  var t = pre.tokens;
  final anime = pre.anime;

  var ep = _findEpisode(t);
  var absolute = false;
  if (ep == null && (anime || t.contains('-'))) {
    ep = _findAbsoluteEpisode(t, anime);
    absolute = ep != null;
  }

  // Title end: first episode marker, junk token or stop marker.
  var end = t.length;
  if (ep != null) end = ep.index;
  for (var i = 0; i < end; i++) {
    if (_isStop(t[i]) && i > 0) {
      end = i;
      break;
    }
    if (_isStop(t[i]) && i == 0 && t[i] == _stop) {
      // Leading stop (e.g. "[1080p] Title"): skip it.
      continue;
    }
  }

  // Year: the last year candidate inside the title region that is not the
  // first token wins; tokens from it onwards are dropped from the title.
  int? year;
  var yearIdx = -1;
  for (var i = 0; i < end; i++) {
    if (_isYear(t[i]) && i > 0) {
      yearIdx = i;
    }
  }
  if (yearIdx > 0) {
    // Only accept a year if what follows it (up to `end`) is not more title,
    // unless the year was in parentheses (followed by a stop marker).
    final followedByStop =
        yearIdx + 1 < t.length &&
        (t[yearIdx + 1] == _stop || _isStop(t[yearIdx + 1]));
    final atEnd = yearIdx == end - 1;
    if (followedByStop || atEnd || ep != null || yearIdx + 1 >= t.length) {
      year = int.parse(t[yearIdx]);
      end = yearIdx;
    } else {
      // "Title 1999 Something 720p": still treat the year as a separator if
      // the rest before the junk is short (likely an edition/episode name).
      year = int.parse(t[yearIdx]);
      end = yearIdx;
    }
  } else if (end < t.length || ep != null) {
    // A year right after the title region (e.g. "Title S01E01 2019")
    // is not part of the title; look for a year after the episode marker.
    for (var i = end; i < t.length; i++) {
      if (_isYear(t[i]) && i > 0) {
        year = int.parse(t[i]);
        break;
      }
      if (t[i] == _stop && ep == null) break;
    }
  }
  if (year == null && t.length == 1 && _isYear(t[0])) {
    // "1917.mkv": the title is the number.
  }

  var titleTokens = t.sublist(0, end).where((x) => x != _stop).toList();

  // "Русское название / Original Title"
  String? alt;
  final slash = titleTokens.indexOf('/');
  if (slash > 0) {
    alt = _join(titleTokens.sublist(slash + 1));
    titleTokens = titleTokens.sublist(0, slash);
    if (alt.isEmpty) alt = null;
  }

  int? season = ep?.season;
  if (ep != null || anime) {
    final (clean, s) = _seasonFromTitleTail(titleTokens);
    if (s != null) {
      titleTokens = clean;
      season ??= s;
    }
  }
  // Trailing ' -' / episode name separators.
  while (titleTokens.isNotEmpty && titleTokens.last == '-') {
    titleTokens.removeLast();
  }

  var title = _join(titleTokens);
  if (title.isEmpty && end < t.length && ep == null) {
    // Name starts with junk: take the first non-junk run.
    final rest = t.where((x) => !_isStop(x) && !_isYear(x)).toList();
    title = _join(rest);
  }

  String? episodeTitle;
  if (ep != null) {
    final after = <String>[];
    for (var i = ep.index + ep.length; i < t.length; i++) {
      if (_isStop(t[i]) || (_isYear(t[i]) && after.isEmpty)) break;
      after.add(t[i]);
    }
    while (after.isNotEmpty && after.first == '-') {
      after.removeAt(0);
    }
    final et = _join(after);
    if (et.isNotEmpty && !_plainNumberRe.hasMatch(et)) episodeTitle = et;
  }

  var releaseGroup = pre.group ?? pre.trailingGroup;
  if (releaseGroup == null && t.isNotEmpty && end < t.length) {
    // "... x264-GRP" handled in tokenise; "... 1080p GRP" not reliable.
  }

  double conf;
  if (ep != null) {
    conf = ep.confidence;
  } else if (year != null) {
    conf = 0.8;
  } else if (end < t.length) {
    conf = 0.6; // title ended on junk: probably a release name
  } else {
    conf = 0.45;
  }
  if (title.isEmpty) conf = 0.1;

  return ParsedName(
    title: title,
    altTitle: alt,
    year: year,
    isEpisode: ep != null,
    season: ep != null ? (season ?? (anime || absolute ? 1 : null)) : null,
    episode: ep?.episode,
    episodeEnd: ep?.episodeEnd,
    episodeTitle: episodeTitle,
    releaseGroup: releaseGroup,
    isAnime: anime,
    resolution: pre.resolution,
    confidence: conf,
  );
}

// ---------------------------------------------------------------------------
// Folder context

final RegExp _seasonFolderRe = RegExp(
  r'^(?:season|сезон|series|saison|staffel|temporada|s)[\s._-]*(\d{1,2})(?:\b.*)?$',
  caseSensitive: false,
);
final RegExp _seasonFolderRe2 = RegExp(
  r'^(\d{1,2})[\s._-]*(?:season|сезон)$',
  caseSensitive: false,
);
final RegExp _specialsRe = RegExp(
  r'^(?:specials?|extras? season|спецвыпуски|сп)$',
  caseSensitive: false,
);

/// Season number if [folder] is a season folder ("Season 1", "S01",
/// "Сезон 2", "2 сезон", "Specials" → 0).
int? seasonFromFolder(String folder) {
  final f = folder.trim();
  if (_specialsRe.hasMatch(f)) return 0;
  final m = _seasonFolderRe.firstMatch(f) ?? _seasonFolderRe2.firstMatch(f);
  if (m == null) return null;
  return int.parse(m.group(1)!);
}

bool _isGeneric(String folder) =>
    genericFolderNames.contains(folder.trim().toLowerCase());

const _videoExts = {
  '.mkv',
  '.mp4',
  '.m4v',
  '.avi',
  '.mov',
  '.wmv',
  '.mpg',
  '.mpeg',
  '.ts',
  '.m2ts',
  '.mts',
  '.webm',
  '.flv',
  '.ogv',
  '.3gp',
  '.rmvb',
  '.divx',
  '.vob',
  '.iso',
  '.mp3',
  '.flac',
  '.m4a',
};

String _stripExt(String name) {
  final ext = p.extension(name).toLowerCase();
  if (_videoExts.contains(ext) || ext.length == 4 || ext.length == 5) {
    if (RegExp(r'^\.[a-z0-9]{2,4}$').hasMatch(ext)) {
      return name.substring(0, name.length - ext.length);
    }
  }
  return name;
}

/// Parses a video file path using its parent folders as context.
///
/// [root] is the source root: folders at or above it are not used as titles.
ParsedName parsePath(String path, {String? root}) {
  final base = _stripExt(p.basename(path));
  var parsed = parseName(base);

  // Relevant parent folders (closest first), stopping at the root.
  final folders = <String>[];
  var dir = p.dirname(path);
  final normRoot = root == null ? null : p.normalize(root);
  while (true) {
    if (normRoot != null &&
        (p.equals(dir, normRoot) || !p.isWithin(normRoot, dir))) {
      break;
    }
    final name = p.basename(dir);
    if (name.isEmpty || dir == p.dirname(dir)) break;
    folders.add(name);
    if (folders.length >= 3) break;
    dir = p.dirname(dir);
  }

  final parent = folders.isNotEmpty ? folders[0] : null;
  final grand = folders.length > 1 ? folders[1] : null;
  final parentSeason = parent != null ? seasonFromFolder(parent) : null;

  // The show folder: grandparent when the parent is a season folder.
  String? showFolder;
  if (parentSeason != null) {
    showFolder = grand;
  } else if (parent != null) {
    showFolder = parent;
  }
  if (showFolder != null && _isGeneric(showFolder)) showFolder = null;
  final showFolderParsed = showFolder != null ? parseName(showFolder) : null;

  // Bare numbers / "Episode 3" files inside a season or show folder.
  if (!parsed.isEpisode) {
    final bare = RegExp(
      r'^(?:e|ep|episode|серия)?[\s._-]*(\d{1,3})(?:[\s._-]+(.*))?$',
      caseSensitive: false,
    ).firstMatch(base.trim());
    if (bare != null &&
        (parentSeason != null || (showFolderParsed?.isEpisode ?? false))) {
      parsed = ParsedName(
        title: '',
        isEpisode: true,
        season: parentSeason ?? showFolderParsed?.season,
        episode: int.parse(bare.group(1)!),
        episodeTitle: bare.group(2) != null
            ? parseName(bare.group(2)!).title
            : null,
        confidence: 0.7,
      );
    }
  }

  if (parsed.isEpisode) {
    var title = parsed.title;
    var alt = parsed.altTitle;
    var year = parsed.year;
    var conf = parsed.confidence;
    final folderTitle = showFolderParsed?.title ?? '';
    if (parentSeason != null && folderTitle.isNotEmpty) {
      // Show/Season N/file: the folder is the most consistent show title.
      title = folderTitle;
      alt = showFolderParsed!.altTitle ?? alt;
      year = showFolderParsed.year ?? year;
      conf = (conf + 0.05).clamp(0, 1).toDouble();
    } else if (title.isEmpty && folderTitle.isNotEmpty) {
      title = folderTitle;
      alt = showFolderParsed!.altTitle;
      year = showFolderParsed.year;
    } else if (title.isEmpty && grand != null && !_isGeneric(grand)) {
      title = parseName(grand).title;
      conf = 0.5;
    }
    var season = parsed.season ?? parentSeason ?? showFolderParsed?.season ?? 1;
    return ParsedName(
      title: title,
      altTitle: alt,
      year: year,
      isEpisode: true,
      season: season,
      episode: parsed.episode,
      episodeEnd: parsed.episodeEnd,
      episodeTitle: parsed.episodeTitle,
      releaseGroup: parsed.releaseGroup,
      isAnime: parsed.isAnime,
      resolution: parsed.resolution,
      confidence: title.isEmpty ? 0.2 : conf,
    );
  }

  // Movie: a "Title (Year)" parent folder beats a messy / yearless file name.
  if (parent != null && !_isGeneric(parent) && parentSeason == null) {
    final fp = parseName(parent);
    if (fp.title.isNotEmpty && !fp.isEpisode) {
      final fileWeak = parsed.year == null || parsed.title.isEmpty;
      if (fp.year != null && fileWeak) {
        return parsed.copyWith(
          title: fp.title,
          altTitle: fp.altTitle,
          year: fp.year,
          confidence: 0.85,
        );
      }
    }
  }
  return parsed;
}

/// Normalised key used to group episodes into shows and compare titles.
String normalizeTitle(String title) {
  var s = title.toLowerCase();
  s = s.replaceAll('ё', 'е').replaceAll('&', ' and ');
  s = s.replaceAll(RegExp(r"['’`]"), '');
  s = s.replaceAll(RegExp(r'[^\p{L}\p{N}]+', unicode: true), ' ');
  s = s.trim().replaceAll(RegExp(r'\s+'), ' ');
  if (s.startsWith('the ')) s = s.substring(4);
  return s;
}
