// SPDX-License-Identifier: MPL-2.0
//
// Home (UI-007): hero, the shared Continue row, Up Next, recently added,
// smart shelves pinned to Home, genre rows, music and sources. Each row
// follows its own library stream ([HomeFeed]), so a change (saved progress,
// a watcher rescan) updates that row in place.

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'home_feed.dart';
import 'playback.dart';
import 'routes.dart';
import 'sources_screen.dart';

class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, this.onOpenSection, this.continueSources});

  /// Switches the shell to a section (1 = Movies, 2 = Shows, 3 = Music).
  final ValueChanged<int>? onOpenSection;

  /// Extra Continue entries from outside the library (Twitch VODs, later).
  final List<ContinueSource> Function()? continueSources;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  final _featured = ValueNotifier<LibraryItem?>(null);
  final _debounce = Debouncer(const Duration(milliseconds: 200));
  Library? _lib;
  late HomeFeed _feed;
  Stream<List<LibraryItem>>? _movies, _shows, _tracks;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final lib = Services.of(context).library;
    if (!identical(lib, _lib)) {
      _lib = lib;
      _feed = HomeFeed(
        lib,
        continueSources: widget.continueSources?.call() ?? const [],
      );
      _movies = lib.movies();
      _shows = lib.shows();
      _tracks = lib.tracks();
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
    return StreamBuilder<List<LibraryItem>>(
      stream: _movies,
      builder: (context, moviesSnap) => StreamBuilder<List<LibraryItem>>(
        stream: _shows,
        builder: (context, showsSnap) => StreamBuilder<List<LibraryItem>>(
          stream: _tracks,
          builder: (context, tracksSnap) => _build(
            context,
            moviesSnap.data ?? const [],
            showsSnap.data ?? const [],
            tracksSnap.data ?? const [],
          ),
        ),
      ),
    );
  }

  Widget _build(
    BuildContext context,
    List<LibraryItem> movies,
    List<LibraryItem> shows,
    List<LibraryItem> tracks,
  ) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final albums = groupAlbums(l, tracks);
    if (movies.isEmpty && shows.isEmpty && albums.isEmpty) {
      return EmptyState(
        icon: Icons.video_library_outlined,
        title: l.homeEmptyTitle,
        message: l.homeEmptyMessage,
      );
    }
    final initial = movies.isNotEmpty
        ? movies.first
        : (shows.isNotEmpty ? shows.first : null);
    final feed = _feed;
    return ListView(
      padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 24),
      clipBehavior: Clip.none,
      children: [
        if (initial != null)
          ValueListenableBuilder<LibraryItem?>(
            valueListenable: _featured,
            builder: (context, f, _) => _Hero(item: f ?? initial),
          )
        else
          SizedBox(height: d.pageInsets.top),
        // TODO(SYNC): the handoff banner "Continue on this device" (01 §2.7)
        // goes here once SYNC delivers positions from other devices.
        _Watch<List<ContinueEntry>>(
          key: const ValueKey('home-continue'),
          identity: feed,
          create: feed.continueRow,
          builder: (context, cw) => _ContinueRow(
            entries: cw ?? const [],
            onFocus: (item, f) => _feature(item, f),
          ),
        ),
        _Watch<List<UpNextEntry>>(
          key: const ValueKey('home-up-next'),
          identity: feed,
          create: feed.upNext,
          builder: (context, next) => _UpNextRow(
            entries: next ?? const [],
            onFocus: (item, f) => _feature(item, f),
          ),
        ),
        // TODO(LIB): a Favorites row once the library exposes favorites.
        _Watch<List<HomeShelf>>(
          key: const ValueKey('home-shelves'),
          identity: feed,
          create: feed.shelves,
          builder: (context, shelves) => Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final s in shelves ?? const <HomeShelf>[])
                _PosterShelf(
                  key: ValueKey('home-shelf-${s.key}'),
                  title: switch (s.kind) {
                    HomeShelfKind.recentMovies => l.homeRecentMovies,
                    HomeShelfKind.recentShows => l.homeRecentShows,
                    HomeShelfKind.smart || HomeShelfKind.genre => s.name ?? '',
                  },
                  items: s.items,
                  onSeeAll: switch (s.kind) {
                    HomeShelfKind.recentMovies => () =>
                      widget.onOpenSection?.call(1),
                    HomeShelfKind.recentShows => () =>
                      widget.onOpenSection?.call(2),
                    _ => null,
                  },
                  onFocus: _feature,
                ),
            ],
          ),
        ),
        if (albums.isNotEmpty) ...[
          MediaRow(
            title: l.navMusic,
            itemCount: albums.length,
            height: d.albumWidth + cardCaptionHeight(d) + (d.isTv ? 12 : 6),
            onSeeAll: () => widget.onOpenSection?.call(3),
            itemBuilder: (context, i) => AlbumCard(
              width: d.albumWidth,
              seed: albums[i].artSeed,
              artUrl: albums[i].artUrl,
              title: albums[i].title,
              subtitle: albums[i].artist,
              onTap: () => playTracks(context, albums[i].tracks, 0),
            ),
          ),
          SizedBox(height: d.rowGap),
        ],
        const _SourcesRow(key: ValueKey('home-sources')),
      ],
    );
  }
}

