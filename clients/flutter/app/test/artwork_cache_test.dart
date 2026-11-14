// SPDX-License-Identifier: MPL-2.0
//
// LIB-013 in the app: remote artwork goes through the core's disk cache in
// the size on screen and falls back to the placeholder when it cannot be
// had (offline and not cached).

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/widgets/artwork.dart';
import 'package:kinolune/widgets/artwork_image.dart';
import 'package:material_ui/material_ui.dart';

/// 1×1 PNG.
final _png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
);

class _FakeCache implements ArtworkCache {
  _FakeCache(this.path);

  /// What every URL resolves to; `null` = offline and not cached.
  final String? path;
  final requests = <(String, int?)>[];

  @override
  String? cached(String url, {int? width}) => null;

  @override
  Future<String?> resolve(String url, {int? width}) async {
    requests.add((url, width));
    return path;
  }
}

const _url = 'https://image.tmdb.org/t/p/w500/poster.jpg';

Future<ImageInfo> _load(ImageProvider provider) {
  final done = Completer<ImageInfo>();
  provider
      .resolve(ImageConfiguration.empty)
      .addListener(
        ImageStreamListener(
          (info, _) => done.complete(info),
          onError: (e, _) => done.completeError(e),
        ),
      );
  return done.future;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory tmp;
  late String file;

  setUp(() {
    tmp = Directory.systemTemp.createTempSync('kinolune_art_');
    file = '${tmp.path}/poster.png';
    File(file).writeAsBytesSync(_png);
  });
  tearDown(() {
    appArtworkCache = null;
    tmp.deleteSync(recursive: true);
  });

  test('providers: cache for remote art, files for local art', () {
    final cache = _FakeCache(file);
    final cached = artworkProvider(_url, width: 256, cache: cache);
    expect(cached, isA<ResizeImage>());
    expect(
      (cached as ResizeImage).imageProvider,
      CachedArtworkImage(cache, _url, width: 256),
    );
    expect(artworkProvider(_url), isA<NetworkImage>());
    expect(artworkProvider('/m/poster.jpg'), isA<FileImage>());
    expect(
      (artworkProvider('file:///m/a%20b.jpg') as FileImage).file.path,
      '/m/a b.jpg',
    );
  });

  test('local art of network sources bypasses the disk cache', () {
    // LIB-019: the core's stream proxy, with a per-process token.
    const proxied = 'http://127.0.0.1:40123/token/3/Movies/poster.jpg';
    final cache = _FakeCache(file);
    expect(artworkProvider(proxied, cache: cache), isA<NetworkImage>());
    expect(
      (artworkProvider(proxied, width: 256, cache: cache) as ResizeImage)
          .imageProvider,
      isA<NetworkImage>(),
    );
    expect(isLoopbackUrl(proxied), isTrue);
    expect(isLoopbackUrl('http://localhost:1/x'), isTrue);
    expect(isLoopbackUrl(_url), isFalse);
    expect(isLoopbackUrl('https://127.0.0.1/x'), isFalse);
  });

  testWidgets('decode width follows the layout and pixel ratio', (
    tester,
  ) async {
    int? width;
    await tester.pumpWidget(
      MediaQuery(
        data: const MediaQueryData(devicePixelRatio: 2),
        child: Builder(
          builder: (context) {
            width = decodeWidth(context, 150);
            return const SizedBox();
          },
        ),
      ),
    );
    // 300 px rounded up to a multiple of 64.
    expect(width, 320);
  });

  test('a cached file is decoded; offline without a copy fails', () async {
    final cache = _FakeCache(file);
    final info = await _load(CachedArtworkImage(cache, _url, width: 128));
    expect(info.image.width, 1);
    expect(cache.requests, [(_url, 128)]);

    final offline = _FakeCache(null);
    await expectLater(
      _load(CachedArtworkImage(offline, '$_url?offline', width: 128)),
      throwsA(isA<StateError>()),
    );
  });

  testWidgets('Artwork asks the cache for the size on screen', (tester) async {
    final cache = _FakeCache(null);
    appArtworkCache = cache;
    await tester.pumpWidget(
      const Directionality(
        textDirection: TextDirection.ltr,
        child: Center(
          child: SizedBox(
            width: 100,
            height: 150,
            child: Artwork(url: _url, seed: 'x', style: ArtStyle.poster),
          ),
        ),
      ),
    );
    await tester.runAsync(() => Future<void>.delayed(Duration.zero));
    await tester.pump();
    expect(cache.requests, hasLength(1));
    final (url, width) = cache.requests.single;
    expect(url, _url);
    expect(width, isNotNull);
    expect(width! % 64, 0);
    // Not available: the brand placeholder.
    expect(find.byType(BrandPlaceholder), findsOneWidget);
  });
}
