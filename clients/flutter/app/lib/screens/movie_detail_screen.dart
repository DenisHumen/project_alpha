// SPDX-License-Identifier: MPL-2.0
//
// Movie Detail (UI-009, 01 §2.2): a full-bleed backdrop with a scrim, the
// clearlogo (else the title), metadata, technical badges, external ratings,
// Play/Resume with the version picker (LIB-010), trailer, favorite, then
// cast and crew, extras, the collection and the technical information of
// the file. Initial focus is on Play/Resume.
//
// Details come from libraries that implement [LibraryDetails] (the Rust
// core and the demo library); with the v0 library the page shows what the
// item row has (title, year, runtime, overview, file) and still plays.

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../sources/source_text.dart' show formatAgo;
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/artwork_image.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'detail_layout.dart' show MatchBadge;
import 'detail_sections.dart';
import 'fix_match_dialog.dart';
import 'library_lists_ui.dart';
import 'playback.dart';

/// Key of the Play/Resume button (tests: initial focus).
const detailPlayKey = ValueKey('detail-play');

class MovieDetailScreen extends StatefulWidget {
  const MovieDetailScreen({super.key, required this.item, this.resumeDevice});

  final LibraryItem item;

  /// Name of the device the saved position comes from ("iPhone"), when
  /// sync knows it (UI-019 / SYNC). Hook: progress does not carry the
  /// device yet, so the line under Resume shows the time only.
  final String? resumeDevice;

  @override
  State<MovieDetailScreen> createState() => _MovieDetailScreenState();
}

class _MovieDetailScreenState extends State<MovieDetailScreen> {
  Stream<List<(LibraryItem, Progress)>>? _cw;
  LibraryItem get _item => widget.item;
  ItemDetails _details = ItemDetails.empty;
  List<MediaVersion> _versions = const [];
  bool? _favorite;
  (MediaCollection, List<LibraryItem>)? _collection;

