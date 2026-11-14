// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:flutter_zxing/flutter_zxing.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import '../platform/form_factor.dart';

/// Whether this device can scan a pairing QR code with its camera: phones
/// and tablets on Android and iOS. Desktops and TVs type the code (UI-019).
bool canScanQr(FormFactor formFactor) =>
    formFactor == FormFactor.phone && (Platform.isAndroid || Platform.isIOS);

/// Scans one QR code with the camera (ZXing C++ through flutter_zxing, no
/// Google services) and pops with its text.
class QrScanScreen extends StatefulWidget {
  const QrScanScreen({super.key});

  @override
  State<QrScanScreen> createState() => _QrScanScreenState();
}

class _QrScanScreenState extends State<QrScanScreen> {
  bool _done = false;

  void _onScan(Code code) {
    final text = code.text;
    if (_done || !code.isValid || text == null || text.isEmpty) return;
    _done = true;
    Navigator.of(context).pop(text);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        foregroundColor: Colors.white,
        title: Text(l.devicesScanTitle),
      ),
      body: Stack(
        children: [
          ReaderWidget(
            onScan: _onScan,
            codeFormat: Format.qrCode,
            showGallery: false,
            showToggleCamera: false,
            cropPercent: 0.7,
            tryInverted: true,
          ),
          Positioned(
            left: 24,
            right: 24,
            bottom: 32,
            child: Text(
              l.devicesScanHint,
              textAlign: TextAlign.center,
              style: const TextStyle(color: Colors.white, fontSize: 16),
            ),
          ),
        ],
      ),
    );
  }
}
