// SPDX-License-Identifier: MPL-2.0
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'osd.dart';
import 'platform_view_player.dart';

/// Default sample: SDR H.264 MKV from the public Media3 test-media bucket (smoke test only).
const defaultUrl =
    'https://storage.googleapis.com/exoplayer-test-media-1/mkv/android-screens-lavf-56.36.100-aac-avc-main-1280x720.mkv';

/// Embedding variants compared by the spike (see README.md).
enum Variant {
  activityNativeOsd('activity_native_osd', 'A · Activity + native OSD'),
  activityFlutterOsd('activity_flutter_osd', 'B · Activity + Flutter OSD'),
  platformViewHc('platform_view_hc', 'C · PlatformView HC (SurfaceView)'),
  platformViewTlhc('platform_view_tlhc', 'D · PlatformView TLHC (TextureView)');

  const Variant(this.id, this.label);
  final String id;
  final String label;
  bool get isActivity => id.startsWith('activity_');
}

void main() => runApp(const SpikeApp());

/// Second Dart entrypoint: the OSD drawn by a transparent FlutterFragment above the native SurfaceView (variant B).
@pragma('vm:entry-point')
void osdMain() => runApp(
  const MaterialApp(
    debugShowCheckedModeBanner: false,
    color: Colors.transparent,
    home: Osd(channel: MethodChannel('pa_spike/osd'), transparent: true),
  ),
);

class SpikeApp extends StatelessWidget {
  const SpikeApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'PA player spike',
    theme: ThemeData(colorSchemeSeed: Colors.indigo, brightness: Brightness.dark),
    home: const HomePage(),
  );
}

class HomePage extends StatefulWidget {
  const HomePage({super.key, this.channel = const MethodChannel('pa_spike/main')});

  final MethodChannel channel;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final _url = TextEditingController(text: defaultUrl);
  String _afr = 'mode';
  bool _tunneling = false;
  int _settleMs = 0;

  Map<String, Object?> _args(Variant v) => {
    'url': _url.text.trim(),
    'variant': v.id,
    'afr': _afr,
    'tunneling': _tunneling,
    'settle_ms': _settleMs,
  };

  Future<void> _open(Variant v) async {
    if (v.isActivity) {
      await widget.channel.invokeMethod<void>('openActivity', _args(v));
    } else {
      await Navigator.of(context).push(
        MaterialPageRoute<void>(builder: (_) => PlatformViewPlayer(args: _args(v))),
      );
    }
  }

  Future<void> _deviceInfo() async {
    final info = await widget.channel.invokeMethod<Object?>('deviceInfo');
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('Device / display capabilities'),
        content: SingleChildScrollView(
          child: SelectableText(const JsonEncoder.withIndent('  ').convert(info)),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Android player embedding spike')),
    body: ListView(
      padding: const EdgeInsets.all(24),
      children: [
        TextField(
          controller: _url,
          decoration: const InputDecoration(labelText: 'Media URL (http/https/file)'),
        ),
        const SizedBox(height: 16),
        Wrap(
          spacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            const Text('AFR:'),
            for (final m in ['mode', 'surface', 'off'])
              ChoiceChip(label: Text(m), selected: _afr == m, onSelected: (_) => setState(() => _afr = m)),
            const SizedBox(width: 16),
            const Text('Tunneling'),
            Switch(value: _tunneling, onChanged: (v) => setState(() => _tunneling = v)),
            const SizedBox(width: 16),
            const Text('Settle after switch:'),
            for (final ms in [0, 1000, 2000])
              ChoiceChip(label: Text('$ms ms'), selected: _settleMs == ms, onSelected: (_) => setState(() => _settleMs = ms)),
          ],
        ),
        const SizedBox(height: 24),
        for (final (i, v) in Variant.values.indexed)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: FilledButton(autofocus: i == 0, onPressed: () => _open(v), child: Text(v.label)),
          ),
        OutlinedButton(onPressed: _deviceInfo, child: const Text('Device info')),
      ],
    ),
  );
}
