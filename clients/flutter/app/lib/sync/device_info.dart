// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:flutter/services.dart';

import '../platform/form_factor.dart';

const _channel = MethodChannel('app.kinolune/device');

/// The name other devices of the family see for this one (UI-019): the
/// model on Android ("Pixel 8"), the host name on desktops.
Future<String> syncDeviceName(FormFactor formFactor) async {
  if (Platform.isAndroid) {
    try {
      final model = await _channel.invokeMethod<String>('deviceName');
      if (model != null && model.trim().isNotEmpty) return model.trim();
    } on PlatformException {
      // fall through
    } on MissingPluginException {
      // fall through
    }
  }
  final host = Platform.localHostname.trim();
  if (host.isNotEmpty && host != 'localhost') return host;
  return switch (formFactor) {
    FormFactor.tv => 'Kinolune TV', // l10n-ignore
    FormFactor.phone => 'Kinolune phone', // l10n-ignore
    FormFactor.desktop => 'Kinolune desktop', // l10n-ignore
  };
}

/// Who holds the Android multicast lock now (LAN sync, discovery).
final _multicastHolders = <String>{};

/// Android drops multicast on Wi-Fi without a `MulticastLock`; mDNS needs
/// it while LAN sync runs (UI-019) and while sources are discovered
/// (NET-020). Each [holder] holds or releases its own claim; the platform
/// lock is held while anyone holds a claim. No-op elsewhere.
Future<void> holdMulticastLock(bool hold, {String holder = 'sync'}) async {
  final before = _multicastHolders.isNotEmpty;
  if (hold) {
    _multicastHolders.add(holder);
  } else {
    _multicastHolders.remove(holder);
  }
  final after = _multicastHolders.isNotEmpty;
  if (before == after || !Platform.isAndroid) return;
  try {
    await _channel.invokeMethod<void>('multicastLock', after);
  } on PlatformException {
    // Older builds: sync still works through the QR address.
  } on MissingPluginException {
    // ditto
  }
}
