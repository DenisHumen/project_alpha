// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import '../integration/library.dart';
import 'source_manager.dart';

/// [SourceManager] over the library's own source list: the v0 fallback when
/// the Rust core is not available (UI-024), and the demo library.
///
/// Reduced: local folders can be added; status is whether the folder exists
/// now; no edit, no speed test. Network shares added earlier stay listed.
class LibrarySourceManager extends SourceManager {
  LibrarySourceManager(this.library);

  final Library library;

  @override
  bool get reduced => true;

  @override
  Set<SourceType> get creatableTypes => const {SourceType.local};

  @override
  bool get canEdit => false;

  @override
  bool get canSpeedTest => false;

  static bool _exists(String path) {
    try {
      return Directory(path).existsSync();
    } on FileSystemException {
      return false;
    }
  }

  ManagedSource _source(MediaSource s) {
    final local = s.kind == SourceKind.localFolder;
    return ManagedSource(
      id: s.id,
      type: local ? SourceType.local : SourceType.smb,
      name: s.name,
      address: s.path,
      username: s.username,
      status: local ? _folderStatus(s.path) : const SourceStatusInfo(),
    );
  }

  /// Whether the folder is there now; no stored status on v0.
  static SourceStatusInfo _folderStatus(String path) {
    final ok = _exists(path);
    final now = DateTime.now();
    return SourceStatusInfo(
      online: ok,
      checkedAt: now,
      lastError: ok ? null : 'folder not found: $path', // l10n-ignore
      lastErrorAt: ok ? null : now,
    );
  }

  @override
  Future<List<ManagedSource>> list() async =>
      (await library.sources()).map(_source).toList();

  @override
  Future<ManagedSource> create(SourceDraft draft) async {
    if (draft.type != SourceType.local) {
      throw SourceException(SourceErrorCode.unsupported, draft.type.name);
    }
    final added = await library.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: draft.name,
        path: draft.address,
      ),
    );
    return _source(added);
  }

  @override
  Future<ManagedSource> update(String id, SourceDraft draft) async =>
      throw const SourceException(SourceErrorCode.unsupported);

  /// The library entry itself is removed by `Library.removeSource`.
  @override
  Future<void> delete(String id) async {}

  @override
  Future<SourceStatusInfo> check(String id) async {
    final s = (await library.sources()).where((s) => s.id == id).firstOrNull;
    if (s == null) throw SourceException(SourceErrorCode.sourceMissing, id);
    if (s.kind != SourceKind.localFolder) {
      throw const SourceException(SourceErrorCode.unsupported);
    }
    return _folderStatus(s.path);
  }

  @override
  Future<SpeedResult> speedTest(String id, {int maxSeconds = 10}) async =>
      throw const SourceException(SourceErrorCode.unsupported);
}
