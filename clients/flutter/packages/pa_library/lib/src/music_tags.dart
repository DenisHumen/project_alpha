// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:audio_metadata_reader/audio_metadata_reader.dart' as amr;
import 'package:path/path.dart' as p;

import 'parser/filename_parser.dart' show genericFolderNames;

/// Track information from tags, falling back to the path.
class TrackInfo {
  final String title;
  final String? artist;
  final String? album;
  final int? trackNumber;
  final int? year;
  final int? durationMs;
  const TrackInfo({
    required this.title,
    this.artist,
    this.album,
    this.trackNumber,
    this.year,
    this.durationMs,
  });

  @override
  String toString() => 'TrackInfo($artist / $album / $trackNumber $title)';
}

/// Reads tags (ID3, Vorbis comments, MP4 atoms) with the pure-Dart
/// `audio_metadata_reader` (MIT). Returns null if the file cannot be parsed.
TrackInfo? readTags(String path) {
  try {
    final m = amr.readMetadata(File(path), getImage: false);
    final title = m.title?.trim();
    if (title == null || title.isEmpty) {
      if ((m.artist ?? '').isEmpty && (m.album ?? '').isEmpty) {
        return m.duration == null
            ? null
            : TrackInfo(title: '', durationMs: m.duration!.inMilliseconds);
      }
    }
    final artist = _clean(m.artist) ?? _clean(m.albumArtist);
    return TrackInfo(
      title: title ?? '',
      artist: artist,
      album: _clean(m.album),
      trackNumber: m.trackNumber,
      year: m.year?.year,
      durationMs: m.duration?.inMilliseconds,
    );
  } catch (_) {
    return null;
  }
}

String? _clean(String? s) {
  final t = s?.replaceAll('\u0000', '').trim();
  return (t == null || t.isEmpty) ? null : t;
}

final RegExp _trackPrefix = RegExp(
  r'^(?:(\d{1,2})[-.])?(\d{1,3})[\s._-]+(?:-\s*)?',
);

/// Track info from the path: `Artist/Album/NN - Title.ext`,
/// `Artist - Album/NN. Title.ext`, `Artist - Title.ext`.
TrackInfo trackInfoFromPath(String path, {String? root}) {
  var base = p.basenameWithoutExtension(path).replaceAll('_', ' ').trim();
  int? number;
  final m = _trackPrefix.firstMatch(base);
  if (m != null && base.length > m.end) {
    number = int.parse(m.group(2)!);
    base = base.substring(m.end).trim();
  }
  String? artist;
  String? title = base;
  final dash = base.indexOf(' - ');
  if (dash > 0) {
    artist = base.substring(0, dash).trim();
    title = base.substring(dash + 3).trim();
  }

  String? album;
  final dir = p.dirname(path);
  bool usable(String d) {
    if (root != null && (p.equals(d, root) || !p.isWithin(root, d)))
      return false;
    final n = p.basename(d).toLowerCase();
    return n.isNotEmpty &&
        !genericFolderNames.contains(n) &&
        !RegExp(r'^(cd|disc|disk)\s*\d+$').hasMatch(n);
  }

  var albumDir = dir;
  if (RegExp(
    r'^(cd|disc|disk)\s*\d+$',
    caseSensitive: false,
  ).hasMatch(p.basename(dir))) {
    albumDir = p.dirname(dir);
  }
  if (usable(albumDir)) {
    var a = p.basename(albumDir);
    // "Artist - Album (Year)" folder.
    a = a.replaceAll(RegExp(r'\s*[\[(](19|20)\d\d[\])]\s*'), ' ').trim();
    a = a.replaceAll(RegExp(r'^(19|20)\d\d\s*[-.]\s*'), '').trim();
    final d = a.indexOf(' - ');
    if (d > 0) {
      artist ??= a.substring(0, d).trim();
      album = a.substring(d + 3).trim();
    } else {
      album = a;
      final artistDir = p.dirname(albumDir);
      if (artist == null && usable(artistDir)) artist = p.basename(artistDir);
    }
  }
  return TrackInfo(
    title: title,
    artist: artist,
    album: album,
    trackNumber: number,
  );
}

/// Merges tag info with the path fallback.
TrackInfo trackInfo(String path, {String? root}) {
  final fromPath = trackInfoFromPath(path, root: root);
  final tags = readTags(path);
  if (tags == null) return fromPath;
  return TrackInfo(
    title: tags.title.isNotEmpty ? tags.title : fromPath.title,
    artist: tags.artist ?? fromPath.artist,
    album: tags.album ?? fromPath.album,
    trackNumber: tags.trackNumber ?? fromPath.trackNumber,
    year: tags.year,
    durationMs: tags.durationMs,
  );
}

const _coverNames = ['cover', 'folder', 'front', 'album', 'albumart', 'poster'];

/// Looks for a local cover / poster image next to [mediaPath].
/// For videos also `<name>-poster.jpg` / `<name>.jpg`.
/// Generic names (`poster.jpg`, `folder.jpg`) are only used for videos when
/// [allowGeneric] is true (the folder holds a single movie).
String? findLocalImage(
  String mediaPath, {
  bool video = false,
  bool allowGeneric = true,
}) {
  final dir = Directory(p.dirname(mediaPath));
  List<FileSystemEntity> entries;
  try {
    entries = dir.listSync(followLinks: true);
  } on FileSystemException {
    return null;
  }
  final byName = <String, String>{};
  for (final e in entries) {
    if (e is! File) continue;
    final ext = p.extension(e.path).toLowerCase();
    if (ext != '.jpg' && ext != '.jpeg' && ext != '.png' && ext != '.webp')
      continue;
    byName[p.basenameWithoutExtension(e.path).toLowerCase()] = e.path;
  }
  if (byName.isEmpty) return null;
  final stem = p.basenameWithoutExtension(mediaPath).toLowerCase();
  final candidates = video
      ? [
          '$stem-poster',
          stem,
          if (allowGeneric) ...['poster', 'folder', 'cover'],
        ]
      : _coverNames;
  for (final c in candidates) {
    final f = byName[c];
    if (f != null) return Uri.file(f).toString();
  }
  return null;
}
