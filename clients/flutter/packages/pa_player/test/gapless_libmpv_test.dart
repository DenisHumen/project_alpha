// SPDX-License-Identifier: MPL-2.0
//
// MUS-002 "done when": a gapless test album plays without pauses at the
// seams, checked sample by sample on real libmpv with the music profile.
//
// mpv writes what it would play to a WAV file (`ao=pcm`); the test compares it
// with the uncut signal. One audio output session for the whole album means
// no re-open of the device between tracks; equal samples mean no inserted
// silence, no lost or doubled samples (encoder delay and padding trimmed).
//
// Needs libmpv, so it is skipped unless PA_LIBMPV_TESTS=1:
//   PA_LIBMPV_TESTS=1 LIBMPV_LIBRARY_PATH=<dir>/libmpv.so.2 \
//     flutter test test/gapless_libmpv_test.dart
// PA_GAPLESS_FFMPEG=<ffmpeg> also encodes the album to FLAC, MP3, AAC and Opus.
import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';

const _channels = 2;

/// Track lengths in frames, deliberately not multiples of codec frame sizes
/// (1024 AAC, 1152 MP3, 960 Opus, 4096 FLAC).
const _lengths = [52919, 30871, 66187, 11028, 44100];

final _enabled = Platform.environment['PA_LIBMPV_TESTS'] == '1';
final _ffmpeg = Platform.environment['PA_GAPLESS_FFMPEG'];

/// The uncut album: two sines per channel, 16-bit quantised, interleaved.
Int16List _signal(int rate) {
  final frames = _lengths.reduce((a, b) => a + b);
  final out = Int16List(frames * _channels);
  for (var i = 0; i < frames; i++) {
    final t = i / rate;
    final l = 0.45 * sin(2 * pi * 440 * t) + 0.2 * sin(2 * pi * 1234.5 * t);
    final r = 0.5 * sin(2 * pi * 660 * t + 1);
    out[i * 2] = (l * 32767).round();
    out[i * 2 + 1] = (r * 32767).round();
  }
  return out;
}

void _writeWav(String path, Int16List samples, int rate) {
  final data = samples.buffer.asUint8List(
    samples.offsetInBytes,
    samples.lengthInBytes,
  );
  final h = ByteData(44);
  void tag(int at, String s) {
    for (var i = 0; i < 4; i++) {
      h.setUint8(at + i, s.codeUnitAt(i));
    }
  }

  tag(0, 'RIFF');
  h.setUint32(4, 36 + data.length, Endian.little);
  tag(8, 'WAVE');
  tag(12, 'fmt ');
  h.setUint32(16, 16, Endian.little);
  h.setUint16(20, 1, Endian.little); // PCM
  h.setUint16(22, _channels, Endian.little);
  h.setUint32(24, rate, Endian.little);
  h.setUint32(28, rate * _channels * 2, Endian.little);
  h.setUint16(32, _channels * 2, Endian.little);
  h.setUint16(34, 16, Endian.little);
  tag(36, 'data');
  h.setUint32(40, data.length, Endian.little);
  File(path).writeAsBytesSync([...h.buffer.asUint8List(), ...data]);
}

/// Reads 16-bit PCM or 32-bit float WAV as interleaved samples in -1..1.
/// The data size may be unset when the writer did not finalise the header.
Float64List _readWav(String path) {
  final b = File(path).readAsBytesSync();
  final d = ByteData.sublistView(b);
  var at = 12;
  var format = 1;
  var bits = 16;
  while (at + 8 <= b.length) {
    final id = String.fromCharCodes(b.sublist(at, at + 4));
    var size = d.getUint32(at + 4, Endian.little);
    if (id == 'fmt ') {
      format = d.getUint16(at + 8, Endian.little);
      bits = d.getUint16(at + 22, Endian.little);
      if (format == 0xFFFE) format = d.getUint16(at + 32, Endian.little);
    } else if (id == 'data') {
      final start = at + 8;
      if (size == 0 || start + size > b.length) size = b.length - start;
      final n = size ~/ (bits ~/ 8);
      final out = Float64List(n);
      for (var i = 0; i < n; i++) {
        out[i] = format == 3 && bits == 32
            ? d.getFloat32(start + i * 4, Endian.little)
            : d.getInt16(start + i * 2, Endian.little) / 32768;
      }
      return out;
    }
    at += 8 + size + (size & 1);
  }
  throw StateError('$path: no data chunk');
}

