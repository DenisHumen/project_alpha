// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';

/// Shared detail-page composition (01 §2.2): full-bleed backdrop darkened from
/// the left and bottom, poster, title, metadata, overview and actions.
class DetailBackdropPage extends StatelessWidget {
  const DetailBackdropPage({
    super.key,
    required this.item,
    required this.meta,
    required this.actions,
    this.extra,
    this.footer,
  });

  final LibraryItem item;
  final String meta;
  final List<Widget> actions;
  final Widget? extra;

  /// Content below the hero (episodes list, file info…).
  final Widget? footer;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final screen = MediaQuery.sizeOf(context);
    final heroHeight = d.isPhone
        ? screen.height * .62
        : screen.height * (d.isTv ? .8 : .78);
    final posterW = d.isPhone ? 110.0 : (d.isTv ? 150.0 : 190.0);

    final info = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          item.title,
          style: theme.textTheme.displayLarge,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
        ),
        if (item.originalTitle != null && item.originalTitle != item.title)
          Text(item.originalTitle!, style: theme.textTheme.bodySmall),
        const SizedBox(height: 8),
        Text(
          meta,
          style: theme.textTheme.labelMedium?.copyWith(
            fontSize: d.type.caption + 1,
          ),
        ),
        if (item.overview != null) ...[
          const SizedBox(height: 12),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 620),
            child: Text(
              item.overview!,
              maxLines: d.isTv ? 3 : 5,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodyLarge?.copyWith(
                color: KColors.textSecondary,
              ),
            ),
          ),
        ],
        SizedBox(height: d.isTv ? 18 : 22),
        FocusTraversalGroup(
          child: Wrap(spacing: 12, runSpacing: 12, children: actions),
        ),
        ?extra,
      ],
    );

    return Scaffold(
      backgroundColor: KColors.bgBase,
      body: CustomScrollView(
        slivers: [
          SliverToBoxAdapter(
            child: SizedBox(
              height: heroHeight,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  Artwork(
                    url: item.backdropUrl,
                    seed: item.showId ?? item.id,
                    style: ArtStyle.backdrop,
                  ),
                  const DecoratedBox(
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.centerLeft,
                        end: Alignment.centerRight,
                        colors: [
                          Color(0xF00B0B0F),
                          Color(0x990B0B0F),
                          Color(0x200B0B0F),
                        ],
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
                          Color(0x660B0B0F),
                          Color(0x000B0B0F),
                          KColors.bgBase,
                        ],
                        stops: [0, .35, 1],
                      ),
                    ),
                  ),
                  SafeArea(
                    bottom: false,
                    child: Padding(
                      padding: EdgeInsets.fromLTRB(
                        d.pageInsets.left,
                        d.pageInsets.top,
                        d.pageInsets.right,
                        d.isTv ? 24 : 32,
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          if (!d.isTv && Navigator.of(context).canPop())
                            IconButton(
                              tooltip: context.l10n.commonBack,
                              onPressed: () => Navigator.of(context).maybePop(),
                              style: IconButton.styleFrom(
                                backgroundColor: Colors.black38,
                              ),
                              icon: const Icon(Icons.arrow_back),
                            ),
                          const Spacer(),
                          if (d.isPhone)
                            info
                          else
                            Row(
                              crossAxisAlignment: CrossAxisAlignment.end,
                              children: [
                                ClipRRect(
                                  borderRadius: BorderRadius.circular(
                                    d.cardRadius,
                                  ),
                                  child: SizedBox(
                                    width: posterW,
                                    child: AspectRatio(
                                      aspectRatio: 2 / 3,
                                      child: Artwork(
                                        url: item.posterUrl,
                                        seed: item.id,
                                        style: ArtStyle.poster,
                                        title: item.title,
                                        subtitle: item.year?.toString(),
                                      ),
                                    ),
                                  ),
                                ),
                                SizedBox(width: d.isTv ? 28 : 32),
                                Expanded(child: info),
                              ],
                            ),
                        ],
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (footer != null) SliverToBoxAdapter(child: footer),
          SliverToBoxAdapter(child: SizedBox(height: d.pageInsets.bottom + 24)),
        ],
      ),
    );
  }
}

class MatchBadge extends StatelessWidget {
  const MatchBadge({super.key, required this.confidence});
  final double? confidence;

  @override
  Widget build(BuildContext context) {
    final c = confidence;
    if (c == null) return const SizedBox.shrink();
    final low = c < .75;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          low ? Icons.help_outline : Icons.verified_outlined,
          size: 14,
          color: low ? Colors.amber : KColors.textSecondary,
        ),
        const SizedBox(width: 4),
        Text(
          low
              ? context.l10n.detailMatchUncertain(c)
              : context.l10n.detailMatched(c),
          style: Theme.of(context).textTheme.labelSmall
              ?.copyWith(color: low ? Colors.amber : null),
        ),
      ],
    );
  }
}
