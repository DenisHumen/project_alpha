// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import '../config.dart';

/// How the UI is laid out and navigated.
///
/// * [desktop] — sidebar, mouse + keyboard (hover acts as focus);
/// * [phone] — bottom tabs, touch;
/// * [tv] — top menu, D-pad focus with scale/glow (01 §4.2).
enum FormFactor { desktop, phone, tv }

const _deviceChannel = MethodChannel('app.kinolune/device');

/// Detects the form factor once at start-up. Overridable with
/// `--dart-define=KINOLUNE_FORM_FACTOR=tv|phone|desktop`.
Future<FormFactor> detectFormFactor() async {
  final forced = AppConfig.forcedFormFactor;
  if (forced != null) return forced;
  if (kIsWeb) return FormFactor.desktop;
  if (Platform.isAndroid) {
    try {
      final isTv =
          await _deviceChannel.invokeMethod<bool>('isTelevision') ?? false;
      if (isTv) return FormFactor.tv;
    } on PlatformException {
      // fall through
    } on MissingPluginException {
      // fall through
    }
    return FormFactor.phone;
  }
  if (Platform.isIOS) return FormFactor.phone;
  return FormFactor.desktop;
}

/// Provides the [FormFactor] to the tree. A phone/tablet with a wide window
/// gets the sidebar layout via [effectiveLayoutOf].
class FormFactorScope extends InheritedWidget {
  const FormFactorScope({
    super.key,
    required this.formFactor,
    required super.child,
  });

  final FormFactor formFactor;

  static FormFactor of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<FormFactorScope>()
          ?.formFactor ??
      FormFactor.desktop;

  @override
  bool updateShouldNotify(FormFactorScope oldWidget) =>
      oldWidget.formFactor != formFactor;
}
