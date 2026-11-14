// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../library/library_queries.dart';
import '../theme/kinolune_theme.dart';

/// Vertical A–Z strip of the library grid (UI-008): tap or drag to jump to
/// the first title of a letter. Letters that do not fit are skipped in the
/// strip, but dragging still reaches every one.
class AlphabetScrubber extends StatefulWidget {
  const AlphabetScrubber({
    super.key,
    required this.entries,
    required this.onSelected,
    this.semanticLabel,
  });

  final List<AlphabetEntry> entries;
  final ValueChanged<AlphabetEntry> onSelected;
  final String? semanticLabel;

  @override
  State<AlphabetScrubber> createState() => _AlphabetScrubberState();
}

class _AlphabetScrubberState extends State<AlphabetScrubber> {
  String? _active;

  static const _minSlot = 14.0, _maxSlot = 22.0;

  void _pick(double dy, double height) {
    final n = widget.entries.length;
    if (n == 0 || height <= 0) return;
    final i = (dy / height * n).floor().clamp(0, n - 1);
    final e = widget.entries[i];
    if (e.letter != _active) {
      setState(() => _active = e.letter);
      widget.onSelected(e);
    }
  }

  void _end() => setState(() => _active = null);

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final style = Theme.of(context).textTheme.labelSmall?.copyWith(
      fontSize: d.type.caption2,
      fontWeight: FontWeight.w600,
      color: KColors.textSecondary,
    );
    return Semantics(
      label: widget.semanticLabel,
      child: LayoutBuilder(
        builder: (context, c) {
          final n = widget.entries.length;
          final slot = (c.maxHeight / n).clamp(0.0, _maxSlot);
          final height = slot * n;
          // Show every step-th letter when the slots get too small.
          final step = slot >= _minSlot ? 1 : (_minSlot / slot).ceil();
          return Align(
            alignment: Alignment.center,
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapDown: (e) => _pick(e.localPosition.dy, height),
              onTapUp: (_) => _end(),
              onTapCancel: _end,
              onVerticalDragStart: (e) => _pick(e.localPosition.dy, height),
              onVerticalDragUpdate: (e) => _pick(e.localPosition.dy, height),
              onVerticalDragEnd: (_) => _end(),
              child: SizedBox(
                width: 24,
                height: height,
                child: Stack(
                  clipBehavior: Clip.none,
                  children: [
                    for (var i = 0; i < n; i += step)
                      Positioned(
                        top: i * slot,
                        left: 0,
                        right: 0,
                        height: slot,
                        child: OverflowBox(
                          key: ValueKey('alpha-${widget.entries[i].letter}'),
                          maxHeight: slot * step,
                          child: Text(
                            widget.entries[i].letter,
                            style: widget.entries[i].letter == _active
                                ? style?.copyWith(color: KColors.textPrimary)
                                : style,
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