/// Listens to a stream made by [create] for as long as it is mounted (a new
/// one when [identity] changes), so a row can leave and re-enter the tree.
class _Watch<T> extends StatefulWidget {
  const _Watch({
    super.key,
    required this.identity,
    required this.create,
    required this.builder,
  });

  final Object identity;
  final Stream<T> Function() create;
  final Widget Function(BuildContext context, T? value) builder;

  @override
  State<_Watch<T>> createState() => _WatchState<T>();
}

class _WatchState<T> extends State<_Watch<T>> {
  late Stream<T> _stream = widget.create();

  @override
  void didUpdateWidget(_Watch<T> old) {
    super.didUpdateWidget(old);
    if (!identical(old.identity, widget.identity)) _stream = widget.create();
  }

  @override
  Widget build(BuildContext context) => StreamBuilder<T>(
    stream: _stream,
    builder: (context, snap) => widget.builder(context, snap.data),
  );
}

double _landscapeRowHeight(KDensity d) =>
    d.landscapeWidth * 9 / 16 + cardCaptionHeight(d) + (d.isTv ? 12 : 6);

String _kindBadge(AppLocalizations l, ContinueKind k) => switch (k) {
  ContinueKind.movie => l.homeBadgeMovie,
  ContinueKind.episode => l.homeBadgeEpisode,
  ContinueKind.twitchVod => l.homeBadgeTwitchVod,
};

/// The shared Continue row: movies and episodes (later Twitch VODs) with a
/// type badge, most recently played first.
class _ContinueRow extends StatelessWidget {
  const _ContinueRow({required this.entries, required this.onFocus});

  final List<ContinueEntry> entries;
  final void Function(LibraryItem item, bool focused) onFocus;

  @override
  Widget build(BuildContext context) {
    if (entries.isEmpty) return const SizedBox.shrink();
    final d = KDensity.of(context);
    final l = context.l10n;
    return Padding(
      padding: EdgeInsets.only(bottom: d.rowGap),
      child: MediaRow(
        title: l.homeContinueWatching,
        itemCount: entries.length,
        height: _landscapeRowHeight(d),
        itemBuilder: (context, i) {
          final e = entries[i];
          final item = e.item;
          final remaining = formatRemaining(l, e.progress);
          return LandscapeCard(
            key: ValueKey('continue-${e.id}'),
            width: d.landscapeWidth,
            seed: e.id,
            imageUrl: e.imageUrl,
            title: e.title,
            subtitle: switch (e.kind) {
              ContinueKind.episode when item != null =>
                '${episodeCode(l, item)} · $remaining',
              ContinueKind.twitchVod when e.subtitle != null =>
                '${e.subtitle} · $remaining',
              _ => remaining,
            },
            badge: _kindBadge(l, e.kind),
            progress: e.progress.fraction,
            onFocus: item == null ? null : (f) => onFocus(e.show ?? item, f),
            // TODO(TWI): open the VOD player for Twitch entries.
            onTap: item == null
                ? null
                : () => playVideo(
                    context,
                    item,
                    startPositionMs: e.progress.positionMs,
                  ),
          );
        },
      ),
    );
  }
}

/// Up Next: the next episode of shows whose last episode was finished.
class _UpNextRow extends StatelessWidget {
  const _UpNextRow({required this.entries, required this.onFocus});

  final List<UpNextEntry> entries;
  final void Function(LibraryItem item, bool focused) onFocus;

  @override
  Widget build(BuildContext context) {
    if (entries.isEmpty) return const SizedBox.shrink();
    final d = KDensity.of(context);
    final l = context.l10n;
    return Padding(
      padding: EdgeInsets.only(bottom: d.rowGap),
      child: MediaRow(
        title: l.homeUpNext,
        itemCount: entries.length,
        height: _landscapeRowHeight(d),
        itemBuilder: (context, i) {
          final e = entries[i];
          return LandscapeCard(
            key: ValueKey('up-next-${e.episode.id}'),
            width: d.landscapeWidth,
            seed: e.episode.id,
            imageUrl: e.episode.backdropUrl ?? e.show.backdropUrl,
            title: e.show.title,
            subtitle: '${episodeCode(l, e.episode)} · ${e.episode.title}',
            badge: l.homeBadgeEpisode,
            onFocus: (f) => onFocus(e.show, f),
            onTap: () => playVideo(context, e.episode),
          );
        },
      ),
    );
  }
}

/// A row of 2:3 posters.
class _PosterShelf extends StatelessWidget {
  const _PosterShelf({
    super.key,
    required this.title,
    required this.items,
    required this.onFocus,
    this.onSeeAll,
  });

