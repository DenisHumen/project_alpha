// SPDX-License-Identifier: MPL-2.0
//
// Minimal TMDB v3 client. Terms (docs/legal/providers.md): attribution
// "This product uses the TMDB API but is not endorsed or certified by TMDB"
// is shown by the app; responses are cached for at most [maxCacheAge]
// (well under TMDB's 6-month limit); HTTP 429 is honoured.

import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'parser/filename_parser.dart' show normalizeTitle;

/// Attribution text required by the TMDB terms of use.
const String tmdbAttribution =
    'This product uses the TMDB API but is not endorsed or certified by TMDB.';

const String tmdbImageBase = 'https://image.tmdb.org/t/p/';

/// Builds an image URL, e.g. `tmdbImageUrl('/abc.jpg', 'w500')`.
String? tmdbImageUrl(String? path, [String size = 'w500']) =>
    (path == null || path.isEmpty) ? null : '$tmdbImageBase$size$path';

/// Response cache (implemented by the library on top of SQLite).
abstract class TmdbCache {
  String? get(String key, Duration maxAge);
  void put(String key, String body);
}

class TmdbException implements Exception {
  final int? statusCode;
  final String message;
  TmdbException(this.message, [this.statusCode]);
  @override
  String toString() => 'TmdbException($statusCode): $message';
}

class TmdbSearchResult {
  final int id;
  final bool isTv;
  final String title;
  final String? originalTitle;
  final int? year;
  final String? overview;
  final String? posterPath;
  final String? backdropPath;
  final double popularity;
  const TmdbSearchResult({
    required this.id,
    required this.isTv,
    required this.title,
    this.originalTitle,
    this.year,
    this.overview,
    this.posterPath,
    this.backdropPath,
    this.popularity = 0,
  });

  factory TmdbSearchResult.fromJson(Map<String, dynamic> j, {required bool tv}) {
    final date = (tv ? j['first_air_date'] : j['release_date']) as String?;
    return TmdbSearchResult(
      id: (j['id'] as num).toInt(),
      isTv: tv,
      title: ((tv ? j['name'] : j['title']) as String?) ?? '',
      originalTitle: (tv ? j['original_name'] : j['original_title']) as String?,
      year: _yearOf(date),
      overview: _nonEmpty(j['overview'] as String?),
      posterPath: j['poster_path'] as String?,
      backdropPath: j['backdrop_path'] as String?,
      popularity: (j['popularity'] as num?)?.toDouble() ?? 0,
    );
  }

  String? get posterUrl => tmdbImageUrl(posterPath, 'w500');
  String? get backdropUrl => tmdbImageUrl(backdropPath, 'w1280');

  @override
  String toString() => 'TmdbSearchResult($id, "$title" ($year), tv: $isTv)';
}

class TmdbEpisode {
  final int season;
  final int episode;
  final String? name;
  final String? overview;
  final String? stillPath;
  final int? runtimeMin;
  const TmdbEpisode(this.season, this.episode,
      {this.name, this.overview, this.stillPath, this.runtimeMin});

  factory TmdbEpisode.fromJson(Map<String, dynamic> j) => TmdbEpisode(
        (j['season_number'] as num).toInt(),
        (j['episode_number'] as num).toInt(),
        name: _nonEmpty(j['name'] as String?),
        overview: _nonEmpty(j['overview'] as String?),
        stillPath: j['still_path'] as String?,
        runtimeMin: (j['runtime'] as num?)?.toInt(),
      );

  String? get stillUrl => tmdbImageUrl(stillPath, 'w780');
}

int? _yearOf(String? date) =>
    (date != null && date.length >= 4) ? int.tryParse(date.substring(0, 4)) : null;
String? _nonEmpty(String? s) => (s == null || s.trim().isEmpty) ? null : s;

/// A scored match.
class TmdbMatch {
  final TmdbSearchResult result;
  final double confidence;
  const TmdbMatch(this.result, this.confidence);
}

class TmdbClient {
  final http.Client _http;
  final String apiKey;
  final String? language;
  final TmdbCache? cache;
  final Uri baseUri;
  static const Duration maxCacheAge = Duration(days: 30);

  TmdbClient(
    this._http, {
    required this.apiKey,
    this.language,
    this.cache,
    Uri? baseUri,
  }) : baseUri = baseUri ?? Uri.parse('https://api.themoviedb.org/3/');

  /// v4 "API Read Access Token" (a JWT) vs v3 api_key.
  bool get _isBearer => apiKey.length > 40 && apiKey.contains('.');

