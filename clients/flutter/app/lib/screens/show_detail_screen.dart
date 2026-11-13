// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'detail_layout.dart';
import 'fix_match_dialog.dart';
import 'playback.dart';

/// Show → seasons → episodes.
class ShowDetailScreen extends StatefulWidget {
  const ShowDetailScreen({super.key, required this.show, this.initialSeason});

  final LibraryItem show;
  final int? initialSeason;

  @override
  State<ShowDetailScreen> createState() => _ShowDetailScreenState();
}

class _ShowDetailScreenState extends State<ShowDetailScreen> {
  Stream<List<LibraryItem>>? _episodes;
  Stream<List<(LibraryItem, Progress)>>? _cw;
  int? _season;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final lib = Services.of(context).library;
    _episodes ??= lib.episodes(widget.show.id);
    _cw ??= lib.continueWatching();
    _season ??= widget.initialSeason;
  }

  @override
  Widget build(BuildContext context) {
    final show = widget.show;
    final d = KDensity.of(context);
    return StreamBuilder<List<LibraryItem>>(
      stream: _episodes,
      builder: (context, epSnap) => StreamBuilder<List<(LibraryItem, Progress)>>(
        stream: _cw,
        builder: (context, cwSnap) {
          final episodes = epSnap.data ?? const <LibraryItem>[];
          final progress = <String, Progress>{
            for (final (i, p)
                in cwSnap.data ?? const <(LibraryItem, Progress)>[])
              if (i.showId == show.id) i.id: p,
          };
          final seasons = {for (final e in episodes) e.season ?? 0}.toList()
            ..sort();
          final season = _season ?? (seasons.isEmpty ? 1 : seasons.first);
          final inSeason = episodes
              .where((e) => (e.season ?? 0) == season)
              .toList();

          // "Up next": the most recent in-progress episode, else the first one.
          LibraryItem? next;
          Progress? nextP;
          for (final e in episodes) {
            final p = progress[e.id];
            if (p != null &&
                (nextP == null || p.updatedAt.isAfter(nextP.updatedAt))) {
              next = e;
              nextP = p;
            }
          }
          next ??= episodes.isEmpty ? null : episodes.first;

          final meta = [
            if (show.year != null) '${show.year}',
            '${seasons.length} season${seasons.length == 1 ? '' : 's'}',
            '${episodes.length} episodes',
          ].join('  ·  ');

          return DetailBackdropPage(
            item: show,
            meta: meta,
            actions: [
              if (next != null)
                KButton(
                  autofocus: !d.isPhone,
                  primary: true,
                  icon: Icons.play_arrow_rounded,
                  label: nextP != null
                      ? 'Resume ${episodeCode(next)}'
                      : 'Play ${episodeCode(next)}',
                  onPressed: () => playVideo(
                    context,
                    next!,
                    startPositionMs: nextP?.positionMs ?? 0,
                  ),
                ),
              KButton(
                icon: Icons.manage_search,
                label: 'Fix match',
                onPressed: () => showFixMatchDialog(context, show),
              ),
            ],
            footer: episodes.isEmpty
                ? null
                : Padding(
                    padding: EdgeInsets.only(
                      left: d.pageInsets.left,
                      right: d.pageInsets.right,
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        FocusTraversalGroup(
                          child: Wrap(
                            spacing: 10,
                            runSpacing: 10,
                            children: [
                              for (final s in seasons)
                                _SeasonChip(
                                  label: s == 0 ? 'Specials' : 'Season $s',
                                  selected: s == season,
                                  onTap: () => setState(() => _season = s),
                                ),
                            ],
                          ),
                        ),
                        SizedBox(height: d.isTv ? 16 : 20),
                        FocusTraversalGroup(
                          child: Column(
                            children: [
                              for (final e in inSeason)
                                _EpisodeTile(
                                  episode: e,
                                  progress: progress[e.id],
                                  onTap: () => playVideo(
                                    context,
                                    e,
                                    startPositionMs:
                                        progress[e.id]?.positionMs ?? 0,
                                  ),
                                ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
          );
        },
      ),
    );
  }
}

class _SeasonChip extends StatelessWidget {
  const _SeasonChip({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return KFocusable(
      onActivate: onTap,
      radius: 999,
      light: false,
      focusScale: d.focusScaleButton,
      builder: (context, focused) {
        final on = focused || selected;
        return AnimatedContainer(
          duration: KMotion.focus,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          decoration: BoxDecoration(
            color: focused
                ? Colors.white
                : (selected
                      ? Colors.white.withValues(alpha: .22)
                      : Colors.white.withValues(alpha: .07)),
            borderRadius: BorderRadius.circular(999),
          ),
          child: Text(
            label,
            style: Theme.of(context).textTheme.labelLarge?.copyWith(
              color: focused
                  ? Colors.black
                  : (on ? Colors.white : KColors.textSecondary),
            ),
          ),
        );
      },
    );
  }
}

class _EpisodeTile extends StatelessWidget {
  const _EpisodeTile({
    required this.episode,
    required this.onTap,
    this.progress,
  });

  final LibraryItem episode;
  final Progress? progress;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final thumbW = d.isPhone ? 140.0 : (d.isTv ? 190.0 : 220.0);
    final p = progress;
    return Padding(
      padding: EdgeInsets.only(bottom: d.isTv ? 14 : 12),
      child: KFocusable(
        onActivate: onTap,
        focusScale: d.isTv ? 1.03 : 1.01,
        light: false,
        semanticLabel: '${episodeCode(episode)} ${episode.title}',
        builder: (context, focused) => AnimatedContainer(
          duration: KMotion.focus,
          padding: const EdgeInsets.all(8),
          color: focused
              ? Colors.white.withValues(alpha: .08)
              : Colors.transparent,
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              ClipRRect(
                borderRadius: BorderRadius.circular(d.cardRadius * .75),
                child: SizedBox(
                  width: thumbW,
                  child: AspectRatio(
                    aspectRatio: 16 / 9,
                    child: Stack(
                      fit: StackFit.expand,
                      children: [
                        Artwork(
                          url: episode.backdropUrl,
                          seed: episode.id,
                          style: ArtStyle.backdrop,
                        ),
                        Center(
                          child: Icon(
                            Icons.play_circle_outline,
                            color: Colors.white.withValues(
                              alpha: focused ? .95 : .6,
                            ),
                            size: 30,
                          ),
                        ),
                        if (p != null)
                          Positioned(
                            left: 8,
                            right: 8,
                            bottom: 7,
                            child: KProgressBar(
                              value: p.positionMs / p.durationMs,
                              height: 3,
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      '${episode.episode}. ${episode.title}',
                      style: theme.textTheme.titleMedium,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                    const SizedBox(height: 2),
                    Text(
                      [
                        episodeCode(episode),
                        formatRuntime(episode.durationMs),
                        if (p != null) formatRemaining(p),
                      ].join('  ·  '),
                      style: theme.textTheme.labelSmall,
                    ),
                    if (episode.overview != null && !d.isPhone) ...[
                      const SizedBox(height: 6),
                      Text(
                        episode.overview!,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