  final String title;
  final List<LibraryItem> items;
  final VoidCallback? onSeeAll;
  final void Function(LibraryItem item, bool focused) onFocus;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Padding(
      padding: EdgeInsets.only(bottom: d.rowGap),
      child: MediaRow(
        title: title,
        itemCount: items.length,
        height: d.posterWidth * 1.5 + cardCaptionHeight(d) + (d.isTv ? 12 : 6),
        onSeeAll: onSeeAll,
        itemBuilder: (context, i) => PosterCard(
          item: items[i],
          width: d.posterWidth,
          onFocus: (f) => onFocus(items[i], f),
          onTap: () => openItem(context, items[i]),
        ),
      ),
    );
  }
}

void _openSources(BuildContext context) => Navigator.of(
  context,
).push(MaterialPageRoute<void>(builder: (_) => const SourcesScreen()));

/// Sources: a card per source; each opens the Sources screen.
class _SourcesRow extends StatelessWidget {
  const _SourcesRow({super.key});

  @override
  Widget build(BuildContext context) {
    final sources = Services.of(context).sources;
    if (sources.isEmpty) return const SizedBox.shrink();
    final d = KDensity.of(context);
    final l = context.l10n;
    final width = d.landscapeWidth;
    return MediaRow(
      title: l.homeSources,
      itemCount: sources.length,
      height: _SourceCard.height(d) + (d.isTv ? 12 : 6),
      onSeeAll: () => _openSources(context),
      itemBuilder: (context, i) => _SourceCard(
        key: ValueKey('home-source-${sources[i].id}'),
        source: sources[i],
        width: width,
      ),
    );
  }
}

class _SourceCard extends StatelessWidget {
  const _SourceCard({super.key, required this.source, required this.width});

  final MediaSource source;
  final double width;

  static double height(KDensity d) => d.isTv ? 64 : 72;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final smb = source.kind == SourceKind.smb;
    return SizedBox(
      width: width,
      height: height(d),
      child: KFocusable(
        onActivate: () => _openSources(context),
        semanticLabel: source.name,
        light: false,
        builder: (context, focused) => DecoratedBox(
          decoration: BoxDecoration(
            color: KColors.bgElevated,
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: KColors.stroke),
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Row(
              children: [
                Icon(
                  smb ? Icons.lan_outlined : Icons.folder_outlined,
                  color: KColors.textSecondary,
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        source.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleSmall,
                      ),
                      Text(
                        source.path,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Top shelf: backdrop of the featured (focused) title, crossfading.
class _Hero extends StatelessWidget {
  const _Hero({required this.item});

  final LibraryItem item;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final l = context.l10n;
    final height = d.isTv ? 250.0 : (d.isPhone ? 340.0 : 380.0);
    final meta = [
      if (item.year != null) '${item.year}',
      if (item.kind == ItemKind.show)
        l.homeKindShow
      else
        formatRuntime(l, item.durationMs),
    ].where((s) => s.isNotEmpty).join('  ·  ');
    return SizedBox(
      height: height,
      child: Stack(
        fit: StackFit.expand,
        children: [
          AnimatedSwitcher(
            duration: KMotion.crossfadeArt,
            child: Artwork(
              key: ValueKey(item.id),
              url: item.backdropUrl,
              seed: item.id,
              style: ArtStyle.backdrop,
            ),
          ),
          const DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.centerLeft,
                end: Alignment.centerRight,
                colors: [
                  Color(0xE60B0B0F),
                  Color(0x660B0B0F),
                  Color(0x000B0B0F),
                ],
                stops: [0, .45, .8],
              ),
            ),
          ),
          DecoratedBox(
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
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              d.pageInsets.top,
              d.pageInsets.right,
              d.isTv ? 14 : 24,
            ),
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
                        Text(
                          item.title,
                          style: theme.textTheme.displayMedium,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                        const SizedBox(height: 6),
                        Text(meta, style: theme.textTheme.labelMedium),
                        if (item.overview != null) ...[
                          const SizedBox(height: 8),
                          Text(
                            item.overview!,
                            maxLines: d.isTv ? 2 : 3,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodyMedium?.copyWith(
                              color: KColors.textSecondary,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
                if (!d.isTv) ...[
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      KButton(
                        label: item.kind == ItemKind.show
                            ? l.homeEpisodes
                            : l.commonPlay,
                        icon: item.kind == ItemKind.show
                            ? Icons.list
                            : Icons.play_arrow_rounded,
                        primary: true,
                        onPressed: () => item.kind == ItemKind.show
                            ? openItem(context, item)
                            : playVideo(context, item),
                      ),
                      const SizedBox(width: 12),
                      KButton(
                        label: l.homeDetails,
                        icon: Icons.info_outline,
                        onPressed: () => openItem(context, item),
                      ),
                    ],
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}
