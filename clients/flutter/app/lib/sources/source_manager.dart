// SPDX-License-Identifier: MPL-2.0
//
// The app's view of media sources (UI-005): what the Sources screen and the
// add/edit forms work with. Two implementations:
//
// * CoreSourceManager — the Rust source manager (`pa-sources`, NET-011)
//   through pa_core: local folders, SMB and WebDAV with status, checks and
//   speed tests; passwords in the OS secret store.
// * LibrarySourceManager — the v0 fallback when the Rust core is not
//   available: local folders of the Dart library only, no status.

/// Kind of a source. Only [available] kinds can be added today; the rest
/// are shown as "coming soon" in the add flow.
enum SourceType {
  local,
  smb,
  webdav,
  nfs,
  sftp,
  ftp,
  plex,
  jellyfin,
  emby;

  /// Kinds the Rust source manager supports (NET-011).
  static const available = {local, smb, webdav};

  /// `pa-sources` kind string (`local`, `smb`, `webdav`).
  String get coreKind => name;

  static SourceType? fromCoreKind(String kind) => switch (kind) {
    'local' => local,
    'smb' => smb,
    'webdav' => webdav,
    _ => null,
  };

  /// Kinds the library scan can index now. WebDAV sources can be checked
  /// and speed-tested, but the scan pipeline reads local or mounted folders
  /// only (pa-catalog); indexing over WebDAV comes with the VFS scan.
  bool get indexable => this == local || this == smb;
}

/// SMB protocol and security options (`pa_sources::SourceOptions`).
class SmbSettings {
  const SmbSettings({
    this.domain,
    this.version = 'any',
    this.encryption = 'auto',
    this.signingRequired = false,
  });

  final String? domain;

  /// `any`, `smb2` or `smb3`.
  final String version;

  /// `auto`, `required` or `off`.
  final String encryption;
  final bool signingRequired;

  static const versions = ['any', 'smb2', 'smb3']; // l10n-ignore
  static const encryptions = ['auto', 'required', 'off']; // l10n-ignore
}

/// Device-local status of a source.
class SourceStatusInfo {
  const SourceStatusInfo({
    this.online,
    this.checkedAt,
    this.lastScanStartedAt,
    this.lastScanFinishedAt,
    this.lastError,
    this.lastErrorAt,
    this.speedBytesPerSec,
    this.latencyMs,
    this.speedMeasuredAt,
  });

  /// Result of the last check; null if never checked.
  final bool? online;
  final DateTime? checkedAt;
  final DateTime? lastScanStartedAt;
  final DateTime? lastScanFinishedAt;

  /// Last connection or scan error as the core wrote it (secrets masked).
  final String? lastError;
  final DateTime? lastErrorAt;
  final int? speedBytesPerSec;
  final int? latencyMs;
  final DateTime? speedMeasuredAt;

  bool get scanning => lastScanStartedAt != null && lastScanFinishedAt == null;

  /// The last error, if nothing has succeeded since: a successful check or
  /// a scan started after it hides it.
  String? get currentError {
    final at = lastErrorAt;
    if (lastError == null || at == null) return null;
    final okCheck = online == true ? checkedAt : null;
    for (final later in [okCheck, lastScanStartedAt]) {
      if (later != null && later.isAfter(at)) return null;
    }
    return lastError;
  }
}

/// A stored source.
class ManagedSource {
  const ManagedSource({
    required this.id,
    required this.type,
    required this.name,
    required this.address,
    this.username,
    this.hasPassword = false,
    this.smb = const SmbSettings(),
    this.tlsPin,
    this.scanIntoLibrary = true,
    this.autoScan = true,
    this.enabled = true,
    this.status = const SourceStatusInfo(),
  });

  /// Same id as the library's `MediaSource.id`.
  final String id;
  final SourceType type;
  final String name;

  /// Folder path (local) or URI without credentials.
  final String address;
  final String? username;
  final bool hasPassword;
  final SmbSettings smb;

  /// WebDAV certificate pin, `sha256:<64 hex>`.
  final String? tlsPin;
  final bool scanIntoLibrary;
  final bool autoScan;
  final bool enabled;
  final SourceStatusInfo status;
}

/// Settings of a new or edited source, as the form produced them.
class SourceDraft {
  const SourceDraft({
    required this.type,
    required this.name,
    required this.address,
    this.username,
    this.password,
    this.smb = const SmbSettings(),
    this.tlsPin,
    this.scanIntoLibrary = true,
    this.autoScan = true,
  });

  final SourceType type;
  final String name;
  final String address;
  final String? username;

  /// On create: the password (null or empty: none). On update: null keeps
  /// the stored one, an empty string removes it.
  final String? password;
  final SmbSettings smb;
  final String? tlsPin;
  final bool scanIntoLibrary;
  final bool autoScan;

  @override
  String toString() =>
      'SourceDraft(${type.name}, $name, $address, user: $username, '
      'password: ${password == null ? 'null' : '***'})';
}

