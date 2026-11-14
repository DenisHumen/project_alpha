// SPDX-License-Identifier: MPL-2.0
//
// SMB helpers of the Dart side.
// - On the Rust core (the default) SMB and WebDAV sources are scanned and
//   played through `pa-vfs` (LIB-018); nothing here is used for them.
// - The v0 Dart library (fallback without the native core) scans a share
//   only if the user mounted it (GVfs FUSE mount,
//   /run/user/<uid>/gvfs/smb-share:server=…,share=…); it never mounts one.

import 'dart:io';

import 'package:path/path.dart' as p;

class SmbUrl {
  final String host;
  final String share;

  /// Path inside the share, without leading slash ('' for the share root).
  final String path;
  final String? user;
  const SmbUrl(this.host, this.share, this.path, {this.user});

  /// Parses `smb://[user@]host/share[/path]`.
  static SmbUrl parse(String url) {
    final uri = Uri.parse(url.trim());
    if (uri.scheme.toLowerCase() != 'smb' || uri.host.isEmpty) {
      throw FormatException('Not an smb:// URL', url);
    }
    final segs = uri.pathSegments.where((s) => s.isNotEmpty).toList();
    if (segs.isEmpty) throw FormatException('SMB URL without a share', url);
    return SmbUrl(
      uri.host,
      segs.first,
      segs.skip(1).join('/'),
      user: uri.userInfo.isEmpty
          ? null
          : Uri.decodeComponent(uri.userInfo.split(':').first),
    );
  }

  String get shareUrl => 'smb://$host/$share';

  @override
  String toString() => 'smb://$host/$share${path.isEmpty ? '' : '/$path'}';
}

class SmbUnsupportedException implements Exception {
  final String message;
  SmbUnsupportedException(this.message);
  @override
  String toString() => message;
}

class SmbMountException implements Exception {
  final String message;
  SmbMountException(this.message);
  @override
  String toString() => message;
}

/// Finds the GVfs mount directory for [url] inside [gvfsRoot]
/// (`/run/user/<uid>/gvfs`). Returns the local path of the URL's folder or null.
String? findGvfsPath(SmbUrl url, String gvfsRoot) {
  final dir = Directory(gvfsRoot);
  if (!dir.existsSync()) return null;
  final host = url.host.toLowerCase();
  final share = url.share.toLowerCase();
  for (final e in dir.listSync(followLinks: false)) {
    final name = p.basename(e.path);
    if (!name.startsWith('smb-share:')) continue;
    final attrs = <String, String>{};
    for (final kv in name.substring('smb-share:'.length).split(',')) {
      final i = kv.indexOf('=');
      if (i > 0) {
        attrs[kv.substring(0, i)] = Uri.decodeComponent(
          kv.substring(i + 1),
        ).toLowerCase();
      }
    }
    if (attrs['server'] == host && attrs['share'] == share) {
      return url.path.isEmpty
          ? e.path
          : p.joinAll([e.path, ...url.path.split('/')]);
    }
  }
  return null;
}

/// Finds the local folder of an SMB share that the user mounted (GVfs, in
/// the file manager), for the v0 Dart library only.
///
/// Kinolune never mounts shares itself (LIB-018): on the Rust core SMB and
/// WebDAV sources are read through `pa-vfs` with the password kept in the
/// core's secret store. The v0 fallback (no native core) can only scan a
/// share that is already mounted.
class SmbResolver {
  /// Overridable for tests.
  final String? gvfsRootOverride;

  SmbResolver({this.gvfsRootOverride});

  Future<String> _gvfsRoot() async {
    if (gvfsRootOverride != null) return gvfsRootOverride!;
    final runtime = Platform.environment['XDG_RUNTIME_DIR'];
    if (runtime != null && runtime.isNotEmpty) return p.join(runtime, 'gvfs');
    final id = await Process.run('id', ['-u']);
    return '/run/user/${(id.stdout as String).trim()}/gvfs';
  }

  /// Local path of [url] if the share is already mounted. Null when it is
  /// not mounted or SMB mounts are not looked up here.
  Future<String?> mounted(String url) async {
    if (!Platform.isLinux && gvfsRootOverride == null) return null;
    try {
      return findGvfsPath(SmbUrl.parse(url), await _gvfsRoot());
    } on FormatException {
      return null;
    }
  }

  /// The local folder of a mounted share, or [SmbMountException].
  Future<String> resolve(String url) async {
    if (!Platform.isLinux && gvfsRootOverride == null) {
      throw SmbUnsupportedException(
        'SMB sources need the Kinolune core on this platform.',
      );
    }
    final smb = SmbUrl.parse(url);
    final local = findGvfsPath(smb, await _gvfsRoot());
    if (local != null) return local;
    throw SmbMountException(
      '${smb.shareUrl} is not mounted. Without the Kinolune core SMB shares '
      'are read only when mounted in the file manager; or add the mounted '
      'folder as a local folder source.',
    );
  }
}
