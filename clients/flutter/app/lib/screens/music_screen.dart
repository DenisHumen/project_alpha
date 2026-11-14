// SPDX-License-Identifier: MPL-2.0
//
// The Music section (MUS-004, 01 §4.11), Spotify-like:
// * Home — recently played, frequently played, mixes (by genre, from the
//   favorites; stable order), recently added albums, artists;
// * Library — playlists, artists, albums, tracks, genres, composers,
//   folders, with the "Downloaded", "Hi-Res" and "Lossless" chips.
// Albums and artists open their pages (lib/screens/music/music_pages.dart).
// Works on every library: the Rust core gives formats and play counts, the
// v0 library plain tracks ([libraryMusic]).

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import 'library_grid_screen.dart' show FilterPill;
import 'library_lists_ui.dart' show libraryListsOf;
import 'music/music_pages.dart';
import 'music/music_play.dart';
import 'music/music_widgets.dart';

export 'music/music_widgets.dart' show TrackTile;

/// Sections of the music library.
enum MusicSection { playlists, artists, albums, tracks, genres, composers, folders }

class MusicScreen extends StatefulWidget {
  const MusicScreen({super.key});

  @override
  State<MusicScreen> createState() => _MusicScreenState();
}

class _MusicScreenState extends State<MusicScreen> {
  LibraryMusic? _music;
  Stream<List<MusicTrack>>? _tracks;
  Stream<List<LibraryItem>>? _favorites;
  Stream<List<MusicPlaylist>>? _playlists;
  bool _library = false;
  MusicSection _section = MusicSection.albums;
  final Set<MusicChip> _chips = {};

