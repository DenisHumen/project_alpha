// SPDX-License-Identifier: MPL-2.0
//
// Pages of the music library (MUS-004, 01 §4.11): album (format badge,
// discs, more by the artist), artist (popular tracks, albums, appears on)
// and a track list (genre, composer, folder, mix, playlist).

import 'package:material_ui/material_ui.dart';

import '../../integration/library.dart';
import '../../l10n/l10n.dart';
import '../../services/app_services.dart';
import '../../theme/kinolune_theme.dart';
import '../../widgets/artwork.dart';
import '../../widgets/focusable.dart';
import '../../widgets/format.dart';
import 'music_play.dart';
import 'music_widgets.dart';

void openAlbum(BuildContext context, MusicIndex index, MusicAlbum album) =>
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => AlbumScreen(index: index, album: album),
      ),
    );

void openArtist(BuildContext context, MusicIndex index, MusicArtist artist) =>
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => ArtistScreen(index: index, artist: artist),
      ),
    );

void openTrackList(
  BuildContext context, {
  required String overline,
  required String title,
  required List<MusicTrack> tracks,
  String? artUrl,
  String? artSeed,
}) => Navigator.of(context).push(
  MaterialPageRoute<void>(
    builder: (_) => TrackListScreen(
      overline: overline,
      title: title,
      tracks: tracks,
      artUrl: artUrl,
      artSeed: artSeed ?? title,
    ),
  ),
);

/// A track row that plays [list] from [i]; long press or right click adds
/// it to a playlist.
Widget musicTrackRow(
  BuildContext context,
  List<MusicTrack> list,
  int i, {
  bool showArt = false,
  bool badge = false,
  int? number,
  String? subtitle,
}) {
  final t = list[i];
  final music = libraryMusic(Services.read(context).library);
  return TrackTile(
    key: ValueKey('track-${t.id}'),
    track: t.item,
    showArt: showArt,
    number: number,
    subtitle: subtitle,
    badge: badge ? t.formatLabel : null,
    onTap: () => playMusic(context, list, index: i),
    onContextMenu: music.hasMusicPlaylists
        ? (_) => showAddToMusicPlaylist(context, music, t)
        : null,
  );
}

/// Picks an audio playlist (or makes a new one) and adds [track] to it.
Future<void> showAddToMusicPlaylist(
  BuildContext context,
  LibraryMusic music,
  MusicTrack track,
) async {
  final playlists = await music.musicPlaylists().first;
  if (!context.mounted) return;
  final l = context.l10n;
  final name = TextEditingController();
  final chosen = await showDialog<Object>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(l.musicAddToPlaylist),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (playlists.isEmpty) Text(l.musicPlaylistsEmpty),
              for (final p in playlists)
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.queue_music),
                  title: Text(p.name),
                  subtitle: Text(l.musicPlaylistTrackCount(p.trackCount)),
                  onTap: () => Navigator.pop(context, p),
                ),
              const SizedBox(height: 8),
              TextField(
                controller: name,
                decoration: InputDecoration(labelText: l.libraryPlaylistName),
                onSubmitted: (v) => Navigator.pop(context, v.trim()),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(l.commonCancel),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, name.text.trim()),
          child: Text(l.libraryPlaylistCreate),
        ),
      ],
    ),
  );
  name.dispose();
  if (!context.mounted) return;
  final messenger = ScaffoldMessenger.maybeOf(context);
  final MusicPlaylist playlist;
  switch (chosen) {
    case final MusicPlaylist p:
      playlist = p;
    case final String n when n.isNotEmpty:
      playlist = await music.createMusicPlaylist(n);
    default:
      return;
  }
  final added = await music.addToMusicPlaylist(playlist.id, track.id);
  if (!context.mounted) return;
  messenger
    ?..removeCurrentSnackBar()
    ..showSnackBar(
      SnackBar(
        content: Text(
          added
              ? l.libraryPlaylistAdded(playlist.name)
              : l.libraryPlaylistAlready(playlist.name),
        ),
      ),
    );
}

