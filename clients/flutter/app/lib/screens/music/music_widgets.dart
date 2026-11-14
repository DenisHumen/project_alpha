// SPDX-License-Identifier: MPL-2.0
//
// Building blocks of the music screens (MUS-004): track rows, the format
// badge, artist tiles, rows and grids of albums, and the page header shared
// by the album, artist and list pages.

import 'package:material_ui/material_ui.dart';

import '../../integration/library.dart';
import '../../l10n/l10n.dart';
import '../../services/app_services.dart';
import '../../theme/kinolune_theme.dart';
import '../../widgets/artwork.dart';
import '../../widgets/cards.dart';
import '../../widgets/focusable.dart';
import '../../widgets/format.dart';

/// Album title, or "Unknown album".
String albumTitle(AppLocalizations l, MusicAlbum a) =>
    a.title ?? l.musicUnknownAlbum;

/// Album artist, or "Unknown artist".
String albumArtist(AppLocalizations l, MusicAlbum a) =>
    a.artist ?? l.musicUnknownArtist;

/// A row of a track list: number (or the art, or "playing"), title, artist,
/// format badge and duration.
class TrackTile extends StatelessWidget {
  const TrackTile({
    super.key,
    required this.track,
    required this.onTap,
    this.showArt = false,
    this.number,
    this.subtitle,
    this.badge,
    this.onContextMenu,
  });

  final LibraryItem track;
  final VoidCallback onTap;
  final bool showArt;

  /// Shown instead of the track number (e.g. the rank in "Popular").
  final int? number;

  /// Instead of artist · album.
  final String? subtitle;

  /// Format badge (`FLAC 24/96`).
  final String? badge;

  /// Long press or right click: the track menu.
  final ValueChanged<Offset>? onContextMenu;

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
          onContextMenu: onContextMenu,
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
                          '${number ?? track.trackNumber ?? ''}',
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
                        subtitle ??
                            [
                              track.artist,
                              if (showArt) track.album,
                            ].whereType<String>().join(' · '),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
                if (badge != null) ...[
                  const SizedBox(width: 8),
                  FormatBadge(badge!, compact: true),
                ],
                const SizedBox(width: 12),
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

/// The stream format of an album or track (`FLAC 24/96`), outlined; Hi-Res
/// formats in the accent colour.
class FormatBadge extends StatelessWidget {
  const FormatBadge(
    this.label, {
    super.key,
    this.hiRes = false,
    this.compact = false,
  });

  final String label;
  final bool hiRes;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final color = hiRes ? KColors.accentSoft : KColors.textSecondary;
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: compact ? 5 : 8,
        vertical: compact ? 1 : 3,
      ),
      decoration: BoxDecoration(
        border: Border.all(color: color.withValues(alpha: .7)),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(
        label,
        maxLines: 1,
        style: Theme.of(context).textTheme.labelSmall
            ?.copyWith(color: color, letterSpacing: .4),
      ),
    );
  }
}

/// A round artist tile with the name under it.
class ArtistCard extends StatelessWidget {
  const ArtistCard({
    super.key,
    required this.artist,
    required this.width,
    required this.onTap,
  });

  final MusicArtist artist;
  final double width;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    return SizedBox(
      width: width,
      child: Column(
        children: [
          KFocusable(
            onActivate: onTap,
            focusScale: d.focusScalePoster,
            radius: 999,
            semanticLabel: artist.name,
            builder: (context, _) => ClipOval(
              child: AspectRatio(
                aspectRatio: 1,
                child: Artwork(
                  url: artist.artUrl,
                  seed: artist.artSeed,
                  style: ArtStyle.album,
                ),
              ),
            ),
          ),
          SizedBox(height: d.isTv ? 10 : 8),
          Text(
            artist.name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.center,
            style: theme.textTheme.titleSmall,
          ),
          Text(
            context.l10n.musicAlbumCount(artist.albums.length),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: theme.textTheme.labelSmall,
          ),
        ],
      ),
    );
  }
}

/// Height of a [MediaRow] of square cards.
double albumRowHeight(KDensity d) =>
    d.albumWidth + cardCaptionHeight(d) + (d.isTv ? 12 : 6);

/// A row of albums.
class AlbumRow extends StatelessWidget {
  const AlbumRow({
    super.key,
    required this.title,
    required this.albums,
    required this.onOpen,
  });