  List<MusicTrack>? _indexed;
  MusicIndex _index = MusicIndex.empty;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final services = Services.of(context);
    final music = libraryMusic(services.library);
    if (!identical(music, _music)) {
      _music = music;
      _tracks = music.musicTracks();
      _favorites = libraryListsOf(context)?.favorites();
      _playlists = music.musicPlaylists();
    }
  }

  MusicIndex _indexOf(List<MusicTrack> tracks) {
    if (!identical(tracks, _indexed)) {
      _indexed = tracks;
      _index = MusicIndex(tracks);
      MusicPlayRecorder.of(Services.read(context)).register(tracks);
    }
    return _index;
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    return StreamBuilder<List<MusicTrack>>(
      stream: _tracks,
      builder: (context, snap) {
        final index = _indexOf(snap.data ?? const []);
        return CustomScrollView(
          clipBehavior: Clip.none,
          slivers: [
            SliverToBoxAdapter(
              child: PageTitle(
                l.navMusic,
                trailing: index.isEmpty
                    ? null
                    : KButton(
                        label: l.musicShuffleAll,
                        icon: Icons.shuffle,
                        onPressed: () =>
                            playMusic(context, index.tracks, shuffle: true),
                      ),
              ),
            ),
            if (!index.isEmpty)
              SliverToBoxAdapter(
                child: _PillRow(
                  children: [
                    FilterPill(
                      key: const ValueKey('music-tab-home'),
                      label: l.musicTabHome,
                      selected: !_library,
                      onTap: () => setState(() => _library = false),
                    ),
                    FilterPill(
                      key: const ValueKey('music-tab-library'),
                      label: l.musicTabLibrary,
                      selected: _library,
                      onTap: () => setState(() => _library = true),
                    ),
                  ],
                ),
              ),
            if (index.isEmpty)
              SliverFillRemaining(
                hasScrollBody: false,
                child: EmptyState(
                  icon: Icons.library_music_outlined,
                  title: l.musicEmptyTitle,
                  message: l.musicEmptyMessage,
                ),
              )
            else if (_library)
              ..._librarySlivers(context, index)
            else
              ..._homeSlivers(context, index),
            SliverToBoxAdapter(
              child: SizedBox(height: d.pageInsets.bottom + 90),
            ),
          ],
        );
      },
    );
  }

  // -- Home ------------------------------------------------------------------

  List<Widget> _homeSlivers(BuildContext context, MusicIndex index) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final recent = index.recentlyPlayedAlbums(limit: 12);
    final frequent = index.frequentlyPlayed(limit: d.isPhone ? 5 : 6);
    final added = index.recentlyAddedAlbums(limit: 12);
    final artists = [...index.artists]
      ..sort((a, b) => b.tracks.length.compareTo(a.tracks.length));
    Widget gap(Widget child) => Padding(
      padding: EdgeInsets.only(bottom: d.rowGap),
      child: child,
    );
    return [
      SliverList.list(
        children: [
          if (recent.isEmpty && frequent.isEmpty)
            Padding(
              padding: musicPagePadding(d, bottom: d.rowGap),
              child: Text(
                l.musicHomeEmpty,
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: KColors.textSecondary,
                ),
              ),
            ),
          if (recent.isNotEmpty)
            gap(
              AlbumRow(
                key: const ValueKey('music-recent'),
                title: l.musicRecentlyPlayed,
                albums: recent,
                onOpen: (a) => openAlbum(context, index, a),
              ),
            ),
          if (frequent.isNotEmpty)
            gap(
              Padding(
                key: const ValueKey('music-frequent'),
                padding: musicPagePadding(d),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Padding(
                      padding: EdgeInsets.only(bottom: d.isTv ? 6 : 10),
                      child: Text(
                        l.musicFrequentlyPlayed,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                    ),
                    _TrackColumns(
                      tracks: frequent,
                      columns: d.isPhone ? 1 : 2,
                    ),
                  ],
                ),
              ),
            ),
          StreamBuilder<List<LibraryItem>>(
            stream: _favorites,
            builder: (context, fav) {
              final mixes = index.mixes(
                favoriteIds: {for (final f in fav.data ?? const []) f.id},
              );
              if (mixes.isEmpty) return const SizedBox.shrink();
              return gap(
                MediaRow(
                  key: const ValueKey('music-mixes'),
                  title: l.musicMixes,
                  itemCount: mixes.length,
                  height: albumRowHeight(d),
                  itemBuilder: (context, i) {
                    final m = mixes[i];
                    final name = m.kind == MusicMixKind.favorites
                        ? l.musicMixFavorites
                        : l.musicMixGenre(m.name);
                    return AlbumCard(
                      width: d.albumWidth,
                      seed: 'mix:${m.key}',
                      artUrl: m.artUrl,
                      title: name,
                      subtitle: l.musicTrackCount(m.tracks.length),
                      onTap: () => openTrackList(
                        context,
                        overline: l.musicMixLabel,
                        title: name,
                        tracks: m.tracks,
                        artUrl: m.artUrl,
                        artSeed: 'mix:${m.key}',
                      ),
                    );
                  },
                ),
              );
            },
          ),
          if (added.isNotEmpty)
            gap(
              AlbumRow(
                key: const ValueKey('music-added'),
                title: l.musicRecentlyAdded,
                albums: added,
                onOpen: (a) => openAlbum(context, index, a),
              ),
            ),
          if (artists.isNotEmpty)
            gap(
              MediaRow(
                key: const ValueKey('music-artists'),
                title: l.musicSectionArtists,
                itemCount: artists.length,
                height: albumRowHeight(d),
                onSeeAll: () => setState(() {
                  _library = true;
                  _section = MusicSection.artists;
                }),
                itemBuilder: (context, i) => ArtistCard(
                  width: d.albumWidth,
                  artist: artists[i],
                  onTap: () => openArtist(context, index, artists[i]),
                ),
              ),
            ),
        ],
      ),
    ];
  }

  // -- Library ---------------------------------------------------------------

  String _sectionLabel(AppLocalizations l, MusicSection s) => switch (s) {
    MusicSection.playlists => l.musicSectionPlaylists,
    MusicSection.artists => l.musicSectionArtists,
    MusicSection.albums => l.musicSectionAlbums,
    MusicSection.tracks => l.musicSectionTracks,
    MusicSection.genres => l.musicSectionGenres,
    MusicSection.composers => l.musicSectionComposers,
    MusicSection.folders => l.musicSectionFolders,
  };

  List<Widget> _librarySlivers(BuildContext context, MusicIndex all) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final index = all.filter(_chips);
    final pills = SliverToBoxAdapter(
      child: _PillRow(
        scroll: true,
        children: [
          for (final s in MusicSection.values)
            FilterPill(
              key: ValueKey('music-section-${s.name}'),
              label: _sectionLabel(l, s),
              selected: _section == s,
              onTap: () => setState(() => _section = s),
            ),
        ],
      ),
    );
    final chips = SliverToBoxAdapter(
      child: _PillRow(
        scroll: true,
        children: [
          for (final (c, label, icon) in [
            (MusicChip.downloaded, l.musicChipDownloaded, Icons.download_done),
            (MusicChip.hiRes, l.musicChipHiRes, Icons.high_quality_outlined),
            (MusicChip.lossless, l.musicChipLossless, Icons.graphic_eq),
          ])
            FilterPill(
              key: ValueKey('music-chip-${c.name}'),
              label: label,
              icon: icon,
              selected: _chips.contains(c),
              onTap: () => setState(
                () => _chips.contains(c) ? _chips.remove(c) : _chips.add(c),
              ),
            ),
        ],
      ),
    );
    if (_section == MusicSection.playlists) {
      return [pills, ..._playlistSlivers(context, all)];
    }
    if (index.isEmpty) {
      return [
        pills,
        chips,
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.only(top: 40),
            child: EmptyState(
              icon: Icons.filter_alt_off_outlined,
              title: l.musicNoMatches,
            ),
          ),
        ),
      ];
    }
    final body = switch (_section) {
      MusicSection.playlists => const SliverToBoxAdapter(),
      MusicSection.albums => AlbumGrid(
        albums: index.albums,
        onOpen: (a) => openAlbum(context, all, a),
      ),
      MusicSection.artists => SliverGrid(
        gridDelegate: CardGridDelegate(
          minItemWidth: d.albumWidth,
          mainSpacing: 18,
          crossSpacing: d.gap,
          artAspect: 1,
          extraHeight: cardCaptionHeight(d),
        ),
        delegate: SliverChildBuilderDelegate(
          (context, i) => LayoutBuilder(
            builder: (context, c) => ArtistCard(
              width: c.maxWidth,
              artist: index.artists[i],
              onTap: () => openArtist(
                context,
                all,
                all.artistNamed(index.artists[i].name) ?? index.artists[i],
              ),
            ),
          ),
          childCount: index.artists.length,
        ),
      ),
      MusicSection.tracks => SliverList.builder(
        itemCount: index.tracks.length,
        itemBuilder: (context, i) => musicTrackRow(
          context,
          index.tracks,
          i,
          showArt: true,
          badge: !d.isPhone,
        ),
      ),
      MusicSection.genres => _groupList(
        context,
        index.genres,
        Icons.local_offer_outlined,
        l.musicGenreLabel,
      ),
      MusicSection.composers => _groupList(
        context,
        index.composers,
        Icons.edit_note,
        l.musicComposerLabel,
      ),
      MusicSection.folders => _groupList(
        context,
        index.folders,
        Icons.folder_outlined,
        l.musicFolderLabel,
      ),
    };
    return [
      pills,
      chips,
      SliverPadding(padding: musicPagePadding(d), sliver: body),
    ];
  }

  Widget _groupList(
    BuildContext context,
    List<MusicGroup> groups,
    IconData icon,
    String overline,
  ) {
    final l = context.l10n;
    if (groups.isEmpty) {
      return SliverToBoxAdapter(
        child: EmptyState(icon: icon, title: l.musicNoMatches),
      );
    }
    return SliverList.builder(
      itemCount: groups.length,
      itemBuilder: (context, i) {
        final g = groups[i];
        return _ListRow(
          key: ValueKey('music-group-${g.key}'),
          icon: icon,
          title: g.name,
          subtitle: l.musicTrackCount(g.tracks.length),
          onTap: () => openTrackList(
            context,
            overline: overline,
            title: g.name,
            tracks: g.tracks,
            artUrl: g.artUrl,
            artSeed: g.artSeed,
          ),
        );
      },
    );
  }

  List<Widget> _playlistSlivers(BuildContext context, MusicIndex index) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final music = _music!;
    if (!music.hasMusicPlaylists) {
      return [
        SliverToBoxAdapter(
          child: EmptyState(
            icon: Icons.queue_music,
            title: l.musicPlaylistsEmpty,
            message: l.musicPlaylistsUnavailable,
          ),
        ),
      ];
    }
    return [
      SliverPadding(
        padding: musicPagePadding(d),
        sliver: StreamBuilder<List<MusicPlaylist>>(
          stream: _playlists,
          builder: (context, snap) {
            final lists = snap.data ?? const <MusicPlaylist>[];
            return SliverList.list(
              children: [
                _ListRow(
                  key: const ValueKey('music-playlist-new'),
                  icon: Icons.add,
                  title: l.libraryPlaylistNew,
                  onTap: () => _newPlaylist(context, music),
                ),
                if (lists.isEmpty)
                  Padding(
                    padding: const EdgeInsets.all(10),
                    child: Text(
                      l.musicPlaylistsEmpty,
                      style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                        color: KColors.textSecondary,
                      ),
                    ),
                  ),
                for (final p in lists)
                  _ListRow(
                    key: ValueKey('music-playlist-${p.id}'),
                    icon: Icons.queue_music,
                    title: p.name,
                    subtitle: l.musicPlaylistTrackCount(p.trackCount),
                    onTap: () => Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (_) =>
                            MusicPlaylistScreen(index: index, playlist: p),
                      ),
                    ),
                  ),
              ],
            );
          },
        ),
      ),
    ];
  }

  Future<void> _newPlaylist(BuildContext context, LibraryMusic music) async {
    final l = context.l10n;
    final name = TextEditingController();
    final chosen = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.libraryPlaylistNew),
        content: TextField(
          controller: name,
          autofocus: true,
          decoration: InputDecoration(labelText: l.libraryPlaylistName),
          onSubmitted: (v) => Navigator.pop(context, v.trim()),
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
    if (chosen != null && chosen.isNotEmpty) {
      await music.createMusicPlaylist(chosen);
    }
  }
}

