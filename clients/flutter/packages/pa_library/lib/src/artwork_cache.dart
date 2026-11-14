// SPDX-License-Identifier: MPL-2.0
//
// Disk cache of posters, backdrops and stills on the Rust core (LIB-013,
// `pa-imagecache`). The core downloads provider images (TMDB size variants
// for the pixels on screen), keeps them in an LRU with a size limit in the
// platform cache directory and serves them offline; the UI gets local file
// paths and decodes them at display size.
//
// TMDB terms: images are cached on the user's device only, never shared or
// re-served, and expire after six months.

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:pa_core/pa_core.dart';

import 'rust_library.dart';

/// Default size limit of the artwork cache: 256 MiB (about 10 000 grid
/// posters).
const defaultArtworkCacheBytes = 256 * 1024 * 1024;

/// Turns artwork URLs into local file paths.
abstract interface class ArtworkCache {
  /// Path of [url] if it is on disk already (or a local file), without
  /// touching the network. Synchronous: a grid can show cached posters in
  /// the first frame.
  String? cached(String url, {int? width});

  /// Path of [url] for [width] physical pixels, downloading it if needed;
  /// `null` if it cannot be had (offline and not cached, HTTP error, not an
  /// image).
  Future<String?> resolve(String url, {int? width});
}

/// Usage of the artwork cache.
typedef ArtworkCacheUsage = ({int bytes, int files, int maxBytes});

/// [ArtworkCache] on the Rust core.
class RustArtworkCache implements ArtworkCache {
  RustArtworkCache._(this._core);

  final CoreImageCache _core;

  /// Opens the cache in [dir] (e.g. `<app cache dir>/artwork`).
  static Future<RustArtworkCache> open(
    String dir, {
    int maxBytes = defaultArtworkCacheBytes,
    ExternalLibrary? externalLibrary,
  }) async {
    await RustLibrary.ensureInitialized(externalLibrary: externalLibrary);
    final core = await CoreImageCache.open(
      dir: dir,
      maxBytes: BigInt.from(maxBytes),
    );
    return RustArtworkCache._(core);
  }

  @override
  String? cached(String url, {int? width}) =>
      _core.cached(url: url, width: width);

  @override
  Future<String?> resolve(String url, {int? width}) async {
    try {
      return await _core.resolve(url: url, width: width);
    } on ImageCacheError {
      return null;
    }
  }

  /// Size of the cache and its limit.
  ArtworkCacheUsage get usage {
    final u = _core.usage();
    return (
      bytes: u.bytes.toInt(),
      files: u.files.toInt(),
      maxBytes: u.maxBytes.toInt(),
    );
  }

  /// Changes the size limit; the cache shrinks at once.
  Future<void> setMaxBytes(int maxBytes) =>
      _core.setMaxBytes(maxBytes: BigInt.from(maxBytes));

  /// Deletes every cached image.
  Future<void> clear() => _core.clear();
}
