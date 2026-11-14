// SPDX-License-Identifier: MPL-2.0
//
// Form input → source address (UI-005). Pure functions, so the form rules
// are tested without widgets. The core checks addresses again (`pa-sources`).

/// Why a form field is not acceptable; `sourceFormErrorText` gives the text.
enum SourceFormError {
  folderEmpty,
  folderNotAbsolute,
  hostEmpty,
  shareEmpty,
  urlEmpty,
  urlScheme,
  credentialsInAddress,
  tlsPin,
}

/// A built address, or why it cannot be built.
class AddressResult {
  const AddressResult.ok(String this.address) : error = null;
  const AddressResult.fail(SourceFormError this.error) : address = null;

  final String? address;
  final SourceFormError? error;
}

final _windowsPath = RegExp(r'^([A-Za-z]:[\\/]|\\\\)');

/// A local folder: an absolute path (POSIX, `C:\…` or `\\server\share`).
AddressResult localAddress(String input) {
  final path = input.trim();
  if (path.isEmpty) return const AddressResult.fail(SourceFormError.folderEmpty);
  if (!path.startsWith('/') && !_windowsPath.hasMatch(path)) {
    return const AddressResult.fail(SourceFormError.folderNotAbsolute);
  }
  return AddressResult.ok(path);
}

/// Parts of an `smb://host/share/folder` address.
class SmbParts {
  const SmbParts(this.host, this.share, [this.folder = '']);

  final String host;
  final String share;

  /// Path inside the share, without leading or trailing slashes.
  final String folder;
}

String _trimSlashes(String s) =>
    s.replaceAll('\\', '/').replaceAll(RegExp(r'^/+|/+$'), '');

/// Splits `smb://host/share/a/b`, `\\host\share\a` or `host/share/a`.
/// Null if there is no host.
SmbParts? splitSmb(String input) {
  var s = input.trim().replaceAll('\\', '/');
  if (s.toLowerCase().startsWith('smb://')) s = s.substring(6);
  s = _trimSlashes(s);
  if (s.isEmpty) return null;
  final parts = s.split('/').where((p) => p.isNotEmpty).toList();
  return SmbParts(
    parts.first,
    parts.length > 1 ? parts[1] : '',
    parts.length > 2 ? parts.sublist(2).join('/') : '',
  );
}

/// `smb://host/share[/folder]` from the form fields. A full address pasted
/// into [host] is split.
AddressResult smbAddress({
  required String host,
  required String share,
  String folder = '',
}) {
  var h = host.trim();
  var sh = _trimSlashes(share.trim());
  var f = _trimSlashes(folder.trim());
  if (h.contains('/') || h.contains('\\')) {
    final p = splitSmb(h);
    if (p != null) {
      h = p.host;
      if (sh.isEmpty) sh = p.share;
      if (f.isEmpty) f = p.folder;
    }
  }
  if (h.isEmpty) return const AddressResult.fail(SourceFormError.hostEmpty);
  if (h.contains('@')) {
    return const AddressResult.fail(SourceFormError.credentialsInAddress);
  }
  if (sh.isEmpty) return const AddressResult.fail(SourceFormError.shareEmpty);
  return AddressResult.ok(['smb://$h', sh, if (f.isNotEmpty) f].join('/'));
}

/// A WebDAV URL; `https://` is added when the scheme is missing.
AddressResult webdavAddress(String input) {
  var s = input.trim();
  if (s.isEmpty) return const AddressResult.fail(SourceFormError.urlEmpty);
  if (!s.contains('://')) s = 'https://$s';
  final uri = Uri.tryParse(s);
  if (uri == null || (uri.scheme != 'https' && uri.scheme != 'http')) {
    return const AddressResult.fail(SourceFormError.urlScheme);
  }
  if (uri.host.isEmpty) return const AddressResult.fail(SourceFormError.urlScheme);
  if (uri.userInfo.isNotEmpty) {
    return const AddressResult.fail(SourceFormError.credentialsInAddress);
  }
  return AddressResult.ok(s);
}

/// A certificate pin as `sha256:<64 hex>`; colons and spaces of a pasted
/// fingerprint are dropped. Empty input is no pin (null address).
AddressResult? tlsPin(String input) {
  var s = input.trim().toLowerCase();
  if (s.isEmpty) return null;
  if (s.startsWith('sha256:')) s = s.substring(7);
  s = s.replaceAll(RegExp(r'[\s:]'), '');
  if (!RegExp(r'^[0-9a-f]{64}$').hasMatch(s)) {
    return const AddressResult.fail(SourceFormError.tlsPin);
  }
  return AddressResult.ok('sha256:$s');
}

/// A display name when the user gave none: the last folder, or the share.
String suggestSourceName(String address) {
  final s = address.replaceAll('\\', '/');
  final uri = Uri.tryParse(s);
  if (uri != null && uri.hasScheme && uri.scheme != 'file' && s.contains('://')) {
    final segs = uri.pathSegments.where((p) => p.isNotEmpty).toList();
    return segs.isEmpty ? uri.host : segs.last;
  }
  final segs = s.split('/').where((p) => p.isNotEmpty).toList();
  return segs.isEmpty ? address : segs.last;
}
