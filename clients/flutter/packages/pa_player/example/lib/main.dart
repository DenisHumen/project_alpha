// SPDX-License-Identifier: MPL-2.0
//
// Manual test app for pa_player.
//
//   pa_player_example /path/movie.mkv            # video, fullscreen OSD
//   pa_player_example a.flac b.mp3 c.opus        # music queue + Now Playing
//   PA_START_MS=60000 pa_player_example movie.mkv   # resume
//   PA_SUBTITLE=/path/movie.en.srt ...               # external subtitle
//   PA_EXIT_AFTER=10 ...  # headless check: print PA_RESULT {json} and exit
//
// On Android (no argv) the start screen has a path field.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:pa_player/pa_player.dart';

const _audioExt = {
  '.mp3',
  '.flac',
  '.m4a',
  '.alac',
  '.opus',
  '.ogg',
  '.oga',
  '.aac',
  '.wav',
  '.aiff',
  '.aif',
  '.wv',
  '.ape',
  '.dsf',
};

bool isAudio(String path) {
  final p = path.toLowerCase();
  final dot = p.lastIndexOf('.');
  return dot >= 0 && _audioExt.contains(p.substring(dot));
}

void main(List<String> args) {
  WidgetsFlutterBinding.ensureInitialized();
  ensurePaPlayerInitialized();
  runApp(ExampleApp(files: args));
}

class ExampleApp extends StatefulWidget {
  const ExampleApp({super.key, required this.files});
  final List<String> files;

  @override
  State<ExampleApp> createState() => _ExampleAppState();
}

class _ExampleAppState extends State<ExampleApp> {
  final music = MusicPlayerController();
  final navKey = GlobalKey<NavigatorState>();
  final log = <String>[];
  int lastPos = 0;
  int lastDur = 0;
  bool finished = false;

  @override
  void initState() {
    super.initState();
    final exitAfter = int.tryParse(Platform.environment['PA_EXIT_AFTER'] ?? '');
    if (exitAfter != null) {
      Timer(Duration(seconds: exitAfter), _exitWithResult);
    }
    if (widget.files.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) => open(widget.files));
    }
  }

  void _exitWithResult() {
    final result = {
      'files': widget.files,
      'video_last_progress_ms': lastPos,
      'video_duration_ms': lastDur,
      'video_finished': finished,
      'music_track': music.current?.title,
      'music_position_ms': music.position.inMilliseconds,
      'music_duration_ms': music.duration.inMilliseconds,
      'music_playing': music.playing,
      'progress_events': log.length,
    };
    stdout.writeln('PA_RESULT ${jsonEncode(result)}');
    exit(0);
  }

  void open(List<String> files) {
    final nav = navKey.currentState!;
    if (files.every(isAudio)) {
      music.play([
        for (final f in files)
          PlayRequest(
            uri: f,
            title: f.split('/').last,
            artist: File(f).parent.path.split('/').last,
          ),
      ]);
      nav.push(
        MaterialPageRoute<void>(
          builder: (_) => NowPlayingScreen(controller: music),
        ),
      );
      return;
    }
    final f = files.first;
    nav.push(
      MaterialPageRoute<void>(
        builder: (_) => PlayerScreen(
          request: PlayRequest(
            uri: f,
            title: f.split('/').last,
            startPositionMs:
                int.tryParse(Platform.environment['PA_START_MS'] ?? '') ?? 0,
            subtitleUri: Platform.environment['PA_SUBTITLE'],
          ),
          onProgress: (p, d) {
            lastPos = p;
            lastDur = d;
            log.add('$p/$d');
            debugPrint('onProgress $p / $d');
          },
          onFinished: () {
            finished = true;
            debugPrint('onFinished');
          },
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'pa_player example',
      navigatorKey: navKey,
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark(
        useMaterial3: true,
      ).copyWith(scaffoldBackgroundColor: PaTokens.bg),
      home: _Home(onOpen: open, music: music),
    );
  }
}

class _Home extends StatefulWidget {
  const _Home({required this.onOpen, required this.music});
  final void Function(List<String>) onOpen;
  final MusicPlayerController music;

  @override
  State<_Home> createState() => _HomeState();
}

class _HomeState extends State<_Home> {
  final ctrl = TextEditingController(
    text: Platform.isAndroid ? '/sdcard/Movies/' : '',
  );

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('pa_player example')),
      bottomNavigationBar: NowPlayingBar(controller: widget.music),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              'File path(s) or URL, separated by ";" — audio files '
              'open in the music player, anything else in the video player.',
            ),
            const SizedBox(height: 12),
            TextField(
              controller: ctrl,
              autofocus: true,
              decoration: const InputDecoration(border: OutlineInputBorder()),
              onSubmitted: (_) => _go(),
            ),
            const SizedBox(height: 12),
            FilledButton(onPressed: _go, child: const Text('Play')),
          ],
        ),
      ),
    );
  }

  void _go() {
    final files = ctrl.text
        .split(';')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
    if (files.isNotEmpty) widget.onOpen(files);
  }
}
