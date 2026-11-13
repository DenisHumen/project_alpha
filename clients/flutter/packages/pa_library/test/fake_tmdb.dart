// SPDX-License-Identifier: MPL-2.0
//
// Offline TMDB stand-in serving the JSON fixtures in test/fixtures/tmdb.

import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

class FakeTmdb {
  final List<Uri> requests = [];

  /// Status code to return for every request (e.g. 401), if set.
  int? forceStatus;

  /// Number of 429 responses to send before serving normally.
  int rateLimitResponses = 0;

  late final http.Client client = MockClient((req) async {
    requests.add(req.url);
    if (forceStatus != null) {
      return http.Response(
        '{"status_message":"Invalid API key","success":false}',
        forceStatus!,
      );
    }
    if (rateLimitResponses > 0) {
      rateLimitResponses--;
      return http.Response('{}', 429, headers: {'retry-after': '0'});
    }
    final fixture = _route(req.url);
    if (fixture == null) {
      return http.Response('{"status_message":"not found"}', 404);
    }
    final body = File('test/fixtures/tmdb/$fixture.json').readAsBytesSync();
    return http.Response.bytes(
      body,
      200,
      headers: {'content-type': 'application/json; charset=utf-8'},
    );
  });

  String? _route(Uri u) {
    final path = u.path.replaceFirst(RegExp(r'^/3/'), '');
    final q = (u.queryParameters['query'] ?? '').toLowerCase();
    switch (path) {
      case 'search/movie':
        if (q == 'the matrix') return 'search_movie_the_matrix';
        return 'search_empty';
      case 'search/tv':
        if (q == 'breaking bad') return 'search_tv_breaking_bad';
        return 'search_empty';
      case 'movie/27205':
        return 'movie_27205';
      case 'tv/1396/season/1/episode/1':
        return 'tv_1396_s1_e1';
      case 'tv/1396/season/1/episode/2':
        return 'tv_1396_s1_e2';
    }
    return null;
  }
}
