// SPDX-License-Identifier: MPL-2.0
import 'dart:math' as math;

import 'package:flutter/widgets.dart';

import 'subtitle_style.dart';

/// Draws the current subtitle cue ([lines]) over the video with
/// [appearance]: size relative to the height of the area, font, colors,
/// edge, line and window backgrounds, distance from the bottom.
class SubtitleOverlay extends StatelessWidget {
  const SubtitleOverlay({
    super.key,
    required this.lines,
    required this.appearance,
  });

  final List<String> lines;
  final SubtitleAppearance appearance;

  /// Font family and features for [font]. Android resolves these names to
  /// its system fonts (the captioning typefaces), Linux through fontconfig
  /// generic families; unknown names fall back to the default font.
  static (String?, List<FontFeature>) family(SubtitleFont font) =>
      switch (font) {
        SubtitleFont.sansSerif => (null, const []),
        SubtitleFont.serif => ('serif', const []),
        SubtitleFont.monospace => ('monospace', const []),
        SubtitleFont.casual => ('casual', const []),
        SubtitleFont.cursive => ('cursive', const []),
        SubtitleFont.condensed => ('sans-serif-condensed', const []),
        SubtitleFont.smallCaps => (null, const [FontFeature.enable('smcp')]),
      };

  /// Font size in logical pixels for an area [height] tall.
  static double fontSizeFor(double height, double scale) => math.max(
    10,
    height * SubtitleAppearance.textSizeFraction * scale.clamp(0.25, 4.0),
  );

  @override
  Widget build(BuildContext context) {
    final text = lines.join('\n');
    if (text.trim().isEmpty) return const SizedBox.shrink();
    return IgnorePointer(
      child: LayoutBuilder(
        builder: (context, box) {
          final a = appearance;
          final size = fontSizeFor(box.maxHeight, a.fontScale);
          final (fontFamily, features) = family(a.font);
          final base = TextStyle(
            fontSize: size,
            height: 1.25,
            fontFamily: fontFamily,
            fontFamilyFallback: fontFamily == null
                ? null
                : const ['sans-serif'],
            fontFeatures: features,
            fontWeight: FontWeight.w500,
            decoration: TextDecoration.none,
          );
          final edgeWidth = math.max(1.0, size * 0.08);
          final offset = math.max(1.0, size * 0.05);
          final shadows = switch (a.edge) {
            SubtitleEdge.dropShadow => [
              Shadow(
                offset: Offset(offset, offset),
                blurRadius: offset * 1.5,
                color: a.edgeColor,
              ),
            ],
            SubtitleEdge.raised => [
              Shadow(
                offset: Offset(-offset / 2, -offset / 2),
                color: const Color(0x80FFFFFF),
              ),
              Shadow(offset: Offset(offset, offset), color: a.edgeColor),
            ],
            SubtitleEdge.depressed => [
              Shadow(offset: Offset(-offset, -offset), color: a.edgeColor),
              Shadow(
                offset: Offset(offset / 2, offset / 2),
                color: const Color(0x80FFFFFF),
              ),
            ],
            SubtitleEdge.outline => [
              Shadow(
                blurRadius: edgeWidth,
                color: a.edgeColor.withValues(alpha: 0.5),
              ),
            ],
            SubtitleEdge.none => const <Shadow>[],
          };
          Widget layer(TextStyle style, {Key? key}) => Text(
            text,
            key: key,
            textAlign: TextAlign.center,
            softWrap: true,
            style: base.merge(style),
          );
          final hasLineBox = a.backgroundColor.a > 0;
          final content = Stack(
            alignment: Alignment.center,
            children: [
              if (hasLineBox)
                layer(
                  TextStyle(
                    color: const Color(0x00000000),
                    backgroundColor: a.backgroundColor,
                  ),
                ),
              if (a.edge == SubtitleEdge.outline)
                layer(
                  TextStyle(
                    foreground: Paint()
                      ..style = PaintingStyle.stroke
                      ..strokeWidth = edgeWidth
                      ..strokeJoin = StrokeJoin.round
                      ..color = a.edgeColor,
                  ),
                ),
              layer(
                TextStyle(color: a.textColor, shadows: shadows),
                key: const ValueKey('subtitle-text'),
              ),
            ],
          );
          return Padding(
            padding: EdgeInsets.fromLTRB(
              box.maxWidth * 0.05,
              0,
              box.maxWidth * 0.05,
              box.maxHeight * a.bottomMargin,
            ),
            child: Align(
              alignment: Alignment.bottomCenter,
              child: DecoratedBox(
                key: const ValueKey('subtitle-window'),
                decoration: BoxDecoration(
                  color: a.windowColor,
                  borderRadius: BorderRadius.circular(size * 0.15),
                ),
                child: Padding(
                  padding: EdgeInsets.symmetric(
                    horizontal: size * 0.3,
                    vertical: size * 0.1,
                  ),
                  child: content,
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
