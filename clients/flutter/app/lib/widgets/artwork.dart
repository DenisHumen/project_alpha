// SPDX-License-Identifier: MPL-2.0
//
// Artwork: network/file images with a deterministic placeholder drawn in code
// (no bundled third-party art). Placeholders are seeded by a string, so a
// title always gets the same colours and shapes.

import 'dart:math' as math;

import 'package:material_ui/material_ui.dart';

import 'artwork_image.dart';

enum ArtStyle { poster, backdrop, album }

/// `posterUrl`/`backdropUrl` prefix used by the demo library.
const kDemoArtScheme = 'demo:';

/// Brand placeholder for missing art (design/placeholders, DSN-006) with the
/// title on top for posters.
class BrandPlaceholder extends StatelessWidget {
  const BrandPlaceholder({
    super.key,
    required this.style,
    this.title,
    this.subtitle,
  });

  final ArtStyle style;
  final String? title;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final asset = switch (style) {
      ArtStyle.poster => 'assets/placeholders/poster.png',
      ArtStyle.backdrop => 'assets/placeholders/backdrop.png',
      ArtStyle.album => 'assets/placeholders/album.png',
    };
    final image = Image.asset(asset, fit: BoxFit.cover, gaplessPlayback: true);
    final t = title;
    if (t == null || style != ArtStyle.poster) return image;
    return Stack(
      fit: StackFit.expand,
      children: [
        image,
        LayoutBuilder(
          builder: (context, c) => Padding(
            padding: EdgeInsets.all(c.maxWidth * .08),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.end,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  t,
                  maxLines: 4,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: Colors.white.withValues(alpha: .9),
                    fontSize: (c.maxWidth * .1).clamp(8.0, 28.0),
                    fontWeight: FontWeight.w600,
                    height: 1.15,
                  ),
                ),
                if (subtitle != null)
                  Text(
                    subtitle!,
                    style: TextStyle(
                      color: Colors.white.withValues(alpha: .6),
                      fontSize: (c.maxWidth * .07).clamp(6.0, 18.0),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

int _hash(String s) {
  var h = 0x811c9dc5;
  for (final c in s.codeUnits) {
    h ^= c;
    h = (h * 0x01000193) & 0xffffffff;
  }
  return h;
}

/// Shows [url] (http(s) or local file) and falls back to [GeneratedArt].
class Artwork extends StatelessWidget {
  const Artwork({
    super.key,
    required this.url,
    required this.seed,
    required this.style,
    this.title,
    this.subtitle,
  });

  final String? url;
  final String seed;
  final ArtStyle style;
  final String? title;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final u = url;
    // Demo library: art drawn in code, seeded by the item.
    if (u != null && u.startsWith(kDemoArtScheme)) {
      return GeneratedArt(
        seed: seed,
        style: style,
        title: title,
        subtitle: subtitle,
      );
    }
    final fallback = BrandPlaceholder(
      style: style,
      title: title,
      subtitle: subtitle,
    );
    if (u == null || u.isEmpty) return fallback;
    Widget err(BuildContext _, Object _, StackTrace? _) => fallback;
    // Remote art comes from the core's disk cache in the size on screen and
    // is decoded at that size off the UI thread (LIB-013).
    return LayoutBuilder(
      builder: (context, c) => Image(
        image: artworkProvider(u, width: decodeWidth(context, c.maxWidth)),
        fit: BoxFit.cover,
        errorBuilder: err,
        gaplessPlayback: true,
      ),
    );
  }
}

class GeneratedArt extends StatelessWidget {
  const GeneratedArt({
    super.key,
    required this.seed,
    required this.style,
    this.title,
    this.subtitle,
  });

  final String seed;
  final ArtStyle style;
  final String? title;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final painter = _ArtPainter(_hash(seed), style);
    final t = title;
    if (t == null) {
      return CustomPaint(painter: painter, child: const SizedBox.expand());
    }
    return CustomPaint(
      painter: painter,
      child: LayoutBuilder(
        builder: (context, c) {
          final w = c.maxWidth;
          return Padding(
            padding: EdgeInsets.all(w * .08),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.end,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  t.toUpperCase(),
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: Colors.white,
                    fontSize: (w * .115).clamp(8.0, 40.0),
                    fontWeight: FontWeight.w800,
                    height: 1.05,
                    letterSpacing: w * .006,
                    shadows: const [
                      Shadow(color: Color(0x88000000), blurRadius: 8),
                    ],
                  ),
                ),
                if (subtitle != null) ...[
                  SizedBox(height: w * .025),
                  Text(
                    subtitle!,
                    maxLines: 1,
                    style: TextStyle(
                      color: Colors.white.withValues(alpha: .75),
                      fontSize: (w * .065).clamp(6.0, 18.0),
                      fontWeight: FontWeight.w500,
                      letterSpacing: w * .01,
                    ),
                  ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }
}

class _ArtPainter extends CustomPainter {
  _ArtPainter(this.h, this.style);

  final int h;
  final ArtStyle style;

  double _r(int i) => ((h >> (i % 24)) ^ (h * (i + 7))) % 1000 / 1000.0;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final hue = (h % 360).toDouble();
    final hue2 = (hue + 25 + _r(1) * 70) % 360;
    final c1 = HSLColor.fromAHSL(1, hue, .55, .34).toColor();
    final c2 = HSLColor.fromAHSL(1, hue2, .6, .1).toColor();
    canvas.drawRect(
      rect,
      Paint()
        ..shader = LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [c1, c2],
        ).createShader(rect),
    );
    switch (style) {
      case ArtStyle.album:
        _album(canvas, size, hue);
      case ArtStyle.poster:
      case ArtStyle.backdrop:
        _scene(canvas, size, hue, hue2);
    }
  }

  void _scene(Canvas canvas, Size s, double hue, double hue2) {
    final w = s.width, hgt = s.height;
    // Moon / sun with halo.
    final orb = Offset(
      w * (.2 + _r(2) * .6),
      hgt * (style == ArtStyle.poster ? .18 + _r(3) * .2 : .2 + _r(3) * .25),
    );
    final rad = math.min(w, hgt) * (.1 + _r(4) * .1);
    final orbColor = HSLColor.fromAHSL(1, (hue + 180) % 360, .5, .82).toColor();
    canvas.drawCircle(
      orb,
      rad * 2.6,
      Paint()
        ..shader = RadialGradient(
          colors: [
            orbColor.withValues(alpha: .35),
            orbColor.withValues(alpha: 0),
          ],
        ).createShader(Rect.fromCircle(center: orb, radius: rad * 2.6)),
    );
    canvas.drawCircle(orb, rad, Paint()..color = orbColor);
    if (_r(5) > .45) {
      // crescent
      canvas.drawCircle(
        orb + Offset(rad * .45, -rad * .2),
        rad * .9,
        Paint()..color = HSLColor.fromAHSL(1, hue, .5, .3).toColor(),
      );
    }
    // Layered hills.
    for (var layer = 0; layer < 3; layer++) {
      final base = hgt * (.55 + layer * .13);
      final amp = hgt * (.06 + _r(10 + layer) * .08);
      final freq = 1.2 + _r(20 + layer) * 2.2;
      final phase = _r(30 + layer) * math.pi * 2;
      final path = Path()..moveTo(0, hgt);
      for (var x = 0.0; x <= w; x += w / 48) {
        final y =
            base -
            amp * math.sin(x / w * math.pi * freq + phase) -
            amp * .4 * math.sin(x / w * math.pi * freq * 2.7 + phase * 1.3);
        path.lineTo(x, y);
      }
      path
        ..lineTo(w, hgt)
        ..close();
      final light = .2 - layer * .06;
      canvas.drawPath(
        path,
        Paint()
          ..color = HSLColor.fromAHSL(
            1,
            (hue2 + layer * 8) % 360,
            .45,
            light.clamp(.03, 1),
          ).toColor(),
      );
    }
    // Vignette for legibility.
    canvas.drawRect(
      Offset.zero & s,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Color(0x00000000), Color(0x00000000), Color(0x99000000)],
          stops: [0, .55, 1],
        ).createShader(Offset.zero & s),
    );
  }

  void _album(Canvas canvas, Size s, double hue) {
    final c = s.center(Offset.zero);
    final m = math.min(s.width, s.height);
    final accent = HSLColor.fromAHSL(1, (hue + 160) % 360, .6, .65).toColor();
    switch (h % 3) {
      case 0: // concentric rings
        for (var i = 7; i > 0; i--) {
          canvas.drawCircle(
            c + Offset(m * (_r(2) - .5) * .3, m * (_r(3) - .5) * .3),
            m * .07 * i,
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = m * .018
              ..color = accent.withValues(alpha: .12 + i * .08),
          );
        }
      case 1: // diagonal stripes
        canvas.save();
        canvas.clipRect(Offset.zero & s);
        final p = Paint()
          ..strokeWidth = m * .06
          ..color = accent.withValues(alpha: .55);
        for (var i = -6; i < 10; i++) {
          final x = i * m * .16;
          canvas.drawLine(
            Offset(x, s.height),
            Offset(x + s.height, 0),
            p..color = accent.withValues(alpha: (.15 + (i % 3) * .2)),
          );
        }
        canvas.restore();
      default: // grid of dots
        final p = Paint()..color = accent;
        for (var y = 0; y < 6; y++) {
          for (var x = 0; x < 6; x++) {
            final r = m * .035 * (.4 + ((x + y + h) % 4) * .3);
            canvas.drawCircle(
              Offset(m * (.14 + x * .145), m * (.14 + y * .145)),
              r,
              p
                ..color = accent.withValues(
                  alpha: .25 + ((x * y + h) % 5) * .15,
                ),
            );
          }
        }
    }
  }

  @override
  bool shouldRepaint(_ArtPainter old) => old.h != h || old.style != style;
}
