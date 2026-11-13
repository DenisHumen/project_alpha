// SPDX-License-Identifier: MPL-2.0
//
// Kinolune mark and wordmark (DSN-006, design/brand). The SVGs in
// assets/brand are copies of design/brand/src/{mark,wordmark}.svg.

import 'package:flutter_svg/flutter_svg.dart';
import 'package:material_ui/material_ui.dart';

import '../branding.dart';

const _markAsset = 'assets/brand/mark.svg';
const _wordmarkAsset = 'assets/brand/wordmark.svg';

/// Mark (crescent + play) followed by the wordmark, height [size].
class KinoluneLogo extends StatelessWidget {
  const KinoluneLogo({super.key, this.size = 32, this.showName = true});

  final double size;
  final bool showName;

  @override
  Widget build(BuildContext context) {
    final mark = SvgPicture.asset(
      _markAsset,
      width: size,
      height: size,
      semanticsLabel: showName ? null : kAppName,
    );
    if (!showName) return mark;
    return Semantics(
      label: kAppName,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          mark,
          SizedBox(width: size * .28),
          // Wordmark viewBox is 764x150; cap height ≈ 0.97 of the box.
          SvgPicture.asset(_wordmarkAsset, height: size * .62),
        ],
      ),
    );
  }
}
