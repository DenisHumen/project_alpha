// SPDX-License-Identifier: MPL-2.0

// Grouping of music tracks into the views of the music UI (MUS-004): albums
// (with discs), artists, genres, composers, folders, the Hi-Res / Lossless /
// downloaded filters and the rows of the music Home. Pure and
// deterministic: the same tracks give the same rows and mixes.

import 'music.dart';

/// Filter chips of the music library.
enum MusicChip { downloaded, hiRes, lossless }

/// Folded name for grouping: lower case, single spaces.
String foldName(String s) =>
    s.trim().toLowerCase().replaceAll(RegExp(r'\s+'), ' ');

/// An album: tracks grouped by library album id, else album artist and
/// title.
class MusicAlbum {
  MusicAlbum._(this.key, this.title, this.artist, this.tracks);

  final String key;

  /// Null: no album tag.
  final String? title;

  /// Album artist; null if unknown.
  final String? artist;

  /// By disc, then track number.
  final List<MusicTrack> tracks;

  MusicTrack get first => tracks.first;
  int? get year => first.albumYear ?? first.item.year;
  String? get artUrl => first.item.posterUrl;
  bool get compilation => first.compilation;

  /// Seed of generated art: the folder of the first track.
  String get artSeed => first.folder;

  int get durationMs =>
      tracks.fold<int>(0, (s, t) => s + (t.item.durationMs ?? 0));

  /// Disc numbers in order; one entry (`1`) for a single disc.
  List<int> get discs {
    final set = {for (final t in tracks) t.discNumber ?? 1};
    return set.toList()..sort();
  }

  bool get isMultiDisc =>
      discs.length > 1 || tracks.any((t) => (t.discTotal ?? 1) > 1);

  List<MusicTrack> disc(int n) =>
      [for (final t in tracks) if ((t.discNumber ?? 1) == n) t];

  /// The most common format of the tracks (`FLAC 24/96`).
  String? get formatLabel {
    final counts = <String, int>{};
    for (final t in tracks) {
      final f = t.formatLabel;
      if (f != null) counts[f] = (counts[f] ?? 0) + 1;
    }
    if (counts.isEmpty) return null;
    return (counts.entries.toList()..sort((a, b) => b.value - a.value))
        .first
        .key;
  }

  bool get isHiRes => tracks.any((t) => t.isHiRes);
  bool get isLossless => tracks.every((t) => t.isLossless);

  DateTime? get addedAt => _latest(tracks.map((t) => t.item.addedAt));
  DateTime? get lastPlayedAt => _latest(tracks.map((t) => t.lastPlayedAt));
}

DateTime? _latest(Iterable<DateTime?> times) {
  DateTime? best;
  for (final t in times) {
    if (t != null && (best == null || t.isAfter(best))) best = t;
  }
  return best;
}

/// An artist with their albums, the albums they appear on and their
/// tracks.
class MusicArtist {
  MusicArtist._(this.key, this.name);

  final String key;
  final String name;

  /// Albums with this album artist, newest first.
  final List<MusicAlbum> albums = [];

  /// Other albums with a track of this artist (compilations, features).
  final List<MusicAlbum> appearsOn = [];

  /// Tracks of this artist (track or album artist), in library order.
  final List<MusicTrack> tracks = [];

  String? get artUrl => albums.isNotEmpty
      ? albums.first.artUrl
      : (appearsOn.isNotEmpty ? appearsOn.first.artUrl : null);
  String get artSeed => albums.isNotEmpty
      ? albums.first.artSeed
      : (tracks.isNotEmpty ? tracks.first.folder : name);

  /// Most played first (by play count, then last play), then album order.
  List<MusicTrack> get popular {
    final list = [...tracks];
    final order = {for (var i = 0; i < tracks.length; i++) tracks[i].id: i};
    list.sort((a, b) {
      final c = b.playCount.compareTo(a.playCount);
      if (c != 0) return c;
      final la = a.lastPlayedAt?.millisecondsSinceEpoch ?? 0;
      final lb = b.lastPlayedAt?.millisecondsSinceEpoch ?? 0;
      if (la != lb) return lb.compareTo(la);
      return order[a.id]!.compareTo(order[b.id]!);
    });
    return list;
  }
}

/// A named group of tracks: a genre, a composer, a folder or a mix.
class MusicGroup {
  MusicGroup(this.key, this.name, this.tracks);

  final String key;
  final String name;
  final List<MusicTrack> tracks;

  String? get artUrl => tracks.isEmpty ? null : tracks.first.item.posterUrl;
  String get artSeed => tracks.isEmpty ? name : tracks.first.folder;
}

/// Kind of a Home mix.
enum MusicMixKind { genre, favorites }

