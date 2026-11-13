// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'osd.dart';

const playerViewType = 'pa_spike/player_view';

/// Variants C and D: the native player is a PlatformView inside the Flutter route, the OSD is regular Flutter UI.
class PlatformViewPlayer extends StatefulWidget {
  const PlatformViewPlayer({super.key, required this.args});

  final Map<String, Object?> args;

  @override
  State<PlatformViewPlayer> createState() => _PlatformViewPlayerState();
}

class _PlatformViewPlayerState extends State<PlatformViewPlayer> {
  MethodChannel? _channel;

  bool get _hybrid => widget.args['variant'] == 'platform_view_hc';

  void _created(int id) => setState(() => _channel = MethodChannel('pa_spike/player_view_$id'));

  @override
  Widget build(BuildContext context) => Scaffold(
    backgroundColor: Colors.black,
    body: Stack(
      fit: StackFit.expand,
      children: [
        if (_hybrid) _hybridComposition() else _textureLayer(),
        if (_channel != null) Osd(channel: _channel!, onClose: () => Navigator.of(context).pop()),
      ],
    ),
  );

  /// Hybrid Composition: the Android view (with its SurfaceView) is added to the real view hierarchy.
  Widget _hybridComposition() => PlatformViewLink(
    viewType: playerViewType,
    surfaceFactory: (context, controller) => AndroidViewSurface(
      controller: controller as AndroidViewController,
      gestureRecognizers: const <Factory<OneSequenceGestureRecognizer>>{},
      hitTestBehavior: PlatformViewHitTestBehavior.transparent,
    ),
    onCreatePlatformView: (params) {
      final controller = PlatformViewsService.initExpensiveAndroidView(
        id: params.id,
        viewType: playerViewType,
        layoutDirection: TextDirection.ltr,
        creationParams: widget.args,
        creationParamsCodec: const StandardMessageCodec(),
        onFocus: () => params.onFocusChanged(true),
      );
      controller
        ..addOnPlatformViewCreatedListener(params.onPlatformViewCreated)
        ..addOnPlatformViewCreatedListener(_created)
        ..create();
      return controller;
    },
  );

  /// Texture Layer Hybrid Composition (default AndroidView): the view is rendered into a Flutter texture.
  Widget _textureLayer() => AndroidView(
    viewType: playerViewType,
    creationParams: widget.args,
    creationParamsCodec: const StandardMessageCodec(),
    onPlatformViewCreated: _created,
  );
}
