// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/widgets.dart';

/// Dark, tvOS-inspired look of the player (01-product-ux-design §4:
/// glass/thin for OSD, motion/osd 0.25 s, auto-hide after 4 s).
abstract final class PaTokens {
  static const Color bg = Color(0xFF0B0B0F);
  static const Color surface = Color(0xFF16161D);
  static const Color glass = Color(0xB31C1C24); // glass/thin @ 70 %
  static const Color glassStrong = Color(0xE6202029); // panels
  static const Color text = Color(0xFFF5F5F7);
  static const Color textSecondary = Color(0xFFA1A1AA);
  static const Color accent = Color(0xFF6E8BFF);
  static const Color track = Color(0x40FFFFFF);
  static const Color focusRing = Color(0xFFFFFFFF);

  static const Duration osdFade = Duration(milliseconds: 250);
  static const Duration osdAutoHide = Duration(seconds: 4);
  static const double radius = 14;
}

/// "1:02:03" / "4:05".
String formatDuration(Duration d) {
  final neg = d.isNegative;
  d = d.abs();
  final h = d.inHours;
  final m = d.inMinutes.remainder(60);
  final s = d.inSeconds.remainder(60);
  final ss = s.toString().padLeft(2, '0');
  final body = h > 0 ? '$h:${m.toString().padLeft(2, '0')}:$ss' : '$m:$ss';
  return neg ? '-$body' : body;
}
