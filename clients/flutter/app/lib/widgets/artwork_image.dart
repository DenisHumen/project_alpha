// SPDX-License-Identifier: MPL-2.0
//
// Image providers for artwork (LIB-013). Remote posters, backdrops and
// stills go through the core's disk cache ([ArtworkCache]): the core
// downloads the provider size variant for the pixels on screen, and Flutter
// decodes the local file at display size on its image threads, not the UI
// thread ([ResizeImage]).

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart' show SynchronousFuture;
import 'package:material_ui/material_ui.dart';

import '../integration/library.dart' show ArtworkCache;

/// Artwork cache of the running app; set in `main()`. `null` in the demo
/// mode and in widget tests: remote art then loads with [NetworkImage].
ArtworkCache? appArtworkCache;

/// Decoded widths are rounded up to this step, so slightly different
/// layouts share one decoded image in Flutter's memory cache.
const _widthStep = 64;

/// Physical width to decode an image at for [logicalWidth] on screen.
int? decodeWidth(BuildContext context, double logicalWidth) {
  if (!logicalWidth.isFinite || logicalWidth <= 0) return null;
  final px = (logicalWidth * MediaQuery.devicePixelRatioOf(context)).ceil();
  return ((px + _widthStep - 1) ~/ _widthStep) * _widthStep;
}

/// Provider for [url] (http(s), `file://` or a local path) decoded at
/// [width] physical pixels (`null`: full size).
ImageProvider artworkProvider(
  String url, {
  int? width,
  ArtworkCache? cache,
}) {
  final c = cache ?? appArtworkCache;
  final remote = url.startsWith('http://') || url.startsWith('https://');
  final ImageProvider base;
  if (remote && c != null) {
    base = CachedArtworkImage(c, url, width: width);
  } else if (remote) {
    base = NetworkImage(url);
  } else {
    final path = url.startsWith('file://') ? Uri.parse(url).toFilePath() : url;
    base = FileImage(File(path));
  }
  return ResizeImage.resizeIfNeeded(width, null, base);
}

/// An artwork image from the core's disk cache: a cached file shows at
/// once, otherwise the core downloads it (off the UI thread) first.
@immutable
class CachedArtworkImage extends ImageProvider<CachedArtworkImage> {
  const CachedArtworkImage(this.cache, this.url, {this.width});

  final ArtworkCache cache;
  final String url;

  /// Physical pixels on screen; picks the provider size variant.
  final int? width;

  @override
  Future<CachedArtworkImage> obtainKey(ImageConfiguration configuration) =>
      SynchronousFuture<CachedArtworkImage>(this);

  @override
  ImageStreamCompleter loadImage(
    CachedArtworkImage key,
    ImageDecoderCallback decode,
  ) => MultiFrameImageStreamCompleter(
    codec: _load(key, decode),
    scale: 1,
    debugLabel: key.url,
  );

  static Future<ui.Codec> _load(
    CachedArtworkImage key,
    ImageDecoderCallback decode,
  ) async {
    final path =
        key.cache.cached(key.url, width: key.width) ??
        await key.cache.resolve(key.url, width: key.width);
    if (path == null) {
      throw StateError('Artwork is not available offline: ${key.url}');
    }
    return decode(await ui.ImmutableBuffer.fromFilePath(path));
  }

  @override
  bool operator ==(Object other) =>
      other is CachedArtworkImage &&
      identical(other.cache, cache) &&
      other.url == url &&
      other.width == width;

  @override
  int get hashCode => Object.hash(identityHashCode(cache), url, width);

  @override
  String toString() => 'CachedArtworkImage("$url", width: $width)';
}
