// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:path/path.dart' as p;

enum MediaType { video, audio }

const Set<String> videoExtensions = {
  '.mkv',
  '.mp4',
  '.m4v',
  '.avi',
  '.mov',
  '.wmv',
  '.mpg',
  '.mpeg',
  '.ts',
  '.m2ts',
  '.mts',
  '.webm',
  '.flv',
  '.ogv',
  '.3gp',
  '.rmvb',
  '.divx',
  '.asf',
};

const Set<String> audioExtensions = {
  '.mp3',
  '.flac',
  '.m4a',
  '.aac',
  '.ogg',
  '.oga',
  '.opus',
  '.wav',
  '.wma',
  '.aiff',
  '.aif',
  '.ape',
  '.wv',
  '.dsf',
  '.dff',
  '.mka',
  '.alac',
};

/// Folder names skipped entirely (lower case).
const Set<String> ignoredFolders = {
  // extras
  'sample', 'samples', 'extras', 'extra', 'featurettes', 'featurette',
  'behind the scenes', 'behindthescenes', 'deleted scenes', 'deletedscenes',
  'interviews', 'interview', 'scenes', 'shorts', 'trailers', 'trailer',
  'other', 'others', 'bonus', 'bonuses', 'making of', 'subs', 'subtitles',
  'доп. материалы', 'доп материалы', 'дополнительные материалы', 'бонусы',
  'трейлеры', 'допы', 'субтитры',
  // disc structures (not supported in 0.1)
  'bdmv', 'video_ts', 'audio_ts', 'certificate',
  // NAS / OS service folders
  '@eadir',
  '#recycle',
  '#snapshot',
  r'$recycle.bin',
  'system volume information',
  'lost+found', '.@__thumb', '@recycle', '.trash', '.trashes',
};

final RegExp _extraFileRe = RegExp(
  r'^(?:sample|trailer|teaser)(?:[\s._\-]|$)'
  r'|[\s._\-\[(](?:sample|trailer|teaser)[\])]?$'
  r'|-(?:featurette|deleted|behindthescenes|interview|scene|short|other|extra)$',
  caseSensitive: false,
);

/// A media file found by the scanner.
class ScannedFile {
  final String path;
  final MediaType type;
  final int size;
  final DateTime modified;
  const ScannedFile(this.path, this.type, this.size, this.modified);

  @override
  String toString() => 'ScannedFile($path, ${type.name})';
}

/// Media type of [path] by extension, or null if it is not a media file.
MediaType? mediaTypeOf(String path) {
  final ext = p.extension(path).toLowerCase();
  if (videoExtensions.contains(ext)) return MediaType.video;
  if (audioExtensions.contains(ext)) return MediaType.audio;
  return null;
}

/// True for samples, trailers and Plex-style extras (`Movie-trailer.mkv`).
bool isExtraFile(String path) {
  final base = p.basenameWithoutExtension(path);
  return _extraFileRe.hasMatch(base);
}

bool isIgnoredFolder(String name) {
  final n = name.toLowerCase();
  if (n.startsWith('.')) return true;
  return ignoredFolders.contains(n);
}

/// Recursively walks [root] and yields media files. Follows symlinks but
/// guards against loops; unreadable folders are skipped (reported through
/// [onError]).
Stream<ScannedFile> scanFolder(
  String root, {
  void Function(String path, Object error)? onError,
  int maxDepth = 12,
}) async* {
  final visited = <String>{};
  final pending = <(String, int)>[(root, 0)];
  while (pending.isNotEmpty) {
    final (dirPath, depth) = pending.removeLast();
    String real;
    try {
      real = await Directory(dirPath).resolveSymbolicLinks();
    } on FileSystemException catch (e) {
      onError?.call(dirPath, e);
      continue;
    }
    if (!visited.add(real)) continue;

    final List<FileSystemEntity> entries;
    try {
      entries = await Directory(dirPath).list(followLinks: true).toList();
    } on FileSystemException catch (e) {
      onError?.call(dirPath, e);
      continue;
    }
    entries.sort((a, b) => b.path.compareTo(a.path)); // stable order (stack)
    for (final e in entries) {
      final name = p.basename(e.path);
      if (e is Directory) {
        if (depth + 1 > maxDepth || isIgnoredFolder(name)) continue;
        pending.add((e.path, depth + 1));
      } else if (e is File) {
        if (name.startsWith('.')) continue;
        final type = mediaTypeOf(name);
        if (type == null) continue;
        if (type == MediaType.video && isExtraFile(name)) continue;
        try {
          final st = await e.stat();
          yield ScannedFile(e.path, type, st.size, st.modified);
        } on FileSystemException catch (err) {
          onError?.call(e.path, err);
        }
      }
    }
  }
}
