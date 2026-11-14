// SPDX-License-Identifier: MPL-2.0
//
// PLAY-005: resume position and Up Next order (mirror the core rules of
// pa-playback `start_position` and pa-library `next_episode`).

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

LibraryItem ep(String id, int? season, int? episode, {String? file}) =>
    LibraryItem(
      id: id,
      kind: ItemKind.episode,
      title: id,
      filePath: file ?? '/tv/$id.mkv',
      showId: 'show:x',
      season: season,
      episode: episode,
    );

Progress pr(int pos, int dur) => Progress(
  itemId: 'a',
  positionMs: pos,
  durationMs: dur,
  updatedAt: DateTime(2026),
);

void main() {
  test('resumePositionMs follows the 2 %..95 % window', () {
    expect(resumePositionMs(null), 0);
    expect(resumePositionMs(pr(50000, 100000)), 50000);
    expect(resumePositionMs(pr(1000, 100000)), 0);
    expect(resumePositionMs(pr(2000, 100000)), 2000);
    expect(resumePositionMs(pr(95000, 100000)), 95000);
    expect(resumePositionMs(pr(96000, 100000)), 0);
    expect(resumePositionMs(pr(7000, 0)), 7000, reason: 'unknown duration');
    expect(resumePositionMs(pr(0, 0)), 0);
  });

  test('nextEpisodeIn: next regular episode across seasons', () {
    final all = [
      ep('s2e1', 2, 1),
      ep('s1e2', 1, 2),
      ep('sp1', 0, 1),
      ep('s1e1', 1, 1),
      ep('s1e10', 1, 10),
      ep('sp2', 0, 2),
    ];
    expect(nextEpisodeIn(all, all[3])?.id, 's1e2');
    expect(nextEpisodeIn(all, all[1])?.id, 's1e10', reason: 'numeric order');
    expect(nextEpisodeIn(all, all[4])?.id, 's2e1', reason: 'S1E10 → S2E1');
    expect(nextEpisodeIn(all, all[0]), isNull, reason: 'last episode');
    expect(nextEpisodeIn(all, all[2])?.id, 'sp2', reason: 'specials');
    expect(nextEpisodeIn(all, all[5]), isNull);
  });

  test('nextEpisodeIn skips the other half of a multi-episode file', () {
    final all = [
      ep('e1', 1, 1, file: '/tv/e1-e2.mkv'),
      ep('e2', 1, 2, file: '/tv/e1-e2.mkv'),
      ep('e3', 1, 3),
    ];
    expect(nextEpisodeIn(all, all[0])?.id, 'e3');
  });

  test('SqliteLibrary.nextEpisode', () async {
    final tmp = Directory.systemTemp.createTempSync('pa_session_');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final media = p.join(tmp.path, 'media');
    for (final f in [
      'TV/Show/Season 1/Show.S01E01.mkv',
      'TV/Show/Season 1/Show.S01E02.mkv',
      'TV/Show/Season 2/Show.S02E01.mkv',
      'Movies/Film (2001)/film.mkv',
    ]) {
      final file = File(p.join(media, f));
      file.parent.createSync(recursive: true);
      file.writeAsBytesSync(List.filled(16, 1));
    }
    final lib = await Library.open(':memory:');
    addTearDown(lib.close);
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await lib.scan().toList();
    final show = (await lib.shows().first).single;
    final eps = await lib.episodes(show.id).first;
    expect(eps.map((e) => (e.season, e.episode)), [(1, 1), (1, 2), (2, 1)]);
    expect((await lib.nextEpisode(eps[0].id))?.id, eps[1].id);
    expect((await lib.nextEpisode(eps[1].id))?.id, eps[2].id);
    expect(await lib.nextEpisode(eps[2].id), isNull);
    final movie = (await lib.movies().first).single;
    expect(await lib.nextEpisode(movie.id), isNull);
    expect(await lib.nextEpisode('missing'), isNull);
  });
}
