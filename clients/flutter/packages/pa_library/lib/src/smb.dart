// SPDX-License-Identifier: MPL-2.0
//
// SMB support for 0.1 (beta):
// - Linux: the share is accessed through the GVfs FUSE mount
//   (/run/user/<uid>/gvfs/smb-share:server=…,share=…). If it is not mounted
//   yet, we try `gio mount smb://…` (credentials fed on stdin).
//   A share mounted by the user (mount.cifs, file manager) can also simply be
//   added as a local folder.
// - Android: no licence-compatible pure-Dart SMB2 client is bundled in 0.1;
//   scanning an SMB source reports [SmbUnsupportedException].

import 'dart:async';
import 'dart:convert';
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

/// Resolves an SMB source to a local folder the scanner can walk.
class SmbResolver {
  /// Overridable for tests.
  final String? gvfsRootOverride;
  final Future<ProcessResult> Function(
    String exe,
    List<String> args, {
    String? stdinText,
  })?
  runner;

  SmbResolver({this.gvfsRootOverride, this.runner});

  Future<String> _gvfsRoot() async {
    if (gvfsRootOverride != null) return gvfsRootOverride!;
    final runtime = Platform.environment['XDG_RUNTIME_DIR'];
    if (runtime != null && runtime.isNotEmpty) return p.join(runtime, 'gvfs');
    final id = await Process.run('id', ['-u']);
    return '/run/user/${(id.stdout as String).trim()}/gvfs';
  }

  Future<ProcessResult> _run(
    String exe,
    List<String> args, {
    String? stdinText,
  }) async {
    if (runner != null) return runner!(exe, args, stdinText: stdinText);
    final proc = await Process.start(exe, args);
    if (stdinText != null) proc.stdin.write(stdinText);
    await proc.stdin.close();
    final out = proc.stdout.transform(utf8.decoder).join();
    final err = proc.stderr.transform(utf8.decoder).join();
    final code = await proc.exitCode.timeout(
      const Duration(seconds: 45),
      onTimeout: () {
        proc.kill();
        return -1;
      },
    );
    return ProcessResult(proc.pid, code, await out, await err);
  }

  /// Local path of [url] if the share is already mounted, without mounting
  /// it. Null when it is not mounted or SMB is not supported here.
  Future<String?> mounted(String url) async {
    if (!Platform.isLinux && gvfsRootOverride == null) return null;
    try {
      return findGvfsPath(SmbUrl.parse(url), await _gvfsRoot());
    } on FormatException {
      return null;
    }
  }

  Future<String> resolve(
    String url, {
    String? username,
    String? password,
  }) async {
    if (Platform.isAndroid || Platform.isIOS) {
      throw SmbUnsupportedException(
        'SMB is not supported on this platform in Kinolune 0.1 (beta). '
        'Copy media to the device or use a folder source.',
      );
    }
    if (!Platform.isLinux && gvfsRootOverride == null) {
      throw SmbUnsupportedException(
        'SMB sources are supported on Linux only in 0.1.',
      );
    }
    final smb = SmbUrl.parse(url);
    final root = await _gvfsRoot();
    var local = findGvfsPath(smb, root);
    if (local != null) return local;

    // Not mounted yet: ask GVfs to mount it. gio prompts for user, domain and
    // password on stdin when the server asks for authentication.
    final user = username ?? smb.user;
    final stdinText = password == null && user == null
        ? '\n\n\n'
        : '${user ?? ''}\nWORKGROUP\n${password ?? ''}\n';
    final ProcessResult res;
    try {
      res = await _run('gio', ['mount', smb.shareUrl], stdinText: stdinText);
    } on ProcessException {
      throw SmbMountException(
        'Could not run "gio" to mount ${smb.shareUrl}. Mount the share in your '
        'file manager (or with mount.cifs) and add the mounted folder as a local folder source.',
      );
    }
    local = findGvfsPath(smb, root);
    if (local != null) return local;
    final err = (res.stderr is String ? res.stderr as String : '').trim();
    throw SmbMountException(
      'Could not mount ${smb.shareUrl}${err.isNotEmpty ? ': $err' : ''}. '
      'Check the address and credentials, or mount it in your file manager and '
      'add the mounted folder as a local folder source.',
    );
  }
}
