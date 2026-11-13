// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/material.dart';

import 'tokens.dart';

/// Thin progress bar that grows on hover/focus/drag. Tap or drag to seek.
/// Keyboard/D-pad seeking is handled by the owning screen while this is
/// focused (left/right ±10 s).
class Scrubber extends StatefulWidget {
  const Scrubber({
    super.key,
    required this.position,
    required this.duration,
    this.buffer = Duration.zero,
    required this.onSeek,
    this.focusNode,
    this.height = 6,
    this.showTimes = true,
    this.endLabel,
  });

  final Duration position;
  final Duration duration;
  final Duration buffer;
  final ValueChanged<Duration> onSeek;
  final FocusNode? focusNode;
  final double height;
  final bool showTimes;

  /// Overrides the right-hand label (default: "-remaining").
  final String? endLabel;

  @override
  State<Scrubber> createState() => _ScrubberState();
}

class _ScrubberState extends State<Scrubber> {
  double? _drag; // 0..1 while dragging
  bool _focused = false;
  bool _hovered = false;

  double get _fraction {
    final d = widget.duration.inMilliseconds;
    if (d <= 0) return 0;
    return (widget.position.inMilliseconds / d).clamp(0.0, 1.0);
  }

  double get _bufferFraction {
    final d = widget.duration.inMilliseconds;
    if (d <= 0) return 0;
    return (widget.buffer.inMilliseconds / d).clamp(0.0, 1.0);
  }

  Duration _at(double f) => Duration(
    milliseconds: (widget.duration.inMilliseconds * f.clamp(0.0, 1.0)).round(),
  );

  void _update(Offset local, double width) {
    setState(() => _drag = (local.dx / width).clamp(0.0, 1.0));
  }

  @override
  Widget build(BuildContext context) {
    final f = _drag ?? _fraction;
    final active = _focused || _hovered || _drag != null;
    final shown = _drag != null ? _at(_drag!) : widget.position;
    final remaining = widget.duration - shown;
    final bar = LayoutBuilder(
      builder: (context, c) {
        final w = c.maxWidth;
        final h = active ? widget.height * 1.6 : widget.height;
        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapUp: (d) {
            widget.onSeek(_at(d.localPosition.dx / w));
          },
          onHorizontalDragStart: (d) => _update(d.localPosition, w),
          onHorizontalDragUpdate: (d) => _update(d.localPosition, w),
          onHorizontalDragEnd: (_) {
            final v = _drag;
            setState(() => _drag = null);
            if (v != null) widget.onSeek(_at(v));
          },
          onHorizontalDragCancel: () => setState(() => _drag = null),
          child: SizedBox(
            height: 28,
            child: Center(
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 120),
                height: h,
                decoration: BoxDecoration(
                  color: PaTokens.track,
                  borderRadius: BorderRadius.circular(h),
                  border: _focused
                      ? Border.all(color: PaTokens.focusRing, width: 1.5)
                      : null,
                ),
                child: Stack(
                  children: [
                    FractionallySizedBox(
                      widthFactor: _bufferFraction,
                      child: Container(
                        decoration: BoxDecoration(
                          color: const Color(0x40FFFFFF),
                          borderRadius: BorderRadius.circular(h),
                        ),
                      ),
                    ),
                    FractionallySizedBox(
                      widthFactor: f,
                      child: Container(
                        decoration: BoxDecoration(
                          color: PaTokens.text,
                          borderRadius: BorderRadius.circular(h),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
    return Focus(
      focusNode: widget.focusNode,
      onFocusChange: (v) => setState(() => _focused = v),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            bar,
            if (widget.showTimes)
              Row(
                children: [
                  Text(formatDuration(shown), style: _timeStyle),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      widget.endLabel ?? '-${formatDuration(remaining)}',
                      textAlign: TextAlign.end,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: _timeStyle,
                    ),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }
}

const _timeStyle = TextStyle(
  color: PaTokens.textSecondary,
  fontSize: 14,
  fontFeatures: [FontFeature.tabularFigures()],
);
