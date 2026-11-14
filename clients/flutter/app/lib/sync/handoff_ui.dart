// SPDX-License-Identifier: MPL-2.0
//
// UI of the handoff between devices (SYNC-003): the toasts over the player
// ("Jump to 41:11?", "Paused: continues on TV") and the Home banner
// "Continue on this device".

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../screens/playback.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'device_sync.dart';
import 'handoff.dart';

/// Wraps the player of one playback session: shows the handoff toasts,
/// sends a heartbeat when the app goes to the background and ends the
/// session when the player closes.
class HandoffScope extends StatefulWidget {
  const HandoffScope({
    super.key,
    required this.controller,
    required this.child,
    this.offerTimeout = const Duration(seconds: 12),
    this.noticeTimeout = const Duration(seconds: 4),
  });

  final HandoffController controller;
  final Widget child;

  /// The offer hides by itself after this long (without declining it).
  final Duration offerTimeout;

  /// "Paused: continues on …" stays this long.
  final Duration noticeTimeout;

  @override
  State<HandoffScope> createState() => _HandoffScopeState();
}

class _HandoffScopeState extends State<HandoffScope>
    with WidgetsBindingObserver {
  Timer? _offerTimer;
  Timer? _noticeTimer;

  HandoffController get _c => widget.controller;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _c.offer.addListener(_onOffer);
    _c.pausedFor.addListener(_onNotice);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      _c.beatNow();
    }
  }

  void _onOffer() {
    _offerTimer?.cancel();
    if (_c.offer.value != null) {
      _offerTimer = Timer(widget.offerTimeout, () => _c.offer.value = null);
    }
  }

  void _onNotice() {
    _noticeTimer?.cancel();
    if (_c.pausedFor.value != null) {
      _noticeTimer = Timer(
        widget.noticeTimeout,
        () => _c.pausedFor.value = null,
      );
    }
  }

  @override
  void dispose() {
    _offerTimer?.cancel();
    _noticeTimer?.cancel();
    _c.offer.removeListener(_onOffer);
    _c.pausedFor.removeListener(_onNotice);
    WidgetsBinding.instance.removeObserver(this);
    // The player (a child) has made its last progress report already.
    _c.end();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final top = MediaQuery.paddingOf(context).top + 16;
    return Stack(
      fit: StackFit.expand,
      children: [
        widget.child,
        Positioned(
          top: top,
          left: 16,
          right: 16,
          child: Align(
            alignment: Alignment.topCenter,
            child: ValueListenableBuilder<HandoffOffer?>(
              valueListenable: _c.offer,
              builder: (context, offer, _) {
                if (offer != null) {
                  return HandoffToast(
                    key: const ValueKey('handoff-offer'),
                    title: l.devicesHandoffJumpTo(
                      formatClock(offer.positionMs),
                    ),
                    subtitle: l.devicesHandoffAhead(
                      offer.deviceName ?? l.devicesHandoffAnotherDevice,
                    ),
                    action: l.devicesHandoffJump,
                    onAction: _c.acceptOffer,
                    onClose: _c.dismissOffer,
                  );
                }
                return ValueListenableBuilder<String?>(
                  valueListenable: _c.pausedFor,
                  builder: (context, device, _) => device == null
                      ? const SizedBox.shrink()
                      : HandoffToast(
                          key: const ValueKey('handoff-paused'),
                          title: l.devicesHandoffPausedHere(device),
                        ),
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

/// A compact dark toast over the video.
class HandoffToast extends StatelessWidget {
  const HandoffToast({
    super.key,
    required this.title,
    this.subtitle,
    this.action,
    this.onAction,
    this.onClose,
  });

  final String title;
  final String? subtitle;
  final String? action;
  final VoidCallback? onAction;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 520),
      child: Material(
        color: const Color(0xE6101014),
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 10, 8, 10),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.devices, color: Colors.white70, size: 20),
              const SizedBox(width: 12),
              Flexible(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleSmall?.copyWith(
                        color: Colors.white,
                      ),
                    ),
                    if (subtitle != null)
                      Text(
                        subtitle!,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: Colors.white70,
                        ),
                      ),
                  ],
                ),
              ),
              if (action != null) ...[
                const SizedBox(width: 8),
                TextButton(
                  key: const ValueKey('handoff-action'),
                  onPressed: onAction,
                  child: Text(action!),
                ),
              ],
              if (onClose != null)
                IconButton(
                  key: const ValueKey('handoff-close'),
                  tooltip: MaterialLocalizations.of(context).closeButtonTooltip,
                  onPressed: onClose,
                  icon: const Icon(Icons.close, color: Colors.white70),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Home banner: a title another paired device plays right now, with
/// "Continue on this device". Hidden when nothing plays elsewhere, the
/// title is not in this library or it already plays here.
class HandoffBanner extends StatelessWidget {
  const HandoffBanner({super.key});

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final handoff = services.handoff;
    return ListenableBuilder(
      listenable: handoff,
      builder: (context, _) {
        final entry = handoff.elsewhere
            .where((e) => e.itemId != null && e.itemId != handoff.playingItemId)
            .firstOrNull;
        if (entry == null) return const SizedBox.shrink();
        return FutureBuilder<LibraryItem?>(
          key: ValueKey('handoff-${entry.itemId}'),
          future: services.library.item(entry.itemId!),
          builder: (context, snap) {
            final item = snap.data;
            if (item == null) return const SizedBox.shrink();
            return _BannerCard(entry: entry, item: item);
          },
        );
      },
    );
  }
}

class _BannerCard extends StatelessWidget {
  const _BannerCard({required this.entry, required this.item});

  final RemotePlayback entry;
  final LibraryItem item;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    final device = entry.deviceName ?? l.devicesHandoffAnotherDevice;
    final title = item.kind == ItemKind.episode
        ? '${episodeCode(l, item)}  ${item.title}'
        : item.title;
    final button = KButton(
      key: const ValueKey('handoff-continue'),
      primary: true,
      icon: Icons.play_arrow_rounded,
      label: l.devicesHandoffContinueHere,
      onPressed: () => playVideo(context, item),
    );
    final text = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          l.devicesHandoffPlayingOn(device),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: theme.textTheme.labelMedium?.copyWith(
            color: KColors.textSecondary,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          '$title · ${formatClock(entry.positionMs)}',
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: theme.textTheme.titleMedium,
        ),
      ],
    );
    return Padding(
      key: const ValueKey('handoff-banner'),
      padding: EdgeInsets.fromLTRB(
        d.pageInsets.left,
        12,
        d.pageInsets.right,
        4,
      ),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: KColors.bgElevated,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: KColors.stroke),
        ),
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: LayoutBuilder(
            builder: (context, c) {
              final icon = Icon(Icons.devices, color: KColors.textSecondary);
              if (c.maxWidth < 520) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        icon,
                        const SizedBox(width: 12),
                        Expanded(child: text),
                      ],
                    ),
                    const SizedBox(height: 12),
                    button,
                  ],
                );
              }
              return Row(
                children: [
                  icon,
                  const SizedBox(width: 12),
                  Expanded(child: text),
                  const SizedBox(width: 12),
                  button,
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}