  /// The library knows details (not v0) and they were read.
  bool _probes = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_cw != null) return;
    _cw = Services.of(context).library.continueWatching();
    unawaited(_load());
  }

  Future<void> _load() async {
    final library = Services.read(context).library;
    final id = _item.id;
    ItemDetails details = ItemDetails.empty;
    if (library case final LibraryDetails d) {
      try {
        details = await d.details(id);
      } catch (e) {
        debugPrint('Details of $id not read: $e');
      }
    }
    var versions = const <MediaVersion>[];
    bool? favorite;
    (MediaCollection, List<LibraryItem>)? collection;
    if (library case final LibraryLists lists) {
      try {
        versions = await lists.versions(id);
        favorite = await lists.isFavorite(id);
        final cols = await lists.itemCollections(id);
        final col =
            cols.where((c) => c.external).firstOrNull ?? cols.firstOrNull;
        if (col != null) {
          final items = await lists.collectionItems(col.id);
          if (items.length > 1) collection = (col, items);
        }
      } catch (e) {
        debugPrint('Lists of $id not read: $e');
      }
    }
    if (!mounted) return;
    setState(() {
      _details = details;
      _versions = versions;
      _favorite = favorite;
      _collection = collection;
      _probes = library is LibraryDetails;
    });
  }

  MediaVersion? get _defaultVersion =>
      _versions.where((v) => v.isDefault).firstOrNull;

  Future<void> _toggleFavorite(LibraryLists lists) async {
    final on = !(_favorite ?? false);
    await setFavorite(context, lists, _item, on);
    if (mounted) setState(() => _favorite = on);
  }

  void _openCollectionItem(LibraryItem item) {
    Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => MovieDetailScreen(item: item)),
    );
  }

  @override
  Widget build(BuildContext context) {
    return StreamBuilder<List<(LibraryItem, Progress)>>(
      stream: _cw,
      builder: (context, snap) {
        Progress? progress;
        for (final (i, p) in snap.data ?? const <(LibraryItem, Progress)>[]) {
          if (i.id == _item.id) progress = p;
        }
        return _page(context, progress);
      },
    );
  }

  Widget _page(BuildContext context, Progress? progress) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    final item = _item;
    final details = _details;
    final lists = libraryListsOf(context);
    final screen = MediaQuery.sizeOf(context);
    final heroHeight = screen.height * (d.isPhone ? .7 : (d.isTv ? .86 : .8));

    final trailer = details.extras
        .where(
          (e) =>
              e.kind == MediaExtraKind.trailer ||
              e.kind == MediaExtraKind.teaser,
        )
        .firstOrNull;
    final meta = [
      if (item.year != null) '${item.year}',
      formatRuntime(l, details.runtimeMs ?? item.durationMs),
      if (details.genres.isNotEmpty) details.genres.take(3).join(', '),
      if (progress != null) formatRemaining(l, progress),
    ].where((s) => s.isNotEmpty).join('  ·  ');
    final badges = techBadges(details.tech, _defaultVersion);

    final actions = <Widget>[
      KButton(
        key: detailPlayKey,
        autofocus: !d.isPhone,
        primary: true,
        icon: Icons.play_arrow_rounded,
        label: progress != null
            ? l.detailResumeAt(formatTimecode(progress.positionMs))
            : l.commonPlay,
        onPressed: () => playVideo(
          context,
          item,
          startPositionMs: progress?.positionMs ?? 0,
        ),
      ),
      if (progress != null)
        KButton(
          icon: Icons.replay,
          label: l.detailFromBeginning,
          onPressed: () => playVideo(context, item),
        ),
      if (trailer != null)
        KButton(
          key: const ValueKey('detail-trailer'),
          icon: Icons.movie_outlined,
          label: l.detailTrailer,
          onPressed: () => playExtra(context, item, trailer),
        ),
      if (lists != null && _favorite != null)
        KButton(
          key: const ValueKey('detail-favorite'),
          icon: _favorite! ? Icons.favorite : Icons.favorite_border,
          label: _favorite! ? l.detailFavoriteOn : l.detailFavorite,
          onPressed: () => _toggleFavorite(lists),
        ),
      KButton(
        icon: Icons.manage_search,
        label: l.commonFixMatch,
        onPressed: () => showFixMatchDialog(context, item).then((_) {
          if (mounted) setState(() {});
        }),
      ),
    ];

    final resumeLine = progress == null
        ? null
        : (widget.resumeDevice == null
              ? l.detailResumeSource(
                  formatTimecode(progress.positionMs),
                  formatAgo(
                    l,
                    progress.updatedAt,
                    now: Services.read(context).clock(),
                  ),
                )
              : l.detailResumeSourceDevice(
                  formatTimecode(progress.positionMs),
                  widget.resumeDevice!,
                  formatAgo(
                    l,
                    progress.updatedAt,
                    now: Services.read(context).clock(),
                  ),
                ));

    final info = ConstrainedBox(
      constraints: BoxConstraints(maxWidth: d.isPhone ? double.infinity : 760),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          _Title(item: item, logoUrl: details.logoUrl),
          if (item.originalTitle != null &&
              item.originalTitle != item.title &&
              details.logoUrl == null)
            Text(item.originalTitle!, style: theme.textTheme.bodySmall),
          SizedBox(height: d.isTv ? 10 : 12),
          if (meta.isNotEmpty)
            Text(
              meta,
              key: const ValueKey('detail-meta'),
              style: theme.textTheme.labelMedium?.copyWith(
                fontSize: d.type.caption + 1,
              ),
            ),
          if (badges.isNotEmpty || details.ratings.isNotEmpty) ...[
            const SizedBox(height: 10),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                for (final b in badges) TechBadge(b),
                if (details.ratings.isNotEmpty) ...[
                  if (badges.isNotEmpty) const SizedBox(width: 6),
                  RatingsRow(ratings: details.ratings),
                ],
              ],
            ),
          ],
          SizedBox(height: d.isTv ? 18 : 20),
          FocusTraversalGroup(
            child: Wrap(spacing: 12, runSpacing: 12, children: actions),
          ),
          if (resumeLine != null) ...[
            const SizedBox(height: 10),
            Text(
              resumeLine,
              key: const ValueKey('detail-resume-source'),
              style: theme.textTheme.labelSmall?.copyWith(
                color: KColors.textSecondary,
              ),
            ),
          ],
          if (item.matchConfidence != null) ...[
            const SizedBox(height: 8),
            MatchBadge(confidence: item.matchConfidence),
          ],
          if (item.overview != null) ...[
            SizedBox(height: d.isTv ? 14 : 18),
            Text(
              item.overview!,
              maxLines: d.isTv ? 3 : (d.isPhone ? 6 : 4),
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodyLarge?.copyWith(
                color: KColors.textSecondary,
              ),
            ),
          ],
        ],
      ),
    );

    final hero = Stack(
      children: [
        Positioned(
          top: 0,
          left: 0,
          right: 0,
          height: heroHeight,
          child: _Backdrop(item: item),
        ),
        ConstrainedBox(
          constraints: BoxConstraints(minHeight: heroHeight),
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              d.pageInsets.top + (d.isTv ? 0 : 56),
              d.pageInsets.right,
              d.isTv ? 8 : 12,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              mainAxisAlignment: MainAxisAlignment.end,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [info],
            ),
          ),
        ),
        if (!d.isTv && Navigator.of(context).canPop())
          Positioned(
            left: d.pageInsets.left - 8,
            top: 0,
            child: SafeArea(
              bottom: false,
              child: Padding(
                padding: const EdgeInsets.only(top: 8),
                child: IconButton(
                  tooltip: l.commonBack,
                  onPressed: () => Navigator.of(context).maybePop(),
                  style: IconButton.styleFrom(backgroundColor: Colors.black38),
                  icon: const Icon(Icons.arrow_back),
                ),
              ),
            ),
          ),
      ],
    );

    final cast = details.credits;
    final collection = _collection;
    return Scaffold(
      backgroundColor: KColors.bgBase,
      body: CustomScrollView(
        slivers: [
          SliverToBoxAdapter(child: hero),
          if (cast.isNotEmpty)
            SliverToBoxAdapter(
              child: DetailSection(
                title: l.detailCastCrew,
                // People pages are P1 (UI-009): no onPerson yet.
                child: CreditsRow(credits: cast),
              ),
            ),
          if (details.extras.isNotEmpty)
            SliverToBoxAdapter(
              child: DetailSection(
                title: l.detailExtras,
                child: ExtrasRow(item: item, extras: details.extras),
              ),
            ),
          if (collection != null)
            SliverToBoxAdapter(
              child: DetailSection(
                title: l.detailCollection(collection.$1.name),
                child: CollectionRow(
                  items: collection.$2,
                  current: item.id,
                  onOpen: _openCollectionItem,
                ),
              ),
            ),
          SliverToBoxAdapter(
            child: DetailSection(
              title: l.detailTechInfo,
              child: TechInfoSection(
                item: item,
                tech: details.tech,
                version: _defaultVersion,
                notProbedHint: _probes && details.tech == null,
              ),
            ),
          ),
          ),
          SliverToBoxAdapter(child: SizedBox(height: d.pageInsets.bottom + 24)),
        ],
      ),
    );
  }
}

