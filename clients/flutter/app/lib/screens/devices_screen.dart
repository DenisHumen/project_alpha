// SPDX-License-Identifier: MPL-2.0
//
// Settings → Devices (UI-019): pair this device with the user's other
// devices by QR code or 6-digit code, see which are online, unpair. Paired
// devices sync progress, "Continue watching" and watched marks over the LAN.

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../l10n/l10n.dart';
import '../platform/form_factor.dart';
import '../services/app_services.dart';
import '../sync/device_sync.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/qr_code.dart';
import 'qr_scan_screen.dart';

class DevicesScreen extends StatefulWidget {
  const DevicesScreen({super.key, this.scanQr});

  /// Opens the camera and returns a scanned code; tests replace it.
  final Future<String?> Function(BuildContext context)? scanQr;

  @override
  State<DevicesScreen> createState() => _DevicesScreenState();
}

class _DevicesScreenState extends State<DevicesScreen> {
  final _code = TextEditingController();
  SyncInvite? _invite;
  bool _busy = false;
  DeviceSync? _sync;
  StreamSubscription<SyncEvent>? _sub;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync ??= Services.read(context).sync;
    _sub ??= _sync!.events.listen(_onEvent);
  }

  @override
  void dispose() {
    unawaited(_sub?.cancel());
    // Leaving the screen closes an open invitation.
    if (_invite != null) unawaited(_sync?.cancelInvite());
    _code.dispose();
    super.dispose();
  }

  void _onEvent(SyncEvent e) {
    if (!mounted) return;
    final l = context.l10n;
    switch (e.kind) {
      case SyncEventKind.paired:
        _snack(l.devicesPaired(e.name ?? ''));
        setState(() => _invite = null);
      case SyncEventKind.inviteClosed:
        setState(() => _invite = null);
      case SyncEventKind.pairingFailed:
        _snack(l.devicesPairingFailed);
      case SyncEventKind.devicesChanged:
        setState(() {});
      case SyncEventKind.libraryChanged ||
          SyncEventKind.playback ||
          SyncEventKind.takeover:
        break;
    }
  }

  void _snack(String text) => ScaffoldMessenger.of(context)
    ..removeCurrentSnackBar()
    ..showSnackBar(SnackBar(content: Text(text)));

  String _errorText(Object e) {
    final l = context.l10n;
    final code = e is SyncException ? e.code : SyncErrorCode.other;
    return switch (code) {
      SyncErrorCode.invalidCode => l.devicesErrorInvalidCode,
      SyncErrorCode.wrongCode => l.devicesErrorWrongCode,
      SyncErrorCode.expired => l.devicesErrorExpired,
      SyncErrorCode.notFound => l.devicesErrorNotFound,
      SyncErrorCode.alreadyPaired => l.devicesErrorAlreadyPaired,
      SyncErrorCode.refused => l.devicesErrorRefused,
      SyncErrorCode.unavailable => l.devicesUnavailable(kAppName),
      SyncErrorCode.other => l.devicesErrorOther,
    };
  }

  Future<void> _run(Future<void> Function() action) async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await action();
    } catch (e) {
      if (mounted) _snack(_errorText(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _showCode() => _run(() async {
    final invite = await Services.read(context).sync.invite();
    if (mounted) setState(() => _invite = invite);
  });

  Future<void> _hideCode() async {
    setState(() => _invite = null);
    await Services.read(context).sync.cancelInvite();
  }

  Future<void> _join(String input) => _run(() async {
    final services = Services.read(context);
    final l = context.l10n;
    final host = await services.sync.join(input);
    _code.clear();
    if (mounted) _snack(l.devicesJoined(host.name));
  });

  Future<void> _scan() async {
    final scan = widget.scanQr;
    final text = scan != null
        ? await scan(context)
        : await Navigator.of(context).push<String>(
            MaterialPageRoute(builder: (_) => const QrScanScreen()),
          );
    if (text != null && text.isNotEmpty && mounted) await _join(text);
  }

  Future<void> _unpair(SyncDevice d) async {
    final l = context.l10n;
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(
          d.thisDevice ? l.devicesLeaveTitle : l.devicesUnpairTitle(d.name),
        ),
        content: Text(
          d.thisDevice ? l.devicesLeaveBody : l.devicesUnpairBody(d.name),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: Text(l.commonCancel),
          ),
          TextButton(
            onPressed: () => Navigator.pop(c, true),
            child: Text(d.thisDevice ? l.devicesLeave : l.devicesUnpair),
          ),
        ],
      ),
    );
    if (ok == true && mounted) {
      await _run(() => Services.read(context).sync.unpair(d.id));
    }
  }

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final sync = services.sync;
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    final devices = sync.devices();
    final me = devices.where((x) => x.thisDevice).firstOrNull;
    final others = devices.where((x) => !x.thisDevice).toList();
    final formFactor = FormFactorScope.of(context);
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 90),
          children: [
            if (!d.isTv)
              Padding(
                padding: EdgeInsets.only(left: d.pageInsets.left - 8, top: 8),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: IconButton(
                    tooltip: l.commonBack,
                    onPressed: () => Navigator.of(context).maybePop(),
                    icon: const Icon(Icons.arrow_back),
                  ),
                ),
              ),
            PageTitle(l.devicesTitle),
            Padding(
              padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
              child: Align(
                alignment: Alignment.topLeft,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 760),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Padding(
                        padding: const EdgeInsets.only(bottom: 20),
                        child: Text(
                          l.devicesSyncNote,
                          style: theme.textTheme.bodyMedium,
                        ),
                      ),
                      if (!sync.available)
                        _Panel(
                          title: l.devicesTitle,
                          children: [Text(l.devicesUnavailable(kAppName))],
                        )
                      else ...[
                        _Panel(
                          title: l.devicesPairedTitle,
                          children: [
                            if (others.isEmpty)
                              Padding(
                                padding: const EdgeInsets.only(bottom: 8),
                                child: Text(
                                  l.devicesNone,
                                  style: theme.textTheme.bodySmall,
                                ),
                              ),
                            if (me != null && others.isNotEmpty)
                              _DeviceTile(
                                device: me,
                                onUnpair: _busy ? null : () => _unpair(me),
                              ),
                            for (final x in others)
                              _DeviceTile(
                                device: x,
                                onUnpair: _busy ? null : () => _unpair(x),
                              ),
                          ],
                        ),
                        _Panel(
                          title: l.devicesAddTitle,
                          subtitle: l.devicesAddBody,
                          children: [
                            if (_invite case final invite?)
                              _InviteView(invite: invite, onHide: _hideCode)
                            else
                              Align(
                                alignment: Alignment.centerLeft,
                                child: KButton(
                                  label: l.devicesShowCode,
                                  icon: Icons.qr_code_2,
                                  primary: others.isEmpty,
                                  onPressed: _busy ? null : _showCode,
                                ),
                              ),
                          ],
                        ),
                        _Panel(
                          title: l.devicesJoinTitle,
                          subtitle: l.devicesJoinBody,
                          children: [
                            Wrap(
                              spacing: 12,
                              runSpacing: 12,
                              crossAxisAlignment: WrapCrossAlignment.center,
                              children: [
                                SizedBox(
                                  width: 220,
                                  child: TextField(
                                    key: const ValueKey('devices-code'),
                                    controller: _code,
                                    keyboardType: TextInputType.number,
                                    maxLength: 7,
                                    decoration: InputDecoration(
                                      labelText: l.devicesCodeField,
                                      counterText: '',
                                    ),
                                    onSubmitted: (v) => _join(v),
                                  ),
                                ),
                                KButton(
                                  label: l.devicesConnect,
                                  icon: Icons.link,
                                  onPressed: _busy
                                      ? null
                                      : () => _join(_code.text),
                                ),
                                if (canScanQr(formFactor) ||
                                    widget.scanQr != null)
                                  KButton(
                                    label: l.devicesScanQr,
                                    icon: Icons.qr_code_scanner,
                                    onPressed: _busy ? null : _scan,
                                  ),
                              ],
                            ),
                          ],
                        ),
                        if (me != null || sync.fingerprint.isNotEmpty)
                          Text(
                            l.devicesFingerprint(sync.fingerprint),
                            style: theme.textTheme.labelSmall,
                          ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Panel extends StatelessWidget {
  const _Panel({required this.title, required this.children, this.subtitle});

  final String title;
  final String? subtitle;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final d = KDensity.of(context);
    return Padding(
      padding: EdgeInsets.only(bottom: d.isTv ? 18 : 24),
      child: FocusTraversalGroup(
        child: Container(
          padding: const EdgeInsets.all(18),
          decoration: BoxDecoration(
            color: KColors.bgElevated,
            borderRadius: BorderRadius.circular(d.panelRadius * .75),
            border: Border.all(color: KColors.stroke),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: theme.textTheme.titleLarge),
              if (subtitle != null) ...[
                const SizedBox(height: 2),
                Text(subtitle!, style: theme.textTheme.bodySmall),
              ],
              const SizedBox(height: 12),
              ...children,
            ],
          ),
        ),
      ),
    );
  }
}

