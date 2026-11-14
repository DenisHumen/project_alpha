// SPDX-License-Identifier: MPL-2.0
//
// Readable ru/en texts for sources (UI-005): kinds, error codes, form
// errors, speeds and relative times.

import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import 'source_address.dart';
import 'source_manager.dart';

String sourceTypeLabel(AppLocalizations l, SourceType t) => switch (t) {
  SourceType.local => l.sourcesTypeLocal,
  SourceType.smb => l.sourcesTypeSmb,
  SourceType.webdav => l.sourcesTypeWebdav,
  SourceType.nfs => l.sourcesTypeNfs,
  SourceType.sftp => l.sourcesTypeSftp,
  SourceType.ftp => l.sourcesTypeFtp,
  SourceType.plex => l.sourcesTypePlex,
  SourceType.jellyfin => l.sourcesTypeJellyfin,
  SourceType.emby => l.sourcesTypeEmby,
};

IconData sourceTypeIcon(SourceType t) => switch (t) {
  SourceType.local => Icons.folder_outlined,
  SourceType.smb || SourceType.nfs => Icons.lan_outlined,
  SourceType.webdav => Icons.cloud_outlined,
  SourceType.sftp || SourceType.ftp => Icons.dns_outlined,
  SourceType.plex ||
  SourceType.jellyfin ||
  SourceType.emby => Icons.video_library_outlined,
};

/// A sentence that says what happened and what to try.
String sourceErrorText(AppLocalizations l, SourceErrorCode code) =>
    switch (code) {
      SourceErrorCode.sourceMissing => l.sourcesErrorSourceMissing,
      SourceErrorCode.invalidSettings => l.sourcesErrorInvalid,
      SourceErrorCode.secretStore => l.sourcesErrorSecretStore,
      SourceErrorCode.authFailed => l.sourcesErrorAuth,
      SourceErrorCode.permissionDenied => l.sourcesErrorPermission,
      SourceErrorCode.unreachable => l.sourcesErrorUnreachable,
      SourceErrorCode.timedOut => l.sourcesErrorTimedOut,
      SourceErrorCode.pathNotFound => l.sourcesErrorPathNotFound,
      SourceErrorCode.notADirectory => l.sourcesErrorNotADirectory,
      SourceErrorCode.invalidPath => l.sourcesErrorInvalidPath,
      SourceErrorCode.unsupported => l.sourcesErrorUnsupported,
      SourceErrorCode.notIndexable => l.sourcesErrorNotIndexable,
      SourceErrorCode.other => l.sourcesErrorOther,
    };

String sourceFormErrorText(AppLocalizations l, SourceFormError e) =>
    switch (e) {
      SourceFormError.folderEmpty => l.addSourceErrorNoFolder,
      SourceFormError.folderNotAbsolute => l.sourcesFormErrorFolderNotAbsolute,
      SourceFormError.hostEmpty => l.sourcesFormErrorHostEmpty,
      SourceFormError.shareEmpty => l.sourcesFormErrorShareEmpty,
      SourceFormError.urlEmpty => l.sourcesFormErrorUrlEmpty,
      SourceFormError.urlScheme => l.sourcesFormErrorUrlScheme,
      SourceFormError.credentialsInAddress => l.sourcesFormErrorCredentials,
      SourceFormError.tlsPin => l.sourcesFormErrorTlsPin,
    };

/// `12.3 MB/s` or `850 KB/s` in the UI language's number format.
String formatSpeed(AppLocalizations l, int bytesPerSec) {
  final f = NumberFormat.decimalPatternDigits(
    locale: l.localeName,
    decimalDigits: 1,
  );
  if (bytesPerSec >= 1000 * 1000) {
    return l.sourcesSpeedMBps(f.format(bytesPerSec / 1e6));
  }
  return l.sourcesSpeedKBps(f.format(bytesPerSec / 1e3));
}

/// "just now", "5 minutes ago", "2 days ago".
String formatAgo(AppLocalizations l, DateTime at, {DateTime? now}) {
  final d = (now ?? DateTime.now()).difference(at);
  if (d.inMinutes < 1) return l.sourcesAgoJustNow;
  if (d.inHours < 1) return l.sourcesAgoMinutes(d.inMinutes);
  if (d.inDays < 1) return l.sourcesAgoHours(d.inHours);
  return l.sourcesAgoDays(d.inDays);
}

/// Speed below which 4K remuxes may stutter (~100 Mbit/s).
const slowFor4kBytesPerSec = 12 * 1000 * 1000;
