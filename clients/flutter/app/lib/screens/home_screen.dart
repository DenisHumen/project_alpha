// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'playback.dart';
import 'routes.dart';

class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, this.onOpenSection});

  /// Switches the shell to a section (1 = Movies, 2 = Shows, 3 = Music).
  final ValueChanged<int>? onOpenSection;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  final _featured = ValueNotifier<LibraryItem?>(null);
  final _debounce = Debouncer(const Duration(milliseconds: 200));
  late Library _lib;
  Stream<List<LibraryItem>>? _movies, _shows, _tracks;
  Stream<List<(LibraryItem, Progress)>>? _cw;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final lib = Services.of(context).library;
    if (_movies == null || !identical(lib, _lib)) {
      _lib = lib;
      _movies = lib.movies();
      _shows = lib.shows();
      _tracks = lib.tracks();
      _cw = lib.continueWatching();
    }
  }

  @override
  void dispose() {
    _debounce.dispose();
    _featured.dispose();
    super.dispose();
  }

  void _feature(LibraryItem item, bool focused) {
    if (focused) _debounce(() => _featured.value = item);
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return StreamBuilder<List<LibraryItem>>(
      stream: _movies,
      builder: (context, moviesSnap) => StreamBuilder<List<LibraryItem>>(
        stream: _shows,
        builder: (context, showsSnap) => StreamBuilder<List<(LibraryItem, Progress)>>(
          stream: _cw,
          builder: (context, cwSnap) => StreamBuilder<List<LibraryItem>>(
            stream: _tracks,
            builder: (context, tracksSnap) {
              final movies = moviesSnap.data ?? const [];
              final shows = showsSnap.data ?? const [];
              final cw = cwSnap.data ?? const [];
              final albums = groupAlbums(tracksSnap.data ?? const []);
              if (movies.isEmpty && shows.isEmpty && albums.isEmpty) {
                return const EmptyState(
                  icon: Icons.video_library_outlined,
                  title: 'Your library is empty',
                  message: 'Add a folder or a network share in Settings, then run a scan.',
                );
              }
              final showsById = {for (final s in shows) s.id: s};
              final initial = movies.isNotEmpty ? movies.first : shows.first;
              return ListView(
                padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 24),
                clipBehavior: Clip.none,
                children: [
                  ValueListenableBuilder<LibraryItem?>(
                    valueListenable: _featured,
                    builder: (context, f, _) => _Hero(item: f ?? initial, showsById: showsById),
                  ),
                  if (cw.isNotEmpty) ...[
                    MediaRow(
                      title: 'Continue Watching',
                      itemCount: cw.length,
                      height: d.landscapeWidth * 9 / 16 + (d.isTv ? 58 : 50),
                      itemBuilder: (context, i) {
                        final (item, p) = cw[i];
                        final show = item.showId == null ? null : showsById[item.showId];
                        return LandscapeCard(
                          width: d.landscapeWidth,
                          seed: item.id,
                          imageUrl: item.backdropUrl ?? show?.backdropUrl,
                          title: show?.title ?? item.title,
                          subtitle: item.kind == ItemKind.episode
                              ? '${episodeCode(item)} · ${formatRemaining(p)}'
                              : formatRemaining(p),
                          badge: item.kind == ItemKind.episode ? episodeCode(item) : null,
                          progress: p.positionMs / p.durationMs,
                          onFocus: (f) => _feature(show ?? item, f),
                          onTap: () => playVideo(context, item, startPositionMs: p.positionMs),
                        );
                      },
                    ),
                    SizedBox(height: d.rowGap),
                  ],
                  if (movies.isNotEmpty) ...[
                    MediaRow(
                      title: 'Recently Added Movies',
                      itemCount: movies.length,
                      height: d.posterWidth * 1.5 + (d.isTv ? 52 : 46),
                      onSeeAll: () => widget.onOpenSection?.call(1),
                      itemBuilder: (context, i) => PosterCard(
                        item: movies[i],
                        width: d.posterWidth,
                        onFocus: (f) => _feature(movies[i], f),
                        onTap: () => openItem(context, movies[i]),
                      ),
                    ),
                    SizedBox(height: d.rowGap),
                  ],
                  if (shows.isNotEmpty) ...[
                    MediaRow(
                      title: 'TV Shows',
                      itemCount: shows.length,
                      height: d.posterWidth * 1.5 + (d.isTv ? 52 : 46),
                      onSeeAll: () => widget.onOpenSection?.call(2),
                      itemBuilder: (context, i) => PosterCard(
                        item: shows[i],
                        width: d.posterWidth,
                        onFocus: (f) => _feature(shows[i], f),
                        onTap: () => openItem(context, shows[i]),
                      ),
                    ),
                    SizedBox(height: d.rowGap),
                  ],
                  if (albums.isNotEmpty)
                    MediaRow(
                      title: 'Music',
                      itemCount: albums.length,
                      height: d.albumWidth + 50,
                      onSeeAll: () => widget.onOpenSection?.call(3),
                      itemBuilder: (context, i) => AlbumCard(
                        width: d.albumWidth,
                        seed: albums[i].artSeed,
                        title: albums[i].title,
                        subtitle: albums[i].artist,
                        onTap: () => playTracks(context, albums[i].tracks, 0),
                      ),
                    ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

/// Top shelf: backdrop of the featured (focused) title, crossfading.
class _Hero extends StatelessWidget {
  const _Hero({required this.item, required this.showsById});

  final LibraryItem item;
  final Map<String, LibraryItem> showsById;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final height = d.isTv ? 250.0 : (d.isPhone ? 340.0 : 380.0);
    final meta = [
      if (item.year != null) '${item.year}',
      if (item.kind == ItemKind.show) 'TV Show' else formatRuntime(item.durationMs),
    ].where((s) => s.isNotEmpty).join('  ·  ');
    return SizedBox(
      height: height,
      child: Stack(fit: StackFit.expand, children: [
        AnimatedSwitcher(
          duration: KMotion.crossfadeArt,
          child: Artwork(key: ValueKey(item.id), url: item.backdropUrl, seed: item.id, style: ArtStyle.backdrop),
        ),
        const DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.centerLeft,
              end: Alignment.centerRight,
              colors: [Color(0xE60B0B0F), Color(0x660B0B0F), Color(0x000B0B0F)],
              stops: [0, .45, .8],
            ),
          ),
        ),
        const DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [Color(0x000B0B0F), Color(0x000B0B0F), KColors.bgBase],
              stops: [0, .55, 1],
            ),
          ),
        ),
        Padding(
          padding: EdgeInsets.fromLTRB(d.pageInsets.left, d.pageInsets.top, d.pageInsets.right, d.isTv ? 14 : 24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.end,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              ConstrainedBox(
                constraints: BoxConstraints(maxWidth: d.isTv ? 440 : 560),
                child: AnimatedSwitcher(
                  duration: KMotion.focus,
                  child: Column(
                    key: ValueKey(item.id),
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(item.title, style: theme.textTheme.displayMedium, maxLines: 2, overflow: TextOverflow.ellipsis),
                      const SizedBox(height: 6),
                      Text(meta, style: theme.textTheme.labelMedium),
                      if (item.overview != null) ...[
                        const SizedBox(height: 8),
                        Text(item.overview!, maxLines: d.isTv ? 2 : 3, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodyMedium?.copyWith(color: KColors.textSecondary)),
                      ],
                    ],
                  ),
                ),
              ),
              if (!d.isTv) ...[
                const SizedBox(height: 16),
                Row(children: [
                  KButton(
                    label: item.kind == ItemKind.show ? 'Episodes' : 'Play',
                    icon: item.kind == ItemKind.show ? Icons.list : Icons.play_arrow_rounded,
                    primary: true,
                    onPressed: () => item.kind == ItemKind.show ? openItem(context, item) : playVideo(context, item),
                  ),
                  const SizedBox(width: 12),
                  KButton(label: 'Details', icon: Icons.info_outline, onPressed: () => openItem(context, item)),
                ]),
              ],
            ],
          ),
        ),
      ]),
    );
  }
}
