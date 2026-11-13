// SPDX-License-Identifier: MPL-2.0
import 'dart:io';

import 'package:flutter/material.dart';

import '../play_request.dart';
import '../ui/tokens.dart';

/// Album art from [PlayRequest.artworkUri] (file or http), or a generated
/// placeholder tinted from the album/title so tracks stay distinguishable.
class Artwork extends StatelessWidget {
  const Artwork({super.key, required this.request, this.size, this.radius = 8});

  final PlayRequest? request;
  final double? size;
  final double radius;

  static ImageProvider? providerFor(PlayRequest? r) {
    final uri = r?.artworkUri;
    if (uri == null || uri.isEmpty) return null;
    if (uri.startsWith('http://') || uri.startsWith('https://')) {
      return NetworkImage(uri);
    }
    final path = uri.startsWith('file://') ? Uri.parse(uri).toFilePath() : uri;
    return FileImage(File(path));
  }

  @override
  Widget build(BuildContext context) {
    final provider = providerFor(request);
    final placeholder = _Placeholder(
      seed: request?.album ?? request?.title ?? '',
    );
    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: SizedBox(
        width: size,
        height: size,
        child: AspectRatio(
          aspectRatio: 1,
          child: provider == null
              ? placeholder
              : Image(
                  image: provider,
                  fit: BoxFit.cover,
                  gaplessPlayback: true,
                  errorBuilder: (_, _, _) => placeholder,
                ),
        ),
      ),
    );
  }
}

class _Placeholder extends StatelessWidget {
  const _Placeholder({required this.seed});
  final String seed;

  @override
  Widget build(BuildContext context) {
    final hue = (seed.hashCode % 360).abs().toDouble();
    final a = HSLColor.fromAHSL(1, hue, 0.45, 0.32).toColor();
    final b = HSLColor.fromAHSL(1, (hue + 40) % 360, 0.5, 0.16).toColor();
    return DecoratedBox(
      decoration: BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [a, b],
        ),
      ),
      child: LayoutBuilder(
        builder: (context, c) => Icon(
          Icons.music_note_rounded,
          color: PaTokens.text.withValues(alpha: 0.7),
          size: c.biggest.shortestSide * 0.4,
        ),
      ),
    );
  }
}
