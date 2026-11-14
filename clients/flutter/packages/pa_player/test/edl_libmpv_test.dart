// SPDX-License-Identifier: MPL-2.0
//
// LIB-020 "done when": a movie split over two files plays as one timeline on
// real libmpv through media_kit, from local paths and from HTTP URLs (the
// loopback stream proxy of network sources, ADR-0038): the duration is the
// sum of the parts, and a seek into part 2 lands at the right position on
// the right frame.
//
// Each test movie is 12 s of solid colour blocks, 3 s each: red, green
// (part 1), blue, white (part 2). Two layouts:
//   - `cd1`/`cd2`: two AVI files (MPEG-4 Part 2), each starting at 0;
//   - DVD: one MPEG-PS stream cut by bytes on a 2048-byte sector into
//     `VTS_01_1.VOB` and `VTS_01_2.VOB`, as on a disc: part 2 starts mid-
//     stream with timestamps that continue part 1.
//
// Needs libmpv and an ffmpeg to make the media, so it is skipped unless
// PA_LIBMPV_TESTS=1 and PA_TEST_FFMPEG are set:
//   PA_LIBMPV_TESTS=1 PA_TEST_FFMPEG=<ffmpeg> \
//     LIBMPV_LIBRARY_PATH=<dir>/libmpv.so.2 \
//     flutter test test/edl_libmpv_test.dart
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';

final _enabled = Platform.environment['PA_LIBMPV_TESTS'] == '1';
final _ffmpeg = Platform.environment['PA_TEST_FFMPEG'];

/// Colour blocks of the movie, 3 s each, as ffmpeg names and as BGR.
const _blocks = ['red', 'lime', 'blue', 'white'];
const _block = 3.0;
const _total = 12.0;

Future<void> _ffmpegRun(List<String> args) async {
  final r = await Process.run(_ffmpeg!, ['-v', 'error', '-y', ...args]);
  if (r.exitCode != 0) fail('ffmpeg ${args.last}: ${r.stderr}');
}

/// ffmpeg inputs and filter for colour blocks [from]..[to) of the movie.
List<String> _colours(int from, int to) => [
  for (var i = from; i < to; i++) ...[
    '-f',
    'lavfi',
    '-i',
    'color=c=${_blocks[i]}:s=64x48:r=10:d=$_block',
  ],
  '-filter_complex',
  '${[for (var i = 0; i < to - from; i++) '[$i:v]'].join()}'
      'concat=n=${to - from}:v=1:a=0[v]',
  '-map',
  '[v]',
];

/// `cd1`/`cd2`: two files, two blocks each.
Future<List<String>> _cdParts(Directory dir) async {
  final out = <String>[];
  for (final (no, from) in [(1, 0), (2, 2)]) {
    final path = '${dir.path}/Heat.1995.cd$no.avi';
    await _ffmpegRun([
      ..._colours(from, from + 2),
      '-c:v',
      'mpeg4',
      '-q:v',
      '3',
      '-g',
      '10',
      path,
    ]);
    out.add(path);
  }
  return out;
}

/// DVD: one MPEG-PS of all blocks, cut in two on a sector boundary near the
/// middle (VOBs are split by size, not on frames).
Future<List<String>> _vobParts(Directory dir) async {
  final whole = '${dir.path}/whole.mpg';
  await _ffmpegRun([
    ..._colours(0, 4),
    '-c:v',
    'mpeg2video',
    '-q:v',
    '3',
    '-g',
    '10',
    '-f',
    'vob',
    whole,
  ]);
  final bytes = File(whole).readAsBytesSync();
  final cut = (bytes.length ~/ 2) ~/ 2048 * 2048;
  final one = '${dir.path}/VTS_01_1.VOB';
  final two = '${dir.path}/VTS_01_2.VOB';
  File(one).writeAsBytesSync(Uint8List.sublistView(bytes, 0, cut));
  File(two).writeAsBytesSync(Uint8List.sublistView(bytes, cut));
  return [one, two];
}

/// A minimal HTTP file server with single `Range` requests, like the
/// loopback stream proxy.
Future<HttpServer> _serve(Directory dir) async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((req) async {
    final res = req.response;
    final file = File('${dir.path}${Uri.decodeComponent(req.uri.path)}');
    if (!file.existsSync()) {
      res.statusCode = HttpStatus.notFound;
      await res.close();
      return;
    }
    final size = file.lengthSync();
    var start = 0, end = size - 1;
    final range = RegExp(
      r'^bytes=(\d*)-(\d*)$',
    ).firstMatch(req.headers.value(HttpHeaders.rangeHeader) ?? '');
    res.headers
      ..set(HttpHeaders.acceptRangesHeader, 'bytes')
      ..contentType = ContentType.binary;
    if (range != null) {
      if (range[1]!.isNotEmpty) start = int.parse(range[1]!);
      if (range[2]!.isNotEmpty) end = int.parse(range[2]!);
      if (range[1]!.isEmpty && range[2]!.isNotEmpty) {
        start = size - int.parse(range[2]!);
        end = size - 1;
      }
      if (end >= size) end = size - 1;
      if (start >= size) {
        res.statusCode = HttpStatus.requestedRangeNotSatisfiable;
        res.headers.set(HttpHeaders.contentRangeHeader, 'bytes */$size');
        await res.close();
        return;
      }
      res.statusCode = HttpStatus.partialContent;
      res.headers.set(
        HttpHeaders.contentRangeHeader,
        'bytes $start-$end/$size',
      );
    }
    res.contentLength = end - start + 1;
    try {
      if (req.method != 'HEAD') await res.addStream(file.openRead(start, end + 1));
      await res.close();
    } catch (_) {
      // The player closed the connection (seek): nothing to do.
    }
  });
  return server;
}

