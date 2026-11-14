// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:path/path.dart' as p;

import 'library.dart';
import 'rust_library.dart';

/// File name of the Rust-core (schema v1) database in the app data folder.
const coreDbFileName = 'kinolune-core.db';

/// File name of the `pa_library` v0 database in the app data folder.
const legacyV0DbFileName = 'kinolune.db';

/// The library the app runs on, and how it got there.
class OpenedLibrary {
  const OpenedLibrary._(
    this.library, {
    required this.onRustCore,
    this.importedV0 = false,
    this.fallbackReason,
  });

  final Library library;

  /// True for [RustLibrary], false for the v0 Dart fallback.
  final bool onRustCore;

  /// A v0 database was imported in this start. Its music tags and the items
  /// of network shares are only read by a scan, so the app should run one.
  final bool importedV0;

  /// Why the app runs on v0 although the Rust core was asked for (the native
  /// library is missing or the core failed to open or import); null if not.
  final Object? fallbackReason;
}

/// Opens the app library in [dataDir] (UI-024, ADR-0032).
///
/// With [rustCore] (the default) the library runs on the Rust core: the
/// database is [coreDbFileName]; a v0 database ([legacyV0DbFileName]) next
/// to it is imported on the first start and then renamed to
/// `kinolune.db.v0.bak` (with `-wal`/`-shm`; a free `.v0.bak.N` name if a
/// backup is already there). Nothing is deleted: the backup stays until the
/// user removes it.
///
/// The v0 Dart library ([SqliteLibrary]) is the fallback: when [rustCore] is
/// false, or when the native core cannot be loaded ([initCore] throws), the
/// core database cannot be opened or the import fails. In that case the v0
/// database is used as before — if it was already renamed, the newest backup
/// is moved back first.
///
/// [initCore] and [externalLibrary] are for tests.
Future<OpenedLibrary> openAppLibrary(
  String dataDir, {
  bool rustCore = true,
  ExternalLibrary? externalLibrary,
  Future<void> Function()? initCore,
}) async {
  final v0Path = p.join(dataDir, legacyV0DbFileName);
  if (!rustCore) {
    return OpenedLibrary._(await _openV0(v0Path), onRustCore: false);
  }

  RustLibrary? lib;
  try {
    await (initCore?.call() ??
        RustLibrary.ensureInitialized(externalLibrary: externalLibrary));
    lib = await RustLibrary.open(p.join(dataDir, coreDbFileName));
    final report = await lib.importLegacyV0(v0Path);
    return OpenedLibrary._(lib, onRustCore: true, importedV0: report != null);
  } catch (e) {
    // The v0 file is renamed only after a successful import, so it is still
    // in place here (or restored from the newest backup).
    try {
      await lib?.close();
    } catch (_) {
      // Already broken; the fallback below is what matters.
    }
    return OpenedLibrary._(
      await _openV0(v0Path),
      onRustCore: false,
      fallbackReason: e,
    );
  }
}

Future<Library> _openV0(String v0Path) async {
  if (!File(v0Path).existsSync()) {
    final backup = newestLegacyV0Backup(v0Path);
    if (backup != null) {
      for (final suffix in ['', '-wal', '-shm']) {
        final f = File('$backup$suffix');
        if (f.existsSync()) f.renameSync('$v0Path$suffix');
      }
    }
  }
  return Library.open(v0Path);
}

/// A backup path for the v0 database at [v0Path] that is free for the
/// database and its `-wal`/`-shm` files: `<v0Path>.v0.bak`, else
/// `<v0Path>.v0.bak.1`, `.2`, …
String freeLegacyV0BackupPath(String v0Path) {
  bool free(String base) => !['', '-wal', '-shm'].any(
    (s) =>
        FileSystemEntity.typeSync('$base$s') != FileSystemEntityType.notFound,
  );
  final base = '$v0Path.v0.bak';
  if (free(base)) return base;
  for (var n = 1; ; n++) {
    if (free('$base.$n')) return '$base.$n';
  }
}

/// The most recently renamed v0 backup of [v0Path] (highest number), or
/// null.
String? newestLegacyV0Backup(String v0Path) {
  final base = '$v0Path.v0.bak';
  String? newest = File(base).existsSync() ? base : null;
  var best = 0;
  final dir = Directory(p.dirname(v0Path));
  if (!dir.existsSync()) return newest;
  final prefix = '${p.basename(base)}.';
  for (final e in dir.listSync(followLinks: false)) {
    final name = p.basename(e.path);
    if (e is! File || !name.startsWith(prefix)) continue;
    final n = int.tryParse(name.substring(prefix.length));
    if (n != null && n > best) {
      best = n;
      newest = e.path;
    }
  }
  return newest;
}
