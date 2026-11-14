// SPDX-License-Identifier: MPL-2.0
import 'dart:io';

import 'package:flutter/material.dart';

import '../engine/player_engine.dart';
import '../l10n/pa_player_strings.dart';
import '../play_request.dart';
import '../ui/focus_button.dart';
import '../ui/tokens.dart';
import 'osd_controller.dart';

/// Playback speeds offered in the speed panel.
const List<double> kPlaybackSpeeds = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// Subtitle file extensions shown in the file browser.
const Set<String> kSubtitleExtensions = {
  '.srt',
  '.ass',
  '.ssa',
  '.vtt',
  '.sub',
  '.sup',
  '.idx',
  '.smi',
  '.txt',
};

String formatRate(double r) {
  var s = r.toStringAsFixed(2);
  s = s.replaceFirst(RegExp(r'\.?0+$'), '');
  return '$s×';
}

/// Right-hand glass panel: audio, subtitles, subtitle file, speed, info.
class OsdSidePanel extends StatelessWidget {
  const OsdSidePanel({
    super.key,
    required this.panel,
    required this.engine,
    required this.request,
    required this.osd,
    required this.onClose,
    this.onSelectSubtitle,
    this.onSubtitleFile,
    this.onSubtitleDelay,
  });

  final OsdPanel panel;
  final PlayerEngine engine;
  final PlayRequest request;
  final OsdController osd;
  final VoidCallback onClose;

  /// Picks a subtitle track (PlayerScreen remembers it for the series);
  /// default: straight to the engine.
  final Future<void> Function(MediaTrack track)? onSelectSubtitle;

  /// Loads a subtitle file picked in the browser.
  final Future<void> Function(String path)? onSubtitleFile;

  /// Sets the subtitle delay (PlayerScreen remembers it for the series).
  final Future<void> Function(Duration delay)? onSubtitleDelay;

  /// Step of the delay buttons.
  static const delayStep = Duration(milliseconds: 100);

