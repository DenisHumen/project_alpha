// SPDX-License-Identifier: MPL-2.0
//
// Placeholder logo, original and drawn in code: a crescent moon ("lune")
// cradling a play triangle. The brand mark is decided in DSN-006; app icons
// and the Android TV banner are rendered from this painter by
// test/tool/generate_icons_test.dart.

import 'dart:math' as math;

import 'package:material_ui/material_ui.dart';

import '../branding.dart';

const kLogoBgTop = Color(0xFF1B2A55);
const kLogoBgBottom = Color(0xFF0B0B1A);
const kLogoMoon = Color(0xFFF3E9C6);
const kLogoPlay = Color(0xFF3B82F6);

/// Draws the mark into a square of [size]. [background] fills a rounded tile.
class LogoPainter extends CustomPainter {
  const LogoPainter({this.background = true, this.cornerRadius = .22, this.markScale = 1});

  final bool background;
  final double cornerRadius;

  /// Scale of the mark inside the square (adaptive icons need ~0.6).
  final double markScale;

  @override
  void paint(Canvas canvas, Size size) {
    final s = math.min(size.width, size.height);
    final rect = Offset.zero & Size(s, s);
    if (background) {
      final rr = RRect.fromRectAndRadius(rect, Radius.circular(s * cornerRadius));
      canvas.drawRRect(
        rr,
        Paint()
          ..shader = const LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [kLogoBgTop, kLogoBgBottom],
          ).createShader(rect),
      );
    }
    canvas.save();
    canvas.translate(s / 2, s / 2);
    canvas.scale(markScale);
    canvas.translate(-s / 2, -s / 2);

    // Halo
    final c = Offset(s * .5, s * .5);
    canvas.drawCircle(
      c,
      s * .42,
      Paint()
        ..shader = RadialGradient(colors: [kLogoMoon.withValues(alpha: .18), kLogoMoon.withValues(alpha: 0)])
            .createShader(Rect.fromCircle(center: c, radius: s * .42)),
    );
    // Crescent = moon disc minus offset disc.
    final moon = Path()..addOval(Rect.fromCircle(center: c, radius: s * .3));
    final bite = Path()..addOval(Rect.fromCircle(center: c + Offset(s * .14, -s * .07), radius: s * .26));
    canvas.drawPath(Path.combine(PathOperation.difference, moon, bite), Paint()..color = kLogoMoon);
    // Play triangle in the crescent's hollow.
    final tri = Path()
      ..moveTo(s * .47, s * .36)
      ..lineTo(s * .69, s * .48)
      ..lineTo(s * .47, s * .60)
      ..close();
    canvas.drawPath(
      tri,
      Paint()
        ..color = kLogoPlay
        ..style = PaintingStyle.fill
        ..strokeJoin = StrokeJoin.round,
    );
    canvas.drawPath(
      tri,
      Paint()
        ..color = kLogoPlay
        ..style = PaintingStyle.stroke
        ..strokeWidth = s * .035
        ..strokeJoin = StrokeJoin.round,
    );
    canvas.restore();
  }

  @override
  bool shouldRepaint(LogoPainter old) =>
      old.background != background || old.cornerRadius != cornerRadius || old.markScale != markScale;
}

class KinoluneLogo extends StatelessWidget {
  const KinoluneLogo({super.key, this.size = 32, this.showName = true, this.nameStyle});

  final double size;
  final bool showName;
  final TextStyle? nameStyle;

  @override
  Widget build(BuildContext context) {
    final mark = SizedBox.square(dimension: size, child: const CustomPaint(painter: LogoPainter()));
    if (!showName) return mark;
    return Row(mainAxisSize: MainAxisSize.min, children: [
      mark,
      SizedBox(width: size * .35),
      Text(
        kAppName,
        style: nameStyle ??
            Theme.of(context).textTheme.titleLarge?.copyWith(fontSize: size * .6, fontWeight: FontWeight.w700, letterSpacing: .3),
      ),
    ]);
  }
}