/// A mix of the music Home: tracks of one genre or of the favorites, in a
/// stable shuffled order.
class MusicMix extends MusicGroup {
  MusicMix(this.kind, super.key, super.name, super.tracks);
  final MusicMixKind kind;
}

/// FNV-1a 32 of a string: a stable seed (String.hashCode may change).
int stableHash(String s) {
  var h = 0x811c9dc5;
  for (final c in s.codeUnits) {
    h ^= c;
    h = (h * 0x01000193) & 0xffffffff;
  }
  return h;
}

/// [tracks] in an order shuffled by [seed]: the same input gives the same
/// order on every device and run.
List<MusicTrack> stableShuffle(List<MusicTrack> tracks, String seed) {
  final keyed = [
    for (final t in tracks) (stableHash('$seed\u0000${t.id}'), t),
  ]..sort((a, b) => a.$1 != b.$1 ? a.$1.compareTo(b.$1) : a.$2.id.compareTo(b.$2.id));
  return [for (final (_, t) in keyed) t];
}

int _byName(String a, String b) {
  final c = a.toLowerCase().compareTo(b.toLowerCase());
  return c != 0 ? c : a.compareTo(b);
}

int _byTrack(MusicTrack a, MusicTrack b) {
  final d = (a.discNumber ?? 1).compareTo(b.discNumber ?? 1);
  if (d != 0) return d;
  final n = (a.item.trackNumber ?? 1 << 20).compareTo(
    b.item.trackNumber ?? 1 << 20,
  );
  if (n != 0) return n;
  return _byName(a.title, b.title);
}

/// The music views over a list of tracks.
class MusicIndex {
  MusicIndex(List<MusicTrack> tracks) : tracks = List.unmodifiable(tracks) {
    _build();
  }

  static final empty = MusicIndex(const []);

  /// In library order (artist, album, disc, number).
  final List<MusicTrack> tracks;

  /// By title.
  late final List<MusicAlbum> albums;

  /// By name.
  late final List<MusicArtist> artists;

  /// By name.
  late final List<MusicGroup> genres;
  late final List<MusicGroup> composers;

  /// By path.
  late final List<MusicGroup> folders;

  final Map<String, MusicAlbum> _albumOf = {};
  final Map<String, MusicArtist> _artists = {};

  bool get isEmpty => tracks.isEmpty;

  /// The album of a track.
  MusicAlbum? albumOf(MusicTrack t) => _albumOf[t.id];

  MusicAlbum? albumByKey(String key) {
    for (final a in albums) {
      if (a.key == key) return a;
    }
    return null;
  }

  MusicArtist? artistNamed(String name) => _artists[foldName(name)];

  /// The tracks that pass every chip in [chips].
  MusicIndex filter(Set<MusicChip> chips) {
    if (chips.isEmpty) return this;
    return MusicIndex([
      for (final t in tracks)
        if ((!chips.contains(MusicChip.downloaded) || t.local) &&
            (!chips.contains(MusicChip.hiRes) || t.isHiRes) &&
            (!chips.contains(MusicChip.lossless) || t.isLossless))
          t,
    ]);
  }

  /// Tracks played most recently first.
  List<MusicTrack> recentlyPlayed({int limit = 20}) {
    final played = [for (final t in tracks) if (t.lastPlayedAt != null) t]
      ..sort((a, b) {
        final c = b.lastPlayedAt!.compareTo(a.lastPlayedAt!);
        return c != 0 ? c : a.id.compareTo(b.id);
      });
    return played.take(limit).toList();
  }

  /// Albums played most recently first.
  List<MusicAlbum> recentlyPlayedAlbums({int limit = 20}) {
    final seen = <String>{};
    final out = <MusicAlbum>[];
    for (final t in recentlyPlayed(limit: tracks.length)) {
      final a = albumOf(t);
      if (a != null && seen.add(a.key)) out.add(a);
      if (out.length == limit) break;
    }
    return out;
  }

  /// Most played tracks (by play count, then last play).
  List<MusicTrack> frequentlyPlayed({int limit = 20}) {
    final played = [for (final t in tracks) if (t.playCount > 0) t]
      ..sort((a, b) {
        final c = b.playCount.compareTo(a.playCount);
        if (c != 0) return c;
        final l = (b.lastPlayedAt ?? DateTime(0)).compareTo(
          a.lastPlayedAt ?? DateTime(0),
        );
        return l != 0 ? l : a.id.compareTo(b.id);
      });
    return played.take(limit).toList();
  }

  /// Albums added most recently first.
  List<MusicAlbum> recentlyAddedAlbums({int limit = 20}) {
    final list = [for (final a in albums) if (a.addedAt != null) a]
      ..sort((a, b) {
        final c = b.addedAt!.compareTo(a.addedAt!);
        return c != 0 ? c : a.key.compareTo(b.key);
      });
    return list.take(limit).toList();
  }

