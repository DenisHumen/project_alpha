// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/widgets.dart';
import 'package:flutter_svg/flutter_svg.dart';

import 'icons.g.dart';

/// Draws a [PaIcon]. Size and colour default to the ambient [IconTheme],
/// so it behaves like Flutter's `Icon`.
class PaIconView extends StatelessWidget {
  const PaIconView(this.icon,
      {super.key, this.size, this.color, this.semanticLabel});

  final PaIcon icon;
  final double? size;
  final Color? color;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    final theme = IconTheme.of(context);
    final s = size ?? theme.size ?? 24;
    final c = color ?? theme.color ?? const Color(0xFFFFFFFF);
    return SvgPicture.string(
      icon.svg,
      width: s,
      height: s,
      colorFilter: ColorFilter.mode(c, BlendMode.srcIn),
      semanticsLabel: semanticLabel,
      excludeFromSemantics: semanticLabel == null,
    );
  }
}