/// The colour block a frame shows: the index in [_blocks] whose colour
/// matches the centre pixel (BGR0), or -1.
int _blockOf(Uint8List raw, int width, int height) {
  final stride = raw.length ~/ height;
  final i = (height ~/ 2) * stride + (width ~/ 2) * 4;
  final (b, g, r) = (raw[i], raw[i + 1], raw[i + 2]);
  bool hi(int v) => v > 180;
  bool lo(int v) => v < 80;
  if (hi(r) && lo(g) && lo(b)) return 0;
  if (lo(r) && hi(g) && lo(b)) return 1;
  if (lo(r) && lo(g) && hi(b)) return 2;
  if (hi(r) && hi(g) && hi(b)) return 3;
  return -1;
}

Future<void> _until(
  bool Function() done,
  String what, {
  Duration timeout = const Duration(seconds: 20),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (!done()) {
    if (DateTime.now().isAfter(deadline)) fail('timed out: $what');
    await Future<void>.delayed(const Duration(milliseconds: 50));
  }
}

/// Opens [uri] paused on libmpv through media_kit and checks the common
/// timeline: duration, and seeks into each block of both parts.
Future<void> _checkTimeline(String uri) async {
  final engine = MediaKitEngine(
    // No VideoController (no texture in a test): decode to vo=null and
    // read frames with screenshot-raw.
    video: false,
    mpvOptions: const {
      'vid': 'auto',
      'vo': 'null',
      'ao': 'null',
      'hr-seek': 'yes',
    },
  );
  try {
    await engine.open([uri], play: false, autoLoadSubtitles: false);
    final state = engine.state;
    await _until(
      () =>
          state.value.error != null ||
          state.value.duration > Duration.zero && state.value.videoWidth > 0,
      'open $uri',
    );
    expect(state.value.error, isNull);
    final duration = state.value.duration.inMilliseconds / 1000;
    expect(duration, closeTo(_total, 0.6), reason: 'one timeline of 2 parts');

    final player = engine.player;
    // Part 2 first (the point of the test), then back into part 1.
    for (final at in [10.5, 7.5, 4.5, 1.5, 11.6]) {
      await engine.seek(Duration(milliseconds: (at * 1000).round()));
      final want = (at / _block).floor();
      var got = -1;
      var pos = -1.0;
      final deadline = DateTime.now().add(const Duration(seconds: 10));
      while (DateTime.now().isBefore(deadline)) {
        await Future<void>.delayed(const Duration(milliseconds: 150));
        pos = player.state.position.inMilliseconds / 1000;
        final raw = await player.screenshot(format: null);
        if (raw == null || raw.isEmpty) continue;
        got = _blockOf(raw, state.value.videoWidth, state.value.videoHeight);
        if (got == want && (pos - at).abs() < 0.5) break;
      }
      expect(pos, closeTo(at, 0.5), reason: 'position after seek to $at');
      expect(
        got,
        want,
        reason: 'frame at $at s: ${_blocks[want]}, not ${got < 0 ? '?' : _blocks[got]}',
      );
    }
  } finally {
    await engine.dispose();
  }
}

void main() {
  late Directory tmp;

  setUpAll(() {
    if (!_enabled) return;
    ensurePaPlayerInitialized();
  });
  setUp(() => tmp = Directory.systemTemp.createTempSync('pa_edl_'));
  tearDown(() => tmp.deleteSync(recursive: true));

  final skip = _enabled && _ffmpeg != null
      ? null
      : 'needs libmpv and ffmpeg: PA_LIBMPV_TESTS=1 PA_TEST_FFMPEG=<ffmpeg> '
            '(see the file header)';

  for (final (name, make) in [
    ('cd1/cd2 movie', _cdParts),
    ('DVD title set of two VOBs', _vobParts),
  ]) {
    test('$name plays as one timeline from local files', () async {
      final parts = await make(tmp);
      final uri = partsUri(parts);
      expect(uri, startsWith(edlScheme));
      await _checkTimeline(uri);
    }, skip: skip);

    test('$name plays as one timeline from HTTP URLs', () async {
      final parts = await make(tmp);
      final server = await _serve(tmp);
      try {
        final base = 'http://127.0.0.1:${server.port}';
        await _checkTimeline(
          partsUri([
            for (final p in parts)
              '$base/${Uri.encodeComponent(p.split('/').last)}',
          ]),
        );
      } finally {
        await server.close(force: true);
      }
    }, skip: skip);
  }
}
