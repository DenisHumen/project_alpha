// SPDX-License-Identifier: MPL-2.0

// A movie split over several files (LIB-020: the VOBs of a DVD title set,
// `cd1`/`cd2` parts) plays as one timeline through an mpv EDL URI
// (https://mpv.io/manual/stable/#edl): mpv opens the parts as segments of
// one virtual file with a common duration and seeking across the parts,
// and makes a chapter of each part. media_kit passes a URI with a scheme it
// does not know (`edl://`) to libmpv unchanged (media_kit 1.2.6,
// `Media.normalizeURI`), so the app needs no access to the `mpv_handle`.

import 'dart:convert';

/// Scheme of an mpv EDL URI.
const edlScheme = 'edl://';

/// One URI that plays [parts] (local paths or URLs) back to back: the part
/// itself if there is only one, else an `edl://` URI. Each part is written
/// as `%<length in UTF-8 bytes>%<part>`, so any character in a path or URL
/// (`;`, `,`, `%`, newlines) is safe. No segment start or length is given:
/// mpv takes each part whole, from its first timestamp (a VOB of a DVD title
/// set does not start at 0).
String partsUri(List<String> parts) {
  if (parts.isEmpty) {
    throw ArgumentError.value(parts, 'parts', 'nothing to play');
  }
  if (parts.length == 1) return parts.single;
  return edlScheme + parts.map((p) => '%${utf8.encode(p).length}%$p').join(';');
}

/// The parts of an EDL URI made by [partsUri]; a plain URI is its own only
/// part. Null for a malformed EDL.
List<String>? partsOf(String uri) {
  if (!uri.startsWith(edlScheme)) return [uri];
  final bytes = utf8.encode(uri.substring(edlScheme.length));
  final out = <String>[];
  var i = 0;
  while (i < bytes.length) {
    if (bytes[i] != 0x25) return null; // '%'
    final end = bytes.indexOf(0x25, i + 1);
    if (end < 0) return null;
    final len = int.tryParse(ascii.decode(bytes.sublist(i + 1, end)));
    if (len == null || end + 1 + len > bytes.length) return null;
    out.add(utf8.decode(bytes.sublist(end + 1, end + 1 + len)));
    i = end + 1 + len;
    if (i < bytes.length) {
      if (bytes[i] != 0x3b) return null; // ';'
      i++;
    }
  }
  return out.isEmpty ? null : out;
}

/// File name of a played URI for the info panel: the last path segment,
/// percent-decoded where possible; for an EDL URI the name of its first
/// part and the number of parts (`Movie.cd1.avi (+1)`).
String mediaDisplayName(String uri) {
  final parts = partsOf(uri);
  if (parts == null) return 'EDL';
  final first = parts.first.split('/').last;
  String name;
  try {
    name = Uri.decodeFull(first);
  } on ArgumentError {
    name = first;
  } on FormatException {
    name = first;
  }
  return parts.length > 1 ? '$name (+${parts.length - 1})' : name;
}
