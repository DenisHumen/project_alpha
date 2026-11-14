// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'library_grid_screen.dart' show FilterPill;
import 'playback.dart';

class MusicScreen extends StatefulWidget {
  const MusicScreen({super.key});

  @override
  State<MusicScreen> createState() => _MusicScreenState();
}

class _MusicScreenState extends State<MusicScreen> {
  Stream<List<LibraryItem>>? _tracks;
  bool _albums = true;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _tracks ??= Services.of(context).library.tracks();
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return StreamBuilder<List<LibraryItem>>(
      stream: _tracks,
      builder: (context, snap) {
        final tracks = snap.data ?? const <LibraryItem>[];
        final l = context.l10n;
        final albums = groupAlbums(l, tracks);
        return CustomScrollView(
          clipBehavior: Clip.none,
          slivers: [
            SliverToBoxAdapter(
              child: PageTitle(
                l.navMusic,
                trailing: tracks.isEmpty
                    ? null
                    : KButton(
                        label: l.musicShuffleAll,
                        icon: Icons.shuffle,
                        onPressed: () {
                          final list = [...tracks]..shuffle();
                          playTracks(context, list, 0);
                        },
                      ),
              ),
            ),
            SliverToBoxAdapter(
              child: Padding(
                padding: EdgeInsets.fromLTRB(
                  d.pageInsets.left,
                  0,
                  d.pageInsets.right,
                  20,
                ),
                child: FocusTraversalGroup(
                  child: Row(
                    children: [
                      FilterPill(
                        label: l.musicAlbumsTab(albums.length),
                        selected: _albums,
                        onTap: () => setState(() => _albums = true),
                      ),
                      const SizedBox(width: 8),
                      FilterPill(
                        label: l.musicTracksTab(tracks.length),
                        selected: !_albums,
                        onTap: () => setState(() => _albums = false),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            if (tracks.isEmpty)
              SliverFillRemaining(
                hasScrollBody: false,
                child: EmptyState(
                  icon: Icons.library_music_outlined,
                  title: l.musicEmptyTitle,
                  message: l.musicEmptyMessage,
                ),
              )
            else if (_albums)
              SliverPadding(
                padding: EdgeInsets.fromLTRB(
                  d.pageInsets.left,
                  0,
                  d.pageInsets.right,
                  d.pageInsets.bottom + 90,
                ),
                sliver: SliverGrid(
                  gridDelegate: CardGridDelegate(
                    minItemWidth: d.albumWidth,
                    mainSpacing: 18,
                    crossSpacing: d.gap,
                    artAspect: 1,
                    extraHeight: cardCaptionHeight(d),
                  ),
                  delegate: SliverChildBuilderDelegate(
                    (context, i) => LayoutBuilder(
                      builder: (context, c) => AlbumCard(
                        width: c.maxWidth,
                        seed: albums[i].artSeed,
                        artUrl: albums[i].artUrl,
                        title: albums[i].title,
                        subtitle: albums[i].artist,
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => AlbumScreen(album: albums[i]),
                          ),
                        ),
                      ),
                    ),
                    childCount: albums.length,
                  ),
                ),
              )
            else
              SliverPadding(
                padding: EdgeInsets.fromLTRB(
                  d.pageInsets.left,
                  0,
                  d.pageInsets.right,
                  d.pageInsets.bottom + 90,
                ),
                sliver: SliverList.builder(
                  itemCount: tracks.length,
                  itemBuilder: (context, i) => TrackTile(
                    track: tracks[i],
                    showArt: true,
                    onTap: () => playTracks(context, tracks, i),
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}

class AlbumScreen extends StatelessWidget {
  const AlbumScreen({super.key, required this.album});

  final Album album;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final l = context.l10n;
    final art = d.isPhone ? 180.0 : (d.isTv ? 200.0 : 240.0);
    final total = album.tracks.fold<int>(0, (s, t) => s + (t.durationMs ?? 0));
    final header = [
      ClipRRect(
        borderRadius: BorderRadius.circular(d.cardRadius),
        child: SizedBox.square(
          dimension: art,
          child: Artwork(
            url: album.artUrl,
            seed: album.artSeed,
            style: ArtStyle.album,
          ),
        ),
      ),
      SizedBox(width: 28, height: 20),
      // On phones the header is a Column inside a ListView: no flex there.
      _flexibleUnlessPhone(
        d.isPhone,
        Column(
          crossAxisAlignment: d.isPhone
              ? CrossAxisAlignment.center
              : CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(l.musicAlbumLabel, style: theme.textTheme.labelSmall),
            Text(
              album.title,
              style: theme.textTheme.displayMedium,
              textAlign: d.isPhone ? TextAlign.center : TextAlign.start,
            ),
            const SizedBox(height: 6),
            Text(
              [
                album.artist,
                if (album.year != null) '${album.year}',
                l.musicTrackCount(album.tracks.length),
                formatRuntime(l, total),
              ].join('  ·  '),
              style: theme.textTheme.labelMedium,
            ),
            const SizedBox(height: 18),
            Wrap(
              spacing: 12,
              runSpacing: 12,
              children: [
                KButton(
                  label: l.musicPlay,
                  icon: Icons.play_arrow_rounded,
                  primary: true,
                  autofocus: !d.isPhone,
                  onPressed: () => playTracks(context, album.tracks, 0),
                ),
                KButton(
                  label: l.commonShuffle,
                  icon: Icons.shuffle,
                  onPressed: () =>
                      playTracks(context, [...album.tracks]..shuffle(), 0),
                ),
              ],
            ),
          ],
        ),
      ),
    ];
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: EdgeInsets.fromLTRB(
            d.pageInsets.left,
            d.pageInsets.top,
            d.pageInsets.right,
            d.pageInsets.bottom + 90,
          ),
          children: [
            if (!d.isTv)
              Align(
                alignment: Alignment.centerLeft,
                child: IconButton(
                  tooltip: l.commonBack,
                  onPressed: () => Navigator.of(context).maybePop(),
                  icon: const Icon(Icons.arrow_back),
                ),
              ),
            if (d.isPhone)
              Column(children: header)
            else
              Row(crossAxisAlignment: CrossAxisAlignment.end, children: header),
            const SizedBox(height: 24),
            for (var i = 0; i < album.tracks.length; i++)
              TrackTile(
                track: album.tracks[i],
                onTap: () => playTracks(context, album.tracks, i),
              ),
          ],
        ),
      ),
    );
  }
}

class TrackTile extends StatelessWidget {
  const TrackTile({
    super.key,
    required this.track,
    required this.onTap,
    this.showArt = false,
  });

  final LibraryItem track;
  final VoidCallback onTap;
  final bool showArt;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final music = Services.of(context).music;
    return ListenableBuilder(
      listenable: music,
      builder: (context, _) {
        final playing = music.current?.uri == track.playbackUri;
        return KFocusable(
          onActivate: onTap,
          focusScale: 1.0,
          light: false,
          semanticLabel: track.title,
          builder: (context, focused) => AnimatedContainer(
            duration: KMotion.focus,
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
            color: focused
                ? Colors.white.withValues(alpha: .09)
                : Colors.transparent,
            child: Row(
              children: [
                SizedBox(
                  width: showArt ? 44 : 28,
                  child: showArt
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(6),
                          child: SizedBox.square(
                            dimension: 40,
                            child: Artwork(
                              url: track.posterUrl,
                              seed: albumSeedFor(track),
                              style: ArtStyle.album,
                            ),
                          ),
                        )
                      : playing
                      ? const Icon(
                          Icons.graphic_eq,
                          color: KColors.accent,
                          size: 18,
                        )
                      : Text(
                          '${track.trackNumber ?? ''}',
                          style: theme.textTheme.labelMedium,
                        ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        track.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.titleMedium?.copyWith(
                          color: playing ? KColors.accentSoft : null,
                        ),
                      ),
                      Text(
                        [
                          track.artist,
                          if (showArt) track.album,
                        ].whereType<String>().join(' · '),
                        maxLines: 1,
                        style: theme.textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
                Text(
                  track.durationMs == null
                      ? ''
                      : formatClock(track.durationMs!),
                  style: theme.textTheme.labelSmall,
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}

Widget _flexibleUnlessPhone(bool phone, Widget child) =>
    phone ? child : Flexible(child: child);
