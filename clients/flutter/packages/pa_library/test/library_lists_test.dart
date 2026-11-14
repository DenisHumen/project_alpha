// SPDX-License-Identifier: MPL-2.0
// LIB-010 through RustLibrary on the real native core: two files of one
// film are one item with a version choice; favorites, collections and video
// playlists.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

/// A fake media file with content unique to [seed] (fingerprints differ).
void media(String root, String rel, int seed) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.generate(150000, (i) => (i * 31 + seed) & 0xff));
}

const hd = 'Movies/Inception (2010)/Inception.2010.1080p.BluRay.mkv';
const uhd = 'Movies/Inception (2010)/Inception.2010.2160p.HDR10.mkv';

void main() {
  late Directory tmp;
  late String root;
  late RustLibrary lib;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
  });

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('pa_lists_');
    root = p.join(tmp.path, 'media');
    media(root, hd, 1);
    media(root, uhd, 2);
    media(root, 'Movies/Arrival (2016)/Arrival.2016.mkv', 3);
    media(root, 'TV/Dark/Season 1/Dark.S01E01.mkv', 4);
    media(root, 'TV/Dark/Season 1/Dark.S01E02.mkv', 5);
    lib = await RustLibrary.open(p.join(tmp.path, 'db', 'core.db'));
    await lib.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: root,
      ),
    );
    await lib.scan().toList();
  });

  tearDown(() async {
    await lib.close();
    tmp.deleteSync(recursive: true);
  });

  test('two files of one film are one item with versions', () async {
    final movies = await lib.movies().first;
    expect([for (final m in movies) m.title], ['Arrival', 'Inception']);
    final inception = movies.last;
    expect(inception.versionCount, 2);
    expect(movies.first.versionCount, 1);

    final versions = await lib.versions(inception.id);
    expect([for (final v in versions) v.label], ['4K · HDR10', '1080p']);
    expect(versions.first.filePath, p.join(root, uhd));
    expect(versions.first.playbackUri, p.join(root, uhd));
    expect(
      [for (final v in versions) v.isDefault].where((d) => d),
      hasLength(1),
    );
    final defaultPath = versions.firstWhere((v) => v.isDefault).filePath;
    expect(inception.filePath, defaultPath);

    // The 4K version becomes the default: the item plays it.
    await lib.setDefaultVersion(inception.id, versions.first.id);
    final item = await lib.item(inception.id);
    expect(item!.filePath, p.join(root, uhd));
    expect(item.versionCount, 2);
    expect((await lib.versions(inception.id)).first.isDefault, isTrue);
    expect(
      (await lib.movies().first).where((m) => m.title == 'Inception'),
      hasLength(1),
    );
    await lib.setDefaultVersion(inception.id, null);
    expect((await lib.item(inception.id))!.filePath, defaultPath);
    await expectLater(
      lib.setDefaultVersion(inception.id, '999999'),
      throwsA(anything),
    );
  });

  test('favorites follow changes and can leave Home', () async {
    final movies = await lib.movies().first;
    final show = (await lib.shows().first).single;
    final favorites = lib.favorites(pinnedOnly: true);
    final seen = <List<String>>[];
    final sub = favorites.listen((l) => seen.add([for (final i in l) i.id]));
    await Future<void>.delayed(const Duration(milliseconds: 100));

    expect(await lib.isFavorite(movies.first.id), isFalse);
    await lib.setFavorite(movies.first.id, true);
    await lib.setFavorite(show.id, true);
    expect(await lib.isFavorite(movies.first.id), isTrue);
    final all = await lib.favorites().first;
    expect({for (final i in all) i.id}, {movies.first.id, show.id});

    await lib.setFavoritePinned(show.id, false);
    expect(
      [for (final i in await lib.favorites(pinnedOnly: true).first) i.id],
      [movies.first.id],
    );
    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(seen.last, [movies.first.id]);
    await sub.cancel();

    await lib.setFavorite(movies.first.id, false);
    expect(await lib.isFavorite(movies.first.id), isFalse);
  });

  test('collections and video playlists', () async {
    final movies = await lib.movies().first;
    final show = (await lib.shows().first).single;
    final episodes = await lib.episodes(show.id).first;

    final c = await lib.createCollection('Sci-fi');
    expect(await lib.addToCollection(c.id, movies.last.id), isTrue);
    expect(await lib.addToCollection(c.id, movies.first.id), isTrue);
    expect(await lib.addToCollection(c.id, movies.first.id), isFalse);
    expect(
      [for (final i in await lib.collectionItems(c.id)) i.id],
      [movies.last.id, movies.first.id],
    );
    expect((await lib.collections()).single.itemCount, 2);
    expect((await lib.itemCollections(movies.first.id)).single.name, 'Sci-fi');
    await lib.deleteCollection(c.id);
    expect(await lib.collections(), isEmpty);

    final pl = await lib.createPlaylist('Weekend');
    expect(await lib.addToPlaylist(pl.id, movies.first.id), 1);
    expect(await lib.addToPlaylist(pl.id, show.id), 2);
    expect(
      [for (final i in await lib.playlistItems(pl.id)) i.id],
      [movies.first.id, episodes[0].id, episodes[1].id],
    );
    await lib.reorderPlaylist(pl.id, [episodes[1].id]);
    expect((await lib.playlistItems(pl.id)).first.id, episodes[1].id);
    expect(await lib.removeFromPlaylist(pl.id, movies.first.id), 1);
    expect((await lib.playlists()).single.itemCount, 2);
    await lib.deletePlaylist(pl.id);
    expect(await lib.playlists(), isEmpty);
  });
}