class _InviteView extends StatelessWidget {
  const _InviteView({required this.invite, required this.onHide});

  final SyncInvite invite;
  final VoidCallback onHide;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final until = MaterialLocalizations.of(context)
        .formatTimeOfDay(TimeOfDay.fromDateTime(invite.expiresAt));
    final code = '${invite.code.substring(0, 3)} ${invite.code.substring(3)}';
    final details = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          code,
          key: const ValueKey('devices-invite-code'),
          style: theme.textTheme.displaySmall?.copyWith(
            letterSpacing: 4,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
        ),
        const SizedBox(height: 6),
        Text(l.devicesCodeHint(until), style: theme.textTheme.bodySmall),
        const SizedBox(height: 14),
        KButton(label: l.devicesHideCode, icon: Icons.close, onPressed: onHide),
      ],
    );
    return LayoutBuilder(
      builder: (context, c) {
        final qr = QrCodeView(
          data: invite.qr,
          size: c.maxWidth < 420 ? 180 : 220,
        );
        if (c.maxWidth < 420) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [qr, const SizedBox(height: 16), details],
          );
        }
        return Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            qr,
            const SizedBox(width: 24),
            Expanded(child: details),
          ],
        );
      },
    );
  }
}

class _DeviceTile extends StatelessWidget {
  const _DeviceTile({required this.device, this.onUnpair});

  final SyncDevice device;
  final VoidCallback? onUnpair;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final icon = switch (device.kind) {
      SyncDeviceKind.phone => Icons.smartphone,
      SyncDeviceKind.tablet => Icons.tablet_mac,
      SyncDeviceKind.tv => Icons.tv,
      SyncDeviceKind.desktop => Icons.computer,
      SyncDeviceKind.server => Icons.dns,
    };
    final status = device.thisDevice
        ? l.devicesThisDeviceTag
        : (device.online ? l.devicesOnline : l.devicesOffline);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          Icon(icon, color: KColors.textSecondary),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  device.name,
                  style: theme.textTheme.titleMedium,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                Row(
                  children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: device.online || device.thisDevice
                            ? const Color(0xFF3FB950)
                            : KColors.textTertiary,
                      ),
                    ),
                    const SizedBox(width: 6),
                    Flexible(
                      child: Text(
                        status,
                        style: theme.textTheme.labelSmall,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
          KButton(
            label: device.thisDevice ? l.devicesLeave : l.devicesUnpair,
            onPressed: onUnpair,
          ),
        ],
      ),
    );
  }
}