  final String title;
  final List<MusicAlbum> albums;
  final ValueChanged<MusicAlbum> onOpen;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    return MediaRow(
      title: title,
      itemCount: albums.length,
      height: albumRowHeight(d),
      itemBuilder: (context, i) => AlbumCard(
        width: d.albumWidth,
        seed: albums[i].artSeed,
        artUrl: albums[i].artUrl,
        title: albumTitle(l, albums[i]),
        subtitle: albumArtist(l, albums[i]),
        onTap: () => onOpen(albums[i]),
      ),
    );
  }
}

/// A grid of albums as a sliver.
class AlbumGrid extends StatelessWidget {
  const AlbumGrid({super.key, required this.albums, required this.onOpen});

  final List<MusicAlbum> albums;
  final ValueChanged<MusicAlbum> onOpen;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    return SliverGrid(
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
            title: albumTitle(l, albums[i]),
            subtitle: [
              albumArtist(l, albums[i]),
              if (albums[i].year != null) '${albums[i].year}',
            ].join(' · '),
            onTap: () => onOpen(albums[i]),
          ),
        ),
        childCount: albums.length,
      ),
    );
  }
}

/// Header of the album, artist and list pages: art, overline, title, a
/// line of facts, badges and buttons; a column on phones.
class MusicPageHeader extends StatelessWidget {
  const MusicPageHeader({
    super.key,
    required this.art,
    required this.overline,
    required this.title,
    required this.facts,
    required this.buttons,
    this.badges = const [],
  });

  final Widget art;
  final String overline;
  final String title;
  final String facts;
  final List<Widget> badges;
  final List<Widget> buttons;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final size = d.isPhone ? 180.0 : (d.isTv ? 200.0 : 240.0);
    final align = d.isPhone
        ? CrossAxisAlignment.center
        : CrossAxisAlignment.start;
    final text = Column(
      crossAxisAlignment: align,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(overline, style: theme.textTheme.labelSmall),
        Text(
          title,
          style: theme.textTheme.displayMedium,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          textAlign: d.isPhone ? TextAlign.center : TextAlign.start,
        ),
        const SizedBox(height: 6),
        Text(
          facts,
          style: theme.textTheme.labelMedium,
          textAlign: d.isPhone ? TextAlign.center : TextAlign.start,
        ),
        if (badges.isNotEmpty) ...[
          const SizedBox(height: 10),
          Wrap(spacing: 8, runSpacing: 8, children: badges),
        ],
        const SizedBox(height: 18),
        Wrap(spacing: 12, runSpacing: 12, children: buttons),
      ],
    );
    final artBox = ClipRRect(
      borderRadius: BorderRadius.circular(d.cardRadius),
      child: SizedBox.square(dimension: size, child: art),
    );
    if (d.isPhone) {
      return Column(children: [artBox, const SizedBox(height: 20), text]);
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        artBox,
        const SizedBox(width: 28),
        Flexible(child: text),
      ],
    );
  }
}

/// A scaffold of a pushed music page: back button and a scrolling list of
/// slivers.
class MusicPage extends StatelessWidget {
  const MusicPage({super.key, required this.slivers});

  final List<Widget> slivers;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    return Scaffold(
      body: SafeArea(
        child: CustomScrollView(
          clipBehavior: Clip.none,
          slivers: [
            SliverPadding(
              padding: EdgeInsets.fromLTRB(
                d.pageInsets.left,
                d.pageInsets.top,
                d.pageInsets.right,
                0,
              ),
              sliver: SliverToBoxAdapter(
                child: d.isTv
                    ? const SizedBox.shrink()
                    : Align(
                        alignment: Alignment.centerLeft,
                        child: IconButton(
                          tooltip: l.commonBack,
                          onPressed: () => Navigator.of(context).maybePop(),
                          icon: const Icon(Icons.arrow_back),
                        ),
                      ),
              ),
            ),
            ...slivers,
            SliverToBoxAdapter(
              child: SizedBox(height: d.pageInsets.bottom + 90),
            ),
          ],
        ),
      ),
    );
  }
}

/// Horizontal page padding for slivers.
EdgeInsets musicPagePadding(KDensity d, {double top = 0, double bottom = 0}) =>
    EdgeInsets.fromLTRB(d.pageInsets.left, top, d.pageInsets.right, bottom);

/// A section title inside a page.
class MusicSectionTitle extends StatelessWidget {
  const MusicSectionTitle(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 24, bottom: 8),
    child: Text(text, style: Theme.of(context).textTheme.titleLarge),
  );
}