List<Widget> _playButtons(
  BuildContext context,
  List<MusicTrack> tracks, {
  bool autofocus = false,
}) {
  final l = context.l10n;
  return [
    KButton(
      label: l.musicPlay,
      icon: Icons.play_arrow_rounded,
      primary: true,
      autofocus: autofocus,
      onPressed: () => playMusic(context, tracks),
    ),
    KButton(
      label: l.commonShuffle,
      icon: Icons.shuffle,
      onPressed: () => playMusic(context, tracks, shuffle: true),
    ),
  ];
}

class AlbumScreen extends StatelessWidget {
  const AlbumScreen({super.key, required this.index, required this.album});

  final MusicIndex index;
  final MusicAlbum album;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final tracks = album.tracks;
    final format = album.formatLabel;
    final artist = album.artist == null
        ? null
        : index.artistNamed(album.artist!);
    final more = [
      for (final a in artist?.albums ?? const <MusicAlbum>[])
        if (a.key != album.key) a,
    ];
    final discs = album.isMultiDisc ? album.discs : const [1];
    return MusicPage(
      slivers: [
        SliverPadding(
          padding: musicPagePadding(d, bottom: 24),
          sliver: SliverToBoxAdapter(
            child: MusicPageHeader(
              art: Artwork(
                url: album.artUrl,
                seed: album.artSeed,
                style: ArtStyle.album,
              ),
              overline: l.musicAlbumLabel,
              title: albumTitle(l, album),
              facts: [
                albumArtist(l, album),
                if (album.year != null) '${album.year}',
                l.musicTrackCount(tracks.length),
                formatRuntime(l, album.durationMs),
              ].where((s) => s.isNotEmpty).join('  ·  '),
              badges: [
                if (format != null) FormatBadge(format, hiRes: album.isHiRes),
              ],
              buttons: [
                ..._playButtons(context, tracks, autofocus: !d.isPhone),
                if (artist != null && !d.isTv)
                  KButton(
                    label: artist.name,
                    icon: Icons.person_outline,
                    onPressed: () => openArtist(context, index, artist),
                  ),
              ],
            ),
          ),
        ),
        for (final disc in discs) ...[
          if (discs.length > 1)
            SliverPadding(
              padding: musicPagePadding(d),
              sliver: SliverToBoxAdapter(
                child: Padding(
                  key: ValueKey('disc-$disc'),
                  padding: const EdgeInsets.fromLTRB(10, 16, 10, 6),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.album_outlined,
                        size: 18,
                        color: KColors.accent,
                      ),
                      const SizedBox(width: 8),
                      Text(
                        l.musicDisc(disc),
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          SliverPadding(
            padding: musicPagePadding(d),
            sliver: SliverList.list(
              children: [
                for (var i = 0; i < tracks.length; i++)
                  if ((tracks[i].discNumber ?? 1) == disc || discs.length == 1)
                    musicTrackRow(
                      context,
                      tracks,
                      i,
                      subtitle:
                          tracks[i].artists.length > 1 ||
                              tracks[i].artist != album.artist
                          ? tracks[i].artists.join(', ')
                          : null,
                    ),
              ],
            ),
          ),
        ],
        if (more.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.only(top: 28),
              child: AlbumRow(
                title: l.musicMoreBy(artist!.name),
                albums: more,
                onOpen: (a) => openAlbum(context, index, a),
              ),
            ),
          ),
      ],
    );
  }
}

class ArtistScreen extends StatelessWidget {
  const ArtistScreen({super.key, required this.index, required this.artist});