  @override
  Widget build(BuildContext context) {
    final l = PaPlayerStrings.of(context);
    final title = switch (panel) {
      OsdPanel.audio => l.audio,
      OsdPanel.subtitles => l.subtitles,
      OsdPanel.subtitleFile => l.subtitleFile,
      OsdPanel.speed => l.playbackSpeed,
      OsdPanel.info => l.info,
      OsdPanel.none => '',
    };
    return Container(
      key: ValueKey('panel-${panel.name}'),
      // Fits a phone in portrait (360 px wide) next to the margins.
      width: MediaQuery.sizeOf(context).width < 428
          ? MediaQuery.sizeOf(context).width - 48
          : 380,
      margin: const EdgeInsets.all(24),
      padding: const EdgeInsets.fromLTRB(12, 16, 12, 12),
      decoration: BoxDecoration(
        color: PaTokens.glassStrong,
        borderRadius: BorderRadius.circular(PaTokens.radius),
      ),
      child: FocusScope(
        autofocus: true,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: Padding(
                    padding: const EdgeInsets.only(left: 14),
                    child: Text(
                      title,
                      style: const TextStyle(
                        color: PaTokens.text,
                        fontSize: 20,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                ),
                FocusIconButton(
                  icon: Icons.close_rounded,
                  tooltip: l.close,
                  size: 40,
                  onPressed: onClose,
                ),
              ],
            ),
            const SizedBox(height: 8),
            Expanded(
              child: ValueListenableBuilder<EngineState>(
                valueListenable: engine.state,
                builder: (context, s, _) => ListView(
                  children: switch (panel) {
                    OsdPanel.audio => _audio(s, l),
                    OsdPanel.subtitles => _subtitles(s, l),
                    OsdPanel.speed => _speed(s, l),
                    OsdPanel.info => _info(s, l),
                    OsdPanel.subtitleFile => [
                      SubtitleFileBrowser(
                        startDir: _dirOf(request.uri),
                        onPicked: (path) {
                          final add = onSubtitleFile;
                          add != null
                              ? add(path)
                              : engine.addExternalSubtitle(path);
                          onClose();
                        },
                      ),
                    ],
                    OsdPanel.none => const [],
                  },
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  List<Widget> _audio(EngineState s, PaPlayerStrings l) {
    if (s.audioTracks.isEmpty) {
      return [_Empty(l.noAudioTracks)];
    }
    return [
      for (final t in s.audioTracks)
        FocusListRow(
          key: ValueKey('audio-${t.id}'),
          title: l.trackLabel(t),
          selected: t == s.audioTrack,
          autofocus: t == s.audioTrack,
          onPressed: () {
            engine.setAudioTrack(t);
            onClose();
          },
        ),
    ];
  }

  void _selectSubtitle(MediaTrack t) {
    final select = onSelectSubtitle;
    select != null ? select(t) : engine.setSubtitleTrack(t);
  }

  void _setDelay(Duration d) {
    final set = onSubtitleDelay;
    set != null ? set(d) : engine.setSubtitleDelay(d);
  }

  static String? _tags(MediaTrack t, PaPlayerStrings l) {
    final tags = [
      if (t.forced) l.forcedTrack,
      if (t.hearingImpaired) l.sdhTrack,
      if (t.external) l.externalFile,
    ];
    return tags.isEmpty ? null : tags.join(' · ');
  }

  List<Widget> _subtitles(EngineState s, PaPlayerStrings l) {
    final off = s.subtitleTrack == null || s.subtitleTrack!.isOff;
    final delay = s.subtitleDelay;
    return [
      FocusListRow(
        key: const ValueKey('sub-off'),
        title: l.off,
        selected: off,
        autofocus: off,
        onPressed: () {
          _selectSubtitle(MediaTrack.off);
          onClose();
        },
      ),
      for (final t in s.subtitleTracks)
        FocusListRow(
          key: ValueKey('sub-${t.id}'),
          title: l.trackLabel(t),
          subtitle: _tags(t, l),
          selected: t == s.subtitleTrack,
          autofocus: t == s.subtitleTrack,
          onPressed: () {
            _selectSubtitle(t);
            onClose();
          },
        ),
      Padding(
        padding: const EdgeInsets.fromLTRB(14, 12, 14, 0),
        child: Text(
          l.subtitleDelay,
          style: const TextStyle(color: PaTokens.textSecondary, fontSize: 13),
        ),
      ),
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 4),
        child: Row(
          children: [
            FocusIconButton(
              key: const ValueKey('sub-delay-earlier'),
              icon: Icons.remove_rounded,
              tooltip: l.subtitlesEarlier,
              size: 44,
              onPressed: () => _setDelay(delay - delayStep),
            ),
            Expanded(
              child: Text(
                l.delayLabel(delay),
                key: const ValueKey('sub-delay-value'),
                textAlign: TextAlign.center,
                style: const TextStyle(color: PaTokens.text, fontSize: 15),
              ),
            ),
            FocusIconButton(
              key: const ValueKey('sub-delay-later'),
              icon: Icons.add_rounded,
              tooltip: l.subtitlesLater,
              size: 44,
              onPressed: () => _setDelay(delay + delayStep),
            ),
          ],
        ),
      ),
      if (delay != Duration.zero)
        FocusListRow(
          key: const ValueKey('sub-delay-reset'),
          title: l.resetDelay,
          leading: const Icon(Icons.restart_alt_rounded),
          onPressed: () => _setDelay(Duration.zero),
        ),
      FocusListRow(
        key: const ValueKey('sub-file'),
        title: l.loadFromFile,
        leading: const Icon(Icons.folder_open_rounded),
        onPressed: () => osd.openPanel(OsdPanel.subtitleFile),
      ),
    ];
  }

  List<Widget> _speed(EngineState s, PaPlayerStrings l) => [
    for (final r in kPlaybackSpeeds)
      FocusListRow(
        key: ValueKey('speed-$r'),
        title: r == 1.0 ? l.normalSpeed : formatRate(r),
        selected: (s.rate - r).abs() < 0.001,
        autofocus: (s.rate - r).abs() < 0.001,
        onPressed: () {
          engine.setRate(r);
          onClose();
        },
      ),
  ];

  List<Widget> _info(EngineState s, PaPlayerStrings l) {
    final rows = <String, String>{
      l.infoTitle: request.title,
      l.infoPosition: '${_fmt(s.position)} / ${_fmt(s.duration)}',
      l.speed: formatRate(s.rate),
      if (s.audioTrack != null) l.audio: l.trackLabel(s.audioTrack!),
      if (s.subtitleTrack != null) l.subtitles: l.trackLabel(s.subtitleTrack!),
      for (final e in s.info.entries) l.engineInfoKey(e.key): e.value,
    };
    return [
      for (final e in rows.entries)
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 6),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                width: 120,
                child: Text(
                  e.key,
                  style: const TextStyle(
                    color: PaTokens.textSecondary,
                    fontSize: 14,
                  ),
                ),
              ),
              Expanded(
                child: Text(
                  e.value,
                  style: const TextStyle(color: PaTokens.text, fontSize: 14),
                ),
              ),
            ],
          ),
        ),
    ];
  }

  static String _fmt(Duration d) => formatDuration(d);

  static String? _dirOf(String uri) {
    String? path;
    if (uri.startsWith('/')) {
      path = uri;
    } else if (uri.startsWith('file://')) {
      path = Uri.parse(uri).toFilePath();
    }
    if (path == null) return null;
    return File(path).parent.path;
  }
}