  /// Mixes: one per genre with the most tracks (at most [genreLimit],
  /// genres with at least [minTracks]), and one of the favorite tracks
  /// ([favoriteIds]) if there are [minTracks] of them. Each holds at most
  /// [size] tracks in a stable shuffled order.
  List<MusicMix> mixes({
    Set<String> favoriteIds = const {},
    int genreLimit = 4,
    int minTracks = 3,
    int size = 50,
  }) {
    final out = <MusicMix>[];
    final favorites = [
      for (final t in tracks) if (favoriteIds.contains(t.id)) t,
    ];
    if (favorites.length >= minTracks) {
      out.add(
        MusicMix(
          MusicMixKind.favorites,
          'favorites',
          '',
          stableShuffle(favorites, 'favorites').take(size).toList(),
        ),
      );
    }
    final top = [for (final g in genres) if (g.tracks.length >= minTracks) g]
      ..sort((a, b) {
        final c = b.tracks.length.compareTo(a.tracks.length);
        return c != 0 ? c : _byName(a.name, b.name);
      });
    for (final g in top.take(genreLimit)) {
      out.add(
        MusicMix(
          MusicMixKind.genre,
          'genre:${g.key}',
          g.name,
          stableShuffle(g.tracks, 'genre:${g.key}').take(size).toList(),
        ),
      );
    }
    return out;
  }

  void _build() {
    // Albums.
    final byKey = <String, List<MusicTrack>>{};
    for (final t in tracks) {
      final key =
          t.albumId ??
          (t.item.album == null
              ? 'dir:${t.folder}'
              : 'aa:${foldName(t.albumArtist ?? '')}|${foldName(t.item.album!)}');
      (byKey[key] ??= []).add(t);
    }
    final albums = <MusicAlbum>[];
    for (final MapEntry(:key, value: list) in byKey.entries) {
      list.sort(_byTrack);
      final a = MusicAlbum._(
        key,
        list.first.item.album,
        list.first.albumArtist,
        List.unmodifiable(list),
      );
      albums.add(a);
      for (final t in list) {
        _albumOf[t.id] = a;
      }
    }
    albums.sort((a, b) {
      final c = _byName(a.title ?? '￿', b.title ?? '￿');
      return c != 0 ? c : a.key.compareTo(b.key);
    });
    this.albums = List.unmodifiable(albums);

    // Artists: album artists own albums; track artists appear on others.
    MusicArtist artist(String name) =>
        _artists[foldName(name)] ??= MusicArtist._(foldName(name), name);
    for (final a in albums) {
      final owners = a.first.albumArtists.isNotEmpty
          ? a.first.albumArtists
          : [?a.first.artist];
      final ownerKeys = {for (final o in owners) foldName(o)};
      for (final o in owners) {
        if (o.trim().isNotEmpty) artist(o).albums.add(a);
      }
      final guests = <String>{};
      for (final t in a.tracks) {
        for (final n in t.artists) {
          if (n.trim().isEmpty) continue;
          final k = foldName(n);
          if (!ownerKeys.contains(k) && guests.add(k)) {
            artist(n).appearsOn.add(a);
          }
        }
      }
    }
    // Tracks in library order, of their track and album artists.
    for (final t in tracks) {
      final names = [...t.artists, ...t.albumArtists, ?t.artist];
      final seen = <String>{};
      for (final n in names) {
        if (n.trim().isEmpty || !seen.add(foldName(n))) continue;
        artist(n).tracks.add(t);
      }
    }
    int newest(MusicAlbum a, MusicAlbum b) {
      final c = (b.year ?? 0).compareTo(a.year ?? 0);
      return c != 0 ? c : _byName(a.title ?? '', b.title ?? '');
    }

    for (final ar in _artists.values) {
      ar.albums.sort(newest);
      ar.appearsOn.sort(newest);
    }
    artists = List.unmodifiable(
      _artists.values.toList()..sort((a, b) => _byName(a.name, b.name)),
    );

    List<MusicGroup> groups(
      Iterable<String> Function(MusicTrack) names, {
      bool fold = true,
    }) {
      final map = <String, MusicGroup>{};
      for (final t in tracks) {
        for (final n in names(t)) {
          if (n.trim().isEmpty) continue;
          final k = fold ? foldName(n) : n;
          (map[k] ??= MusicGroup(k, n.trim(), [])).tracks.add(t);
        }
      }
      return List.unmodifiable(
        map.values.toList()..sort((a, b) => _byName(a.name, b.name)),
      );
    }

    genres = groups((t) => t.genres);
    composers = groups((t) => t.composers);
    folders = groups((t) => [t.folder], fold: false);
  }
}