  Future<Map<String, dynamic>> _get(String path, Map<String, String> query) async {
    final q = <String, String>{
      'language': ?language,
      ...query,
    };
    final cacheKey = Uri(path: path, queryParameters: q.isEmpty ? null : q).toString();
    final cached = cache?.get(cacheKey, maxCacheAge);
    if (cached != null) return jsonDecode(cached) as Map<String, dynamic>;

    final uri = baseUri.resolve(path).replace(queryParameters: {
      ...q,
      if (!_isBearer) 'api_key': apiKey,
    });
    final headers = {
      'Accept': 'application/json',
      if (_isBearer) 'Authorization': 'Bearer $apiKey',
    };
    for (var attempt = 0;; attempt++) {
      final res = await _http.get(uri, headers: headers).timeout(const Duration(seconds: 20));
      if (res.statusCode == 429 && attempt < 3) {
        final wait = int.tryParse(res.headers['retry-after'] ?? '') ?? (attempt + 1);
        await Future<void>.delayed(Duration(milliseconds: (wait.clamp(0, 10)) * 1000));
        continue;
      }
      if (res.statusCode != 200) {
        String msg = 'HTTP ${res.statusCode}';
        try {
          msg = (jsonDecode(res.body) as Map<String, dynamic>)['status_message'] as String? ?? msg;
        } catch (_) {}
        throw TmdbException(msg, res.statusCode);
      }
      final body = utf8.decode(res.bodyBytes);
      final json = jsonDecode(body) as Map<String, dynamic>;
      cache?.put(cacheKey, body);
      return json;
    }
  }

  Future<List<TmdbSearchResult>> searchMovie(String query, {int? year}) async {
    final j = await _get('search/movie', {
      'query': query,
      'include_adult': 'false',
      if (year != null) 'year': '$year',
    });
    return [
      for (final r in (j['results'] as List? ?? const []))
        TmdbSearchResult.fromJson(r as Map<String, dynamic>, tv: false)
    ];
  }

  Future<List<TmdbSearchResult>> searchTv(String query, {int? year}) async {
    final j = await _get('search/tv', {
      'query': query,
      'include_adult': 'false',
      if (year != null) 'first_air_date_year': '$year',
    });
    return [
      for (final r in (j['results'] as List? ?? const []))
        TmdbSearchResult.fromJson(r as Map<String, dynamic>, tv: true)
    ];
  }

  Future<TmdbSearchResult> movie(int id) async =>
      TmdbSearchResult.fromJson(await _get('movie/$id', const {}), tv: false);

  Future<TmdbSearchResult> tv(int id) async =>
      TmdbSearchResult.fromJson(await _get('tv/$id', const {}), tv: true);

  Future<TmdbEpisode?> episode(int tvId, int season, int episode) async {
    try {
      return TmdbEpisode.fromJson(
          await _get('tv/$tvId/season/$season/episode/$episode', const {}));
    } on TmdbException catch (e) {
      if (e.statusCode == 404) return null;
      rethrow;
    }
  }

  /// Searches and scores candidates; tries the year-filtered search first,
  /// then without the year, then the alternative title.
  Future<TmdbMatch?> findBest(String title,
      {int? year, String? altTitle, required bool tv}) async {
    Future<List<TmdbSearchResult>> search(String q, int? y) =>
        tv ? searchTv(q, year: y) : searchMovie(q, year: y);

    final queries = <(String, int?)>[
      if (year != null) (title, year),
      (title, null),
      if (altTitle != null && altTitle.isNotEmpty) ...[
        if (year != null) (altTitle, year),
        (altTitle, null),
      ],
    ];
    TmdbMatch? best;
    for (final (q, y) in queries) {
      if (q.trim().isEmpty) continue;
      final results = await search(q, y);
      for (final (i, r) in results.take(10).indexed) {
        final c = scoreMatch(r, [title, ?altTitle], year, rank: i);
        if (best == null || c > best.confidence) best = TmdbMatch(r, c);
      }
      if (best != null && best.confidence >= 0.85) break;
    }
    if (best == null || best.confidence < 0.45) return null;
    return best;
  }
}

/// Scores a search result against the parsed title(s) and year (0..1).
double scoreMatch(TmdbSearchResult r, List<String> titles, int? year, {int rank = 0}) {
  var sim = 0.0;
  for (final t in titles) {
    final a = normalizeTitle(t);
    for (final b in [r.title, ?r.originalTitle]) {
      sim = _max(sim, titleSimilarity(a, normalizeTitle(b)));
    }
  }
  double yearScore;
  if (year == null || r.year == null) {
    yearScore = 0.5;
  } else {
    final d = (year - r.year!).abs();
    yearScore = d == 0 ? 1.0 : (d == 1 ? 0.7 : 0.0);
  }
  final rankScore = rank == 0 ? 1.0 : (rank < 3 ? 0.6 : 0.3);
  return (sim * 0.7 + yearScore * 0.2 + rankScore * 0.1).clamp(0.0, 1.0);
}

double _max(double a, double b) => a > b ? a : b;

/// Token-set similarity of two normalised titles (0..1).
double titleSimilarity(String a, String b) {
  if (a == b) return 1;
  if (a.isEmpty || b.isEmpty) return 0;
  final ta = a.split(' ').toSet(), tb = b.split(' ').toSet();
  final inter = ta.intersection(tb).length;
  final union = ta.union(tb).length;
  final jaccard = inter / union;
  // Prefix bonus: "dune" vs "dune part two" should not be a strong match,
  // but "office us" vs "office" should be decent.
  final contains = (a.contains(b) || b.contains(a)) ? 0.15 : 0.0;
  return (jaccard + contains).clamp(0.0, 1.0);
}
