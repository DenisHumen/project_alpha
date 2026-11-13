// SPDX-License-Identifier: MPL-2.0

import 'package:pa_library/src/tmdb.dart';
import 'package:test/test.dart';

import 'fake_tmdb.dart';

class _MapCache implements TmdbCache {
  final Map<String, String> m = {};
  @override
  String? get(String key, Duration maxAge) => m[key];
  @override
  void put(String key, String body) => m[key] = body;
}

void main() {
  late FakeTmdb fake;
  setUp(() => fake = FakeTmdb());

  TmdbClient client({String key = 'v3key', TmdbCache? cache, String? lang}) =>
      TmdbClient(fake.client, apiKey: key, cache: cache, language: lang);

  test('searchMovie parses results and builds image URLs', () async {
    final r = await client().searchMovie('The Matrix', year: 1999);
    expect(r, hasLength(2));
    expect(r.first.id, 603);
    expect(r.first.title, 'The Matrix');
    expect(r.first.year, 1999);
    expect(r.first.isTv, isFalse);
    expect(r.first.posterUrl,
        'https://image.tmdb.org/t/p/w500/fixture-matrix-poster.jpg');
    expect(r.first.backdropUrl,
        'https://image.tmdb.org/t/p/w1280/fixture-matrix-backdrop.jpg');
    expect(r[1].posterUrl, isNull);
    final u = fake.requests.single;
    expect(u.host, 'api.themoviedb.org');
    expect(u.path, '/3/search/movie');
    expect(u.queryParameters['api_key'], 'v3key');
    expect(u.queryParameters['year'], '1999');
  });

  test('v4 token is sent as bearer, not in the query', () async {
    final token = 'eyJhbGciOiJIUzI1NiJ9.${'a' * 60}.signature';
    await client(key: token).searchMovie('The Matrix');
    expect(fake.requests.single.queryParameters.containsKey('api_key'), isFalse);
  });

  test('language parameter', () async {
    await client(lang: 'ru-RU').searchTv('Breaking Bad');
    expect(fake.requests.single.queryParameters['language'], 'ru-RU');
  });

  test('searchTv and episode', () async {
    final c = client();
    final tv = await c.searchTv('Breaking Bad');
    expect(tv.single.id, 1396);
    expect(tv.single.isTv, isTrue);
    expect(tv.single.year, 2008);
    final ep = await c.episode(1396, 1, 1);
    expect(ep!.name, 'Pilot');
    expect(ep.runtimeMin, 58);
    expect(ep.stillUrl, 'https://image.tmdb.org/t/p/w780/fixture-bb-s1e1.jpg');
    expect(await c.episode(1396, 9, 9), isNull, reason: '404 → null');
  });

  test('movie by id', () async {
    final m = await client().movie(27205);
    expect(m.title, 'Inception');
    expect(m.year, 2010);
  });

  test('401 throws TmdbException', () async {
    fake.forceStatus = 401;
    expect(
        () => client().searchMovie('x'),
        throwsA(isA<TmdbException>()
            .having((e) => e.statusCode, 'status', 401)
            .having((e) => e.message, 'message', 'Invalid API key')));
  });

  test('429 is retried', () async {
    fake.rateLimitResponses = 2;
    final r = await client().searchMovie('The Matrix');
    expect(r, isNotEmpty);
    expect(fake.requests, hasLength(3));
  });

  test('responses are cached without the api key in the cache key', () async {
    final cache = _MapCache();
    final c = client(cache: cache);
    await c.searchMovie('The Matrix');
    await c.searchMovie('The Matrix');
    expect(fake.requests, hasLength(1));
    expect(cache.m.keys.single, isNot(contains('v3key')));
  });

  group('findBest', () {
    test('picks the exact title + year', () async {
      final m = await client().findBest('The Matrix', year: 1999, tv: false);
      expect(m!.result.id, 603);
      expect(m.confidence, greaterThan(0.9));
    });
    test('falls back to the alternative title', () async {
      final m = await client().findBest('Матрица',
          altTitle: 'The Matrix', year: 1999, tv: false);
      expect(m!.result.id, 603);
    });
    test('returns null when nothing matches', () async {
      expect(await client().findBest('Nothing Here', tv: false), isNull);
    });
    test('tv', () async {
      final m = await client().findBest('Breaking Bad', tv: true);
      expect(m!.result.id, 1396);
    });
  });

  group('scoring', () {
    TmdbSearchResult r(String t, int? y) =>
        TmdbSearchResult(id: 1, isTv: false, title: t, year: y);
    test('year mismatch lowers the score', () {
      expect(scoreMatch(r('Heat', 1995), ['Heat'], 1995),
          greaterThan(scoreMatch(r('Heat', 1986), ['Heat'], 1995)));
    });
    test('off-by-one year is tolerated', () {
      expect(scoreMatch(r('Heat', 1996), ['Heat'], 1995), greaterThan(0.8));
    });
    test('different title scores low', () {
      expect(scoreMatch(r('Dune Part Two', 2024), ['Dune'], 2021), lessThan(0.45));
    });
    test('titleSimilarity', () {
      expect(titleSimilarity('matrix', 'matrix'), 1);
      expect(titleSimilarity('office us', 'office'), greaterThan(0.5));
      expect(titleSimilarity('alien', 'predator'), 0);
    });
  });

  test('attribution text', () {
    expect(tmdbAttribution, contains('not endorsed or certified by TMDB'));
  });
}