/// A row of pills under the title; [scroll] scrolls it sideways.
class _PillRow extends StatelessWidget {
  const _PillRow({required this.children, this.scroll = false});

  final List<Widget> children;
  final bool scroll;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final row = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (var i = 0; i < children.length; i++) ...[
          if (i > 0) const SizedBox(width: 8),
          children[i],
        ],
      ],
    );
    return Padding(
      padding: EdgeInsets.only(bottom: d.isTv ? 12 : 16),
      child: FocusTraversalGroup(
        child: scroll
            ? SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                clipBehavior: Clip.none,
                padding: musicPagePadding(d),
                child: row,
              )
            : Padding(
                padding: musicPagePadding(d),
                child: Align(alignment: Alignment.centerLeft, child: row),
              ),
      ),
    );
  }
}

/// Tracks in [columns] columns (Frequently played).
class _TrackColumns extends StatelessWidget {
  const _TrackColumns({required this.tracks, required this.columns});

  final List<MusicTrack> tracks;
  final int columns;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final rows = (tracks.length / columns).ceil();
    Widget cell(int i) => i < tracks.length
        ? musicTrackRow(
            context,
            tracks,
            i,
            showArt: true,
            subtitle: [
              ?tracks[i].artist,
              l.musicPlayCount(tracks[i].playCount),
            ].join(' · '),
          )
        : const SizedBox.shrink();
    return Column(
      children: [
        for (var r = 0; r < rows; r++)
          Row(
            children: [
              for (var c = 0; c < columns; c++) ...[
                if (c > 0) const SizedBox(width: 16),
                Expanded(child: cell(c * rows + r)),
              ],
            ],
          ),
      ],
    );
  }
}

/// A focusable list row: icon, title, count.
class _ListRow extends StatelessWidget {
  const _ListRow({
    super.key,
    required this.icon,
    required this.title,
    required this.onTap,
    this.subtitle,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return KFocusable(
      onActivate: onTap,
      focusScale: 1.0,
      light: false,
      semanticLabel: title,
      builder: (context, focused) => AnimatedContainer(
        duration: KMotion.focus,
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
        color: focused
            ? Colors.white.withValues(alpha: .09)
            : Colors.transparent,
        child: Row(
          children: [
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: Colors.white.withValues(alpha: .08),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Icon(icon, size: 20, color: KColors.textSecondary),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.titleMedium,
                  ),
                  if (subtitle != null)
                    Text(subtitle!, style: theme.textTheme.labelSmall),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