class _Empty extends StatelessWidget {
  const _Empty(this.text);
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.all(14),
    child: Text(
      text,
      style: const TextStyle(color: PaTokens.textSecondary, fontSize: 15),
    ),
  );
}

/// Minimal D-pad-friendly file browser for subtitle files. Starts in the
/// video's folder; no platform file-picker dependency (works on Android TV).
class SubtitleFileBrowser extends StatefulWidget {
  const SubtitleFileBrowser({super.key, this.startDir, required this.onPicked});

  final String? startDir;
  final ValueChanged<String> onPicked;

  @override
  State<SubtitleFileBrowser> createState() => _SubtitleFileBrowserState();
}

class _SubtitleFileBrowserState extends State<SubtitleFileBrowser> {
  late Directory _dir;
  List<FileSystemEntity> _entries = const [];
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    _dir = Directory(
      widget.startDir ??
          Platform.environment['HOME'] ??
          (Platform.isAndroid ? '/storage/emulated/0' : '/'),
    );
    _load();
  }

  void _load() {
    try {
      final all = _dir.listSync(followLinks: false)
        ..sort((a, b) => a.path.toLowerCase().compareTo(b.path.toLowerCase()));
      _entries = [
        ...all.whereType<Directory>().where((d) => !_name(d).startsWith('.')),
        ...all.whereType<File>().where((f) {
          final n = _name(f).toLowerCase();
          final dot = n.lastIndexOf('.');
          return dot > 0 && kSubtitleExtensions.contains(n.substring(dot));
        }),
      ];
      _failed = false;
    } catch (e) {
      _entries = const [];
      _failed = true;
    }
  }

  static String _name(FileSystemEntity e) =>
      e.uri.pathSegments.where((s) => s.isNotEmpty).lastOrNull ?? e.path;

  void _open(Directory d) => setState(() {
    _dir = d;
    _load();
  });

  @override
  Widget build(BuildContext context) {
    final parent = _dir.parent;
    final l = PaPlayerStrings.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
          child: Text(
            _dir.path,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(color: PaTokens.textSecondary, fontSize: 13),
          ),
        ),
        if (parent.path != _dir.path)
          FocusListRow(
            title: '..',
            leading: const Icon(Icons.arrow_upward_rounded),
            autofocus: _entries.isEmpty,
            onPressed: () => _open(parent),
          ),
        if (_failed) _Empty(l.cannotOpenFolder),
        if (!_failed && _entries.isEmpty) _Empty(l.noSubtitleFiles),
        for (var i = 0; i < _entries.length; i++)
          FocusListRow(
            key: ValueKey(_entries[i].path),
            title: _name(_entries[i]),
            autofocus: i == 0,
            leading: Icon(
              _entries[i] is Directory
                  ? Icons.folder_rounded
                  : Icons.subtitles_rounded,
            ),
            onPressed: () {
              final e = _entries[i];
              if (e is Directory) {
                _open(e);
              } else {
                widget.onPicked(e.path);
              }
            },
          ),
      ],
    );
  }
}
