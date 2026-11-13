// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'detail_layout.dart';
import 'fix_match_dialog.dart';
import 'playback.dart';

class MovieDetailScreen extends StatefulWidget {
  const MovieDetailScreen({super.key, required this.item});

  final LibraryItem item;

  @override
  State<MovieDetailScreen> createState() => _MovieDetailScreenState();
}

class _MovieDetailScreenState extends State<MovieDetailScreen> {
  Stream<List<(LibraryItem, Progress)>>? _cw;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _cw ??= Services.of(context).library.continueWatching();
  }

  @override
  Widget build(BuildContext context) {
    final item = widget.item;
    final d = KDensity.of(context);
    return StreamBuilder<List<(LibraryItem, Progress)>>(
      stream: _cw,
      builder: (context, snap) {
        Progress? progress;
        for (final (i, p) in snap.data ?? const <(LibraryItem, Progress)>[]) {
          if (i.id == item.id) progress = p;
        }
        final meta = [
          if (item.year != null) '${item.year}',
          formatRuntime(item.durationMs),
          if (progress != null) formatRemaining(progress),
        ].where((s) => s.isNotEmpty).join('  ·  ');
        return DetailBackdropPage(
          item: item,
          meta: meta,
          actions: [
            KButton(
              autofocus: !d.isPhone,
              primary: true,
              icon: Icons.play_arrow_rounded,
              label: progress != null ? 'Resume ${formatClock(progress.positionMs)}' : 'Play',
              onPressed: () => playVideo(context, item, startPositionMs: progress?.positionMs ?? 0),
            ),
            if (progress != null)
              KButton(icon: Icons.replay, label: 'From beginning', onPressed: () => playVideo(context, item)),
            KButton(
              icon: Icons.manage_search,
              label: 'Fix match',
              onPressed: () => showFixMatchDialog(context, item).then((_) => setState(() {})),
            ),
          ],
          extra: Padding(
            padding: const EdgeInsets.only(top: 16),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              MatchBadge(confidence: item.matchConfidence),
              const SizedBox(height: 4),
              Text(item.filePath, maxLines: 1, overflow: TextOverflow.ellipsis, style: Theme.of(context).textTheme.labelSmall?.copyWith(color: KColors.textTertiary)),
            ]),
          ),
        );
      },
    );
  }
}
