// SPDX-License-Identifier: MPL-2.0
// RustArtworkCache (LIB-013) on the real native core. No network: remote
// URLs are only looked up, local art passes through.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

void main() {
  late Directory tmp;

  setUp(() => tmp = Directory.systemTemp.createTempSync('pa_art_cache_'));
  tearDown(() => tmp.deleteSync(recursive: true));

  test('local art passes through, remote art is only looked up', () async {
    final cache = await RustArtworkCache.open(
      p.join(tmp.path, 'artwork'),
      maxBytes: 1 << 20,
      externalLibrary: openBuiltCoreLibrary(),
    );
    final poster = p.join(tmp.path, 'Movie (2020)', 'poster.jpg');
    expect(cache.cached(poster, width: 200), poster);
    expect(await cache.resolve(poster), poster);
    expect(
      cache.cached('https://image.tmdb.org/t/p/w500/x.jpg', width: 200),
      isNull,
    );
    // Unsupported scheme: no exception, just no image.
    expect(await cache.resolve('ftp://example.org/x.jpg'), isNull);
    expect(cache.usage, (bytes: 0, files: 0, maxBytes: 1 << 20));
    await cache.setMaxBytes(1000);
    expect(cache.usage.maxBytes, 1000);
    await cache.clear();
    expect(Directory(p.join(tmp.path, 'artwork')).existsSync(), isTrue);
  });
}