/// Backdrop with scrims from the left and the bottom (01 §4.5
/// scrim/gradient), fading into the page background.
class _Backdrop extends StatelessWidget {
  const _Backdrop({required this.item});
  final LibraryItem item;

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: [
        Artwork(url: item.backdropUrl, seed: item.id, style: ArtStyle.backdrop),
        const DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.centerLeft,
              end: Alignment.centerRight,
              colors: [Color(0xE60B0B0F), Color(0x8C0B0B0F), Color(0x100B0B0F)],
              stops: [0, .5, 1],
            ),
          ),
        ),
        DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [
                const Color(0x660B0B0F),
                const Color(0x000B0B0F),
                KColors.bgBase.withValues(alpha: .85),
                KColors.bgBase,
              ],
              stops: const [0, .3, .78, 1],
            ),
          ),
        ),
      ],
    );
  }
}

/// The clearlogo, else the title in the display style.
class _Title extends StatelessWidget {
  const _Title({required this.item, this.logoUrl});
  final LibraryItem item;
  final String? logoUrl;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final text = Text(
      item.title,
      key: const ValueKey('detail-title'),
      style: Theme.of(context).textTheme.displayLarge,
      maxLines: 2,
      overflow: TextOverflow.ellipsis,
    );
    final url = logoUrl;
    if (url == null || url.startsWith(kDemoArtScheme)) return text;
    final h = d.isTv ? 120.0 : (d.isPhone ? 80.0 : 110.0);
    final w = d.isPhone ? 280.0 : 440.0;
    return Semantics(
      label: item.title,
      header: true,
      child: ConstrainedBox(
        key: const ValueKey('detail-logo'),
        constraints: BoxConstraints(maxHeight: h, maxWidth: w),
        child: Image(
          image: artworkProvider(url, width: decodeWidth(context, w)),
          fit: BoxFit.contain,
          alignment: Alignment.centerLeft,
          excludeFromSemantics: true,
          errorBuilder: (_, _, _) => text,
        ),
      ),
    );
  }
}
