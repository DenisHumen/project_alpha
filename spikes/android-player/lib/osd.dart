// SPDX-License-Identifier: MPL-2.0
import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Minimal Flutter OSD used by variants B, C and D: D-pad focusable transport controls plus a live stats panel.
/// It talks to the native player only through [channel] (`toggle`, `seekBy`, `stats`, `close`).
class Osd extends StatefulWidget {
  const Osd({super.key, required this.channel, this.onClose, this.transparent = false});

  final MethodChannel channel;
  final VoidCallback? onClose;

  /// True when the OSD is a separate Flutter engine drawn above the native SurfaceView (variant B).
  final bool transparent;

  @override
  State<Osd> createState() => _OsdState();
}

class _OsdState extends State<Osd> {
  Timer? _timer;
  Map<Object?, Object?> _stats = const {};
  bool _showStats = true;

  @override
  void initState() {
    super.initState();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _poll());
  }

  Future<void> _poll() async {
    try {
      final stats = await widget.channel.invokeMethod<Map<Object?, Object?>>('stats');
      if (mounted && stats != null) setState(() => _stats = stats);
    } on MissingPluginException {
      // Native side not ready yet (or already disposed).
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _close() {
    if (widget.onClose != null) {
      widget.onClose!();
    } else {
      widget.channel.invokeMethod<void>('close');
    }
  }

  @override
  Widget build(BuildContext context) {
    final playing = _stats['is_playing'] == true;
    return Material(
      type: MaterialType.transparency,
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (_showStats) _statsPanel(),
            const Spacer(),
            Container(
              color: const Color(0x99000000),
              padding: const EdgeInsets.all(12),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  IconButton(onPressed: () => widget.channel.invokeMethod<void>('seekBy', -10000), icon: const Icon(Icons.replay_10)),
                  IconButton(
                    autofocus: true,
                    onPressed: () => widget.channel.invokeMethod<void>('toggle'),
                    icon: Icon(playing ? Icons.pause : Icons.play_arrow),
                  ),
                  IconButton(onPressed: () => widget.channel.invokeMethod<void>('seekBy', 10000), icon: const Icon(Icons.forward_10)),
                  IconButton(onPressed: () => setState(() => _showStats = !_showStats), icon: const Icon(Icons.info_outline)),
                  IconButton(onPressed: _close, icon: const Icon(Icons.close)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _statsPanel() => Container(
    margin: const EdgeInsets.all(16),
    padding: const EdgeInsets.all(8),
    color: const Color(0x99000000),
    child: Text(
      [
        for (final e in _stats.entries) '${e.key}: ${e.value}',
        if (widget.transparent) 'osd: separate engine (FlutterEngineGroup)',
      ].join('\n'),
      style: const TextStyle(fontFamily: 'monospace', fontSize: 12, color: Colors.white),
    ),
  );
}