/// Plays [files] as one queue through libmpv and returns what it output.
Future<Float64List> _playThroughMpv(
  List<String> files,
  String outWav,
  int rate, {
  required AudioProfile profile,
}) async {
  final engine = MediaKitEngine(
    video: false,
    mpvOptions: {
      'ao': 'pcm',
      'ao-pcm-file': outWav,
      'ao-pcm-waveheader': 'yes',
      'audio-format': 's16',
      'audio-samplerate': '$rate',
      'audio-channels': 'stereo',
    },
  );
  try {
    await engine.setAudioProfile(profile);
    await engine.open(files);
    final deadline = DateTime.now().add(const Duration(seconds: 90));
    bool done() {
      final s = engine.state.value;
      return s.completed && s.playlistIndex == files.length - 1;
    }

    while (!done()) {
      final error = engine.state.value.error;
      if (error != null) fail('mpv: $error');
      if (DateTime.now().isAfter(deadline)) fail('playback did not finish');
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
  } finally {
    await engine.dispose(); // closes the audio output, finalises the WAV
  }
  return _readWav(outWav);
}

/// Sample-level comparison of the played album with the uncut signal.
class _SeamReport {
  _SeamReport(Float64List out, Int16List ref, List<int> seams) {
    lengthDiff = (out.length - ref.length) ~/ _channels;
    final n = min(out.length, ref.length);
    var run = 0;
    for (var i = 0; i < n; i++) {
      final r = ref[i] / 32768;
      final e = (out[i] - r).abs();
      maxError = max(maxError, e);
      final frame = i ~/ _channels;
      if (seams.any((s) => (frame - s).abs() <= 2048)) {
        seamError = max(seamError, e);
      }
      // A pause: output near silence where the source is loud.
      if (i % _channels == 0) {
        run = (r.abs() > 0.1 && out[i].abs() < 0.01) ? run + 1 : 0;
        longestDropout = max(longestDropout, run);
      }
    }
  }

  late final int lengthDiff;
  double maxError = 0;
  double seamError = 0;
  int longestDropout = 0;

  @override
  String toString() =>
      'length diff $lengthDiff frames, max error ${maxError.toStringAsFixed(5)}, '
      'max error near seams ${seamError.toStringAsFixed(5)}, '
      'longest dropout $longestDropout frames';
}

class _Format {
  const _Format(this.ext, this.rate, this.args, {this.lossless = false});
  final String ext;
  final int rate;
  final List<String> args;
  final bool lossless;
}

const _wav = _Format('wav', 44100, [], lossless: true);
const _encoded = [
  _Format('flac', 44100, ['-c:a', 'flac'], lossless: true),
  _Format('mp3', 44100, ['-c:a', 'libmp3lame', '-b:a', '256k']),
  _Format('m4a', 44100, ['-c:a', 'aac', '-b:a', '256k']),
  _Format('opus', 48000, ['-c:a', 'libopus', '-b:a', '192k']),
];

/// Cuts the signal into tracks of [_lengths] and encodes them as [f].
Future<({List<String> files, Int16List ref, List<int> seams})> _album(
  Directory dir,
  _Format f,
) async {
  final ref = _signal(f.rate);
  final files = <String>[];
  final seams = <int>[];
  var at = 0;
  for (var i = 0; i < _lengths.length; i++) {
    final wav = '${dir.path}/${f.ext}-${i + 1}.wav';
    _writeWav(
      wav,
      Int16List.sublistView(ref, at * _channels, (at + _lengths[i]) * _channels),
      f.rate,
    );
    at += _lengths[i];
    if (i < _lengths.length - 1) seams.add(at);
    if (f.ext == 'wav') {
      files.add(wav);
      continue;
    }
    final out = '${dir.path}/${f.ext}-${i + 1}.${f.ext}';
    final r = await Process.run(_ffmpeg!, [
      '-v',
      'error',
      '-y',
      '-i',
      wav,
      ...f.args,
      out,
    ]);
    if (r.exitCode != 0) fail('ffmpeg ${f.ext}: ${r.stderr}');
    files.add(out);
  }
  return (files: files, ref: ref, seams: seams);
}

void main() {
  late Directory tmp;

  setUpAll(() {
    if (!_enabled) return;
    ensurePaPlayerInitialized();
  });
  setUp(() => tmp = Directory.systemTemp.createTempSync('pa_gapless_'));
  tearDown(() => tmp.deleteSync(recursive: true));

  const skip = 'needs libmpv: PA_LIBMPV_TESTS=1 (see the file header)';

  for (final f in [_wav, if (_ffmpeg != null) ..._encoded]) {
    test(
      '${f.ext} album plays without gaps at the seams',
      () async {
        final a = await _album(tmp, f);
        final out = await _playThroughMpv(
          a.files,
          '${tmp.path}/out.wav',
          f.rate,
          profile: const AudioProfile(replayGain: ReplayGainMode.off),
        );
        final rep = _SeamReport(out, a.ref, a.seams);
        // ignore: avoid_print
        print('${f.ext}: $rep');
        expect(rep.lengthDiff, 0, reason: '$rep');
        expect(rep.longestDropout, lessThanOrEqualTo(2), reason: '$rep');
        if (f.lossless) {
          // Bit-exact up to one LSB of the s16 output.
          expect(rep.maxError, lessThanOrEqualTo(1.5 / 32768), reason: '$rep');
        } else {
          expect(rep.seamError, lessThan(0.05), reason: '$rep');
        }
      },
      skip: _enabled ? false : skip,
      timeout: const Timeout(Duration(minutes: 3)),
    );
  }

  test(
    'the check catches a non-gapless setup (control)',
    () async {
      final a = await _album(tmp, _wav);
      final out = await _playThroughMpv(
        a.files,
        '${tmp.path}/out.wav',
        _wav.rate,
        profile: const AudioProfile(
          gapless: false,
          replayGain: ReplayGainMode.off,
        ),
      );
      final rep = _SeamReport(out, a.ref, a.seams);
      // ignore: avoid_print
      print('gapless off: $rep');
      expect(
        rep.lengthDiff != 0 || rep.maxError > 1.5 / 32768,
        isTrue,
        reason: 'without gapless-audio the output is re-opened per track',
      );
    },
    skip: _enabled ? false : skip,
    timeout: const Timeout(Duration(minutes: 3)),
  );

  /// Output level relative to the source, measured on loud samples.
  double level(Float64List out, Int16List ref) {
    var sum = 0.0;
    var n = 0;
    for (var i = 0; i < min(out.length, ref.length); i++) {
      final r = ref[i] / 32768;
      if (r.abs() > 0.3) {
        sum += out[i] / r;
        n++;
      }
    }
    return sum / n;
  }

  test(
    'fallback gain applies to files without ReplayGain tags',
    () async {
      final a = await _album(tmp, _wav);
      final out = await _playThroughMpv(
        a.files.take(1).toList(),
        '${tmp.path}/out.wav',
        _wav.rate,
        profile: const AudioProfile(fallbackDb: -6.0206),
      );
      expect(level(out, a.ref), closeTo(0.5, 0.005), reason: '-6 dB = ½');
    },
    skip: _enabled ? false : skip,
    timeout: const Timeout(Duration(minutes: 3)),
  );

  test(
    'ReplayGain tags: track, album, pre-amp and clipping prevention',
    () async {
      final ref = _signal(44100);
      final wav = '${tmp.path}/one.wav';
      _writeWav(wav, Int16List.sublistView(ref, 0, 44100 * _channels), 44100);
      Future<String> tagged(String name, Map<String, String> tags) async {
        final out = '${tmp.path}/$name.flac';
        final r = await Process.run(_ffmpeg!, [
          '-v', 'error', '-y', '-i', wav, '-c:a', 'flac', //
          for (final e in tags.entries) ...['-metadata', '${e.key}=${e.value}'],
          out,
        ]);
        if (r.exitCode != 0) fail('ffmpeg: ${r.stderr}');
        return out;
      }

      final quiet = await tagged('quiet', {
        'REPLAYGAIN_TRACK_GAIN': '-6.02 dB',
        'REPLAYGAIN_TRACK_PEAK': '0.65',
        'REPLAYGAIN_ALBUM_GAIN': '-12.04 dB',
        'REPLAYGAIN_ALBUM_PEAK': '0.65',
      });
      final loud = await tagged('loud', {
        'REPLAYGAIN_TRACK_GAIN': '+12 dB',
        'REPLAYGAIN_TRACK_PEAK': '0.8',
      });
      Future<double> play(String f, AudioProfile p) async =>
          level(await _playThroughMpv([f], '${tmp.path}/o.wav', 44100, profile: p), ref);

      const track = AudioProfile(replayGain: ReplayGainMode.track);
      expect(await play(quiet, track), closeTo(0.5, 0.005));
      expect(
        await play(quiet, track.copyWith(replayGain: ReplayGainMode.album)),
        closeTo(0.25, 0.005),
      );
      expect(
        await play(quiet, track.copyWith(preampDb: 6.0206)),
        closeTo(1, 0.01),
      );
      // +12 dB would clip a 0.8 peak: the gain is capped at 1 / 0.8.
      expect(await play(loud, track), closeTo(1.25, 0.01));
    },
    skip: !_enabled
        ? skip
        : _ffmpeg == null
        ? 'needs PA_GAPLESS_FFMPEG to tag files'
        : false,
    timeout: const Timeout(Duration(minutes: 3)),
  );
}