/// Result of a speed test.
class SpeedResult {
  const SpeedResult({
    required this.latencyMs,
    required this.bytesRead,
    required this.elapsedMs,
    this.bytesPerSec,
    this.file,
  });

  final int latencyMs;
  final int bytesRead;
  final int elapsedMs;

  /// Null when no file was found to read (only the latency was measured).
  final int? bytesPerSec;
  final String? file;
}

/// What went wrong, independent of the language. The Sources screen turns it
/// into a readable sentence (`sourceErrorText`).
///
/// The Rust core has no stable error codes yet (CORE-009): the bridge gives
/// `SourcesError.code` (`not_found`, `invalid`, `secrets`, `unreachable`,
/// `other`) and the message holds the `pa-vfs` error kind as text
/// ("source 3: authentication failed: …"). [classifySourceError] maps both;
/// it moves to the CORE-009 codes once they exist.
enum SourceErrorCode {
  /// The source was removed meanwhile.
  sourceMissing,

  /// The core refused the settings (bad address, option).
  invalidSettings,

  /// The OS keychain is locked or unavailable.
  secretStore,
  authFailed,
  permissionDenied,

  /// Host down, connection refused, share offline.
  unreachable,
  timedOut,

  /// The folder or share does not exist.
  pathNotFound,
  notADirectory,
  invalidPath,

  /// The backend is missing on this device (e.g. libsmb2 not installed).
  unsupported,

  /// The scan cannot read this kind of source yet.
  notIndexable,
  other,
}

/// Error of a source operation.
class SourceException implements Exception {
  const SourceException(this.code, [this.detail = '']);

  /// Classifies an error of the Rust bridge (`SourcesError`).
  factory SourceException.fromCore(String code, String message) =>
      SourceException(classifySourceError(code, message), message);

  final SourceErrorCode code;

  /// Technical message (secrets masked), shown under "Details".
  final String detail;

  @override
  String toString() => 'SourceException(${code.name}: $detail)';
}

/// Maps a bridge error code and message to a [SourceErrorCode].
SourceErrorCode classifySourceError(String code, String message) =>
    switch (code) {
      'not_found' => SourceErrorCode.sourceMissing,
      'invalid' => SourceErrorCode.invalidSettings,
      'secrets' => SourceErrorCode.secretStore,
      _ => classifySourceMessage(message),
    };

/// `pa-vfs` error kinds as `VfsError` prints them (`<kind>: <message>`), and
/// the scan's own fatal messages (pa-catalog).
const _kindTexts = <(String, SourceErrorCode)>[
  ('authentication failed', SourceErrorCode.authFailed), // l10n-ignore
  ('permission denied', SourceErrorCode.permissionDenied), // l10n-ignore
  ('source unavailable', SourceErrorCode.unreachable), // l10n-ignore
  ('timed out', SourceErrorCode.timedOut), // l10n-ignore
  ('not a directory', SourceErrorCode.notADirectory), // l10n-ignore
  ('invalid path', SourceErrorCode.invalidPath), // l10n-ignore
  ('unsupported', SourceErrorCode.unsupported), // l10n-ignore
  ('no local path', SourceErrorCode.notIndexable), // l10n-ignore
  ('folder not found', SourceErrorCode.pathNotFound), // l10n-ignore
  ('not found', SourceErrorCode.pathNotFound), // l10n-ignore
];

/// Classifies a stored `last_error` or an error message by the `pa-vfs`
/// kind it names. The earliest kind in the text wins, so a path that
/// happens to contain "not found" does not override the kind before it.
SourceErrorCode classifySourceMessage(String message) {
  final text = message.toLowerCase();
  SourceErrorCode? best;
  var bestAt = text.length + 1;
  for (final (kind, code) in _kindTexts) {
    final at = text.indexOf(kind);
    if (at >= 0 && at < bestAt) {
      best = code;
      bestAt = at;
    }
  }
  return best ?? SourceErrorCode.other;
}

/// The source list and its operations. Implementations: CoreSourceManager
/// (Rust core) and LibrarySourceManager (v0 fallback).
abstract class SourceManager {
  /// True on the v0 fallback: local folders only, no status or speed test.
  bool get reduced;

  /// Kinds that can be added.
  Set<SourceType> get creatableTypes;

  bool get canEdit;
  bool get canSpeedTest;

  /// Why passwords are kept in an encrypted file instead of the OS
  /// keychain (shown as a warning), or null.
  String? get secretWarning => null;

  Future<List<ManagedSource>> list();
  Future<ManagedSource> create(SourceDraft draft);
  Future<ManagedSource> update(String id, SourceDraft draft);

  /// Forgets the source's settings and password. The app removes its
  /// library items separately (`Library.removeSource`).
  Future<void> delete(String id);

  /// Checks that the source can be reached; the result is in the status.
  Future<SourceStatusInfo> check(String id);
  Future<SpeedResult> speedTest(String id, {int maxSeconds = 10});
}