  final MusicIndex index;
  final MusicArtist artist;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final popular = artist.popular.take(5).toList();
    return MusicPage(
      slivers: [
        SliverPadding(
          padding: musicPagePadding(d, bottom: 8),
          sliver: SliverToBoxAdapter(
            child: MusicPageHeader(
              art: ClipOval(
                child: Artwork(
                  url: artist.artUrl,
                  seed: artist.artSeed,
                  style: ArtStyle.album,
                ),
              ),
              overline: l.musicArtistLabel,
              title: artist.name,
              facts: [
                l.musicAlbumCount(artist.albums.length),
                l.musicTrackCount(artist.tracks.length),
              ].join('  ·  '),
              buttons: _playButtons(
                context,
                artist.tracks,
                autofocus: !d.isPhone,
              ),
            ),
          ),
        ),
        if (popular.isNotEmpty)
          SliverPadding(
            padding: musicPagePadding(d),
            sliver: SliverList.list(
              children: [
                MusicSectionTitle(l.musicPopular),
                for (var i = 0; i < popular.length; i++)
                  musicTrackRow(
                    context,
                    popular,
                    i,
                    showArt: true,
                    subtitle: popular[i].playCount > 0
                        ? l.musicPlayCount(popular[i].playCount)
                        : popular[i].item.album,
                  ),
              ],
            ),
          ),
        if (artist.albums.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.only(top: 24),
              child: AlbumRow(
                title: l.musicSectionAlbums,
                albums: artist.albums,
                onOpen: (a) => openAlbum(context, index, a),
              ),
            ),
          ),
        if (artist.appearsOn.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.only(top: 24),
              child: AlbumRow(
                title: l.musicAppearsOn,
                albums: artist.appearsOn,
                onOpen: (a) => openAlbum(context, index, a),
              ),
            ),
          ),
      ],
    );
  }
}

/// Tracks of a genre, a composer, a folder, a mix or a playlist.
class TrackListScreen extends StatelessWidget {
  const TrackListScreen({
    super.key,
    required this.overline,
    required this.title,
    required this.tracks,
    required this.artSeed,
    this.artUrl,
  });

  final String overline;
  final String title;
  final List<MusicTrack> tracks;
  final String? artUrl;
  final String artSeed;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final total = tracks.fold<int>(0, (s, t) => s + (t.item.durationMs ?? 0));
    return MusicPage(
      slivers: [
        SliverPadding(
          padding: musicPagePadding(d, bottom: 20),
          sliver: SliverToBoxAdapter(
            child: MusicPageHeader(
              art: Artwork(url: artUrl, seed: artSeed, style: ArtStyle.album),
              overline: overline,
              title: title,
              facts: [
                l.musicTrackCount(tracks.length),
                formatRuntime(l, total),
              ].where((s) => s.isNotEmpty).join('  ·  '),
              buttons: _playButtons(context, tracks, autofocus: !d.isPhone),
            ),
          ),
        ),
        SliverPadding(
          padding: musicPagePadding(d),
          sliver: SliverList.builder(
            itemCount: tracks.length,
            itemBuilder: (context, i) => musicTrackRow(
              context,
              tracks,
              i,
              showArt: true,
              badge: !d.isPhone,
            ),
          ),
        ),
      ],
    );
  }
}

/// A playlist page: its tracks are loaded from the library.
class MusicPlaylistScreen extends StatelessWidget {
  const MusicPlaylistScreen({
    super.key,
    required this.index,
    required this.playlist,
  });

  final MusicIndex index;
  final MusicPlaylist playlist;

  @override
  Widget build(BuildContext context) {
    final music = libraryMusic(Services.read(context).library);
    final byId = {for (final t in index.tracks) t.id: t};
    return FutureBuilder<List<LibraryItem>>(
      future: music.musicPlaylistItems(playlist.id),
      builder: (context, snap) {
        final tracks = [
          for (final i in snap.data ?? const <LibraryItem>[])
            byId[i.id] ?? MusicTrack.fromItem(i),
        ];
        return TrackListScreen(
          overline: context.l10n.musicPlaylistLabel,
          title: playlist.name,
          tracks: tracks,
          artUrl: tracks.isEmpty ? null : tracks.first.item.posterUrl,
          artSeed: tracks.isEmpty ? playlist.name : tracks.first.folder,
        );
      },
    );
  }
}
