// SPDX-License-Identifier: MPL-2.0
// PLAY-DSK-001 spike UI: a video surface with a Flutter OSD on top.
//
// Configuration comes from the environment so runs can be scripted:
//   SPIKE_FILE     media file to play (required)
//   SPIKE_MODE     texture | native            (default: texture)
//   SPIKE_SECONDS  auto-quit after N seconds and print SPIKE_RESULT (0 = never)
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

const _channel = MethodChannel('pa.spike/mpv');
const _warmup = Duration(seconds: 3);

void main() => runApp(const SpikeApp());

class SpikeApp extends StatelessWidget {
  const SpikeApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: ThemeData.dark(),
    home: const PlayerScreen(),
  );
}

class PlayerScreen extends StatefulWidget {
  const PlayerScreen({super.key});

  @override
  State<PlayerScreen> createState() => _PlayerScreenState();
}

class _PlayerScreenState extends State<PlayerScreen> {
  final _env = Platform.environment;
  late final String _mode = _env['SPIKE_MODE'] ?? 'texture';
  int? _textureId;
  String _status = 'opening…';
  String _lastKey = '—';
  Timer? _ticker;
  Map<String, dynamic>? _warm;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _open());
  }

  Future<void> _open() async {
    final file = _env['SPIKE_FILE'];
    if (file == null) {
      setState(() => _status = 'SPIKE_FILE is not set');
      return;
    }
    final view = View.of(context);
    final size = view.physicalSize;
    final id = await _channel.invokeMethod<int>('open', {
      'path': file,
      'mode': _mode,
      // Native child window covers the whole client area (physical pixels).
      'x': 0,
      'y': 0,
      'w': size.width.round(),
      'h': size.height.round(),
    });
    setState(() => _textureId = (id != null && id >= 0) ? id : null);

    _ticker = Timer.periodic(const Duration(milliseconds: 500), (_) async {
      final s = await _stats();
      if (mounted) {
        setState(
          () => _status =
              '${s['time-pos']?.toStringAsFixed(1) ?? '-'} s · vo=${s['current-vo']} '
              '· hwdec=${s['hwdec-current']} · drops=${s['frame-drop-count']}',
        );
      }
    });

    Timer(_warmup, () async => _warm = await _stats());
    final seconds = int.tryParse(_env['SPIKE_SECONDS'] ?? '0') ?? 0;
    if (seconds > 0) {
      Timer(Duration(seconds: seconds), _finish);
    }
  }

  Future<Map<String, dynamic>> _stats() async =>
      Map<String, dynamic>.from(await _channel.invokeMethod('stats') as Map);

  Future<void> _finish() async {
    final end = await _stats();
    final warm = _warm ?? {};
    double d(String k) =>
        ((end[k] as num?) ?? 0).toDouble() -
        ((warm[k] as num?) ?? 0).toDouble();
    final wall = d('wall_s');
    final result = {
      ...end,
      'window_s': wall,
      'cpu_percent_one_core': wall > 0 ? 100 * d('process_cpu_s') / wall : null,
      'frames_rendered_in_window': d('frames_rendered'),
      'populate_calls_in_window': d('populate_calls'),
      'drops_in_window': d('frame-drop-count'),
      'decoder_drops_in_window': d('decoder-frame-drop-count'),
      'last_key': _lastKey,
    };
    // mpv reports NaN for some properties (e.g. avsync before sync starts).
    final clean = result.map(
      (k, v) => MapEntry(k, v is double && !v.isFinite ? null : v),
    );
    stdout.writeln('SPIKE_RESULT ${jsonEncode(clean)}');
    await stdout.flush();
    exit(0);
  }

  @override
  void dispose() {
    _ticker?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final video = _textureId == null
        ? const ColoredBox(color: Colors.transparent)
        : Center(
            child: AspectRatio(
              aspectRatio: 16 / 9,
              child: Texture(textureId: _textureId!),
            ),
          );
    return Focus(
      autofocus: true,
      onKeyEvent: (node, event) {
        if (event is KeyDownEvent) {
          setState(() => _lastKey = event.logicalKey.keyLabel);
          if (event.logicalKey == LogicalKeyboardKey.space) {
            _channel.invokeMethod('command', {
              'args': ['cycle', 'pause'],
            });
          }
        }
        return KeyEventResult.handled;
      },
      child: Scaffold(
        backgroundColor: Colors.black,
        body: Stack(
          children: [
            Positioned.fill(child: video),
            // The OSD: if it is visible over the picture, Flutter can overlay
            // this surface type.
            Positioned(
              left: 24,
              right: 24,
              bottom: 24,
              child: Container(
                padding: const EdgeInsets.all(12),
                color: const Color(0xCCB00020),
                child: Text(
                  'FLUTTER OSD · mode=$_mode · $_status · key=$_lastKey',
                  style: const TextStyle(fontSize: 18, color: Colors.white),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
