// SPDX-License-Identifier: MPL-2.0
// parseFilename() over FFI: the Rust parser of META-005 answers from Dart.
import 'package:pa_core/pa_core.dart';
import 'package:test/test.dart';

import 'native_library.dart';

void main() {
  setUpAll(() async {
    await RustLib.init(externalLibrary: openBuiltCoreLibrary());
  });

  tearDownAll(RustLib.dispose);

  test('anime release: title, group, episode, CRC', () async {
    final result = await parseFilename(
      path:
          'Anime/One Piece/[SubsPlease] One Piece - 1085 (1080p) [ABCD1234].mkv',
    );
    expect(result.parserVersion, filenameParserVersion());
    expect(result.hypotheses, isNotEmpty);
    final best = result.hypotheses.first.parsed;
    expect(best.title, 'One Piece');
    expect(best.releaseGroup, 'SubsPlease');
    expect(best.crc32, 'ABCD1234');
    expect(best.resolution, '1080p');
    expect(best.container, 'mkv');
    expect(best.animeHint, isTrue);
  });

  test('series episode with season folder', () async {
    final result = await parseFilename(
      path: 'Shows/Dark/Season 1/Dark.S01E02.1080p.WEB-DL.mkv',
    );
    final best = result.hypotheses.first.parsed;
    expect(best.title, 'Dark');
    expect(best.season, 1);
    expect(best.episodes, [const EpisodeNumber(number: 2)]);
    expect(best.media, MediaHint.episode);
  });

  test('movie with year', () async {
    final result = await parseFilename(
      path: 'Movies/Blade Runner 2049 (2017)/Blade.Runner.2049.2017.2160p.mkv',
    );
    final best = result.hypotheses.first.parsed;
    expect(best.title, 'Blade Runner 2049');
    expect(best.year, 2017);
    expect(best.media, MediaHint.movie);
  });

  test('hypotheses are sorted by weight', () async {
    final result = await parseFilename(path: 'Show - 05.mkv');
    final weights = result.hypotheses.map((h) => h.weight).toList();
    expect(
      weights,
      orderedEquals([...weights]..sort((a, b) => b.compareTo(a))),
    );
    expect(weights.every((w) => w > 0 && w <= 1), isTrue);
  });

  test('parseFilenameWith limits hypotheses', () async {
    final result = await parseFilenameWith(
      path: 'Show - 05.mkv',
      maxHypotheses: 1,
      useFolders: false,
    );
    expect(result.hypotheses, hasLength(1));
  });
}
