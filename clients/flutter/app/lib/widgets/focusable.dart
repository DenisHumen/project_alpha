// SPDX-License-Identifier: MPL-2.0
//
// The focus engine visual (01 §4.2): a focused element scales up (posters
// ~1.10, cards ~1.08 on TV), gets a soft glow and drop shadow and a moving
// specular "light" (parallax-light). Desktop hover uses a gentler version,
// phones get a press effect (scale 0.97) instead.

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../theme/kinolune_theme.dart';

class KFocusable extends StatefulWidget {
  const KFocusable({
    super.key,
    required this.builder,
    this.onActivate,
    this.onFocusChange,
    this.focusScale,
    this.radius,
    this.autofocus = false,
    this.focusNode,
    this.semanticLabel,
    this.light = true,
  });

  final Widget Function(BuildContext context, bool focused) builder;
  final VoidCallback? onActivate;
  final ValueChanged<bool>? onFocusChange;
  final double? focusScale;
  final double? radius;
  final bool autofocus;
  final FocusNode? focusNode;
  final String? semanticLabel;

  /// Draw the specular light overlay (cards/posters; off for text buttons).
  final bool light;

  @override
  State<KFocusable> createState() => _KFocusableState();
}

class _KFocusableState extends State<KFocusable> with SingleTickerProviderStateMixin {
  bool _focused = false;
  bool _hovered = false;
  bool _pressed = false;
  Offset _pointer = const Offset(.3, .2);
  late final AnimationController _breath =
      AnimationController(vsync: this, duration: const Duration(milliseconds: 3200));

  @override
  void dispose() {
    _breath.dispose();
    super.dispose();
  }

  void _setFocus(bool f) {
    setState(() => _focused = f);
    widget.onFocusChange?.call(f);
    final d = KDensity.of(context);
    if (f && d.isTv && !_reduceMotion) {
      _breath.repeat(reverse: true);
    } else {
      _breath.stop();
    }
  }

  bool get _reduceMotion => MediaQuery.maybeDisableAnimationsOf(context) ?? false;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final active = _focused || _hovered;
    final scale = _pressed
        ? .97
        : _focused
            ? (widget.focusScale ?? d.focusScaleCard)
            : _hovered
                ? 1 + ((widget.focusScale ?? d.focusScaleCard) - 1) * .5
                : 1.0;
    final radius = BorderRadius.circular(widget.radius ?? d.cardRadius);
    final showGlow = _focused && (d.isTv || d.isDesktop);

    final content = AnimatedBuilder(
      animation: _breath,
      builder: (context, child) {
        // Light drifts slowly on TV (no touchpad tilt), follows the pointer on desktop.
        final t = Curves.easeInOut.transform(_breath.value);
        final light = d.isTv ? Offset(.15 + .5 * t, .1 + .15 * math.sin(t * math.pi)) : _pointer;
        return Stack(fit: StackFit.passthrough, children: [
          child!,
          if (widget.light)
            Positioned.fill(
              child: IgnorePointer(
                child: AnimatedOpacity(
                  opacity: active ? 1 : 0,
                  duration: KMotion.focus,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      borderRadius: radius,
                      gradient: RadialGradient(
                        center: Alignment(light.dx * 2 - 1, light.dy * 2 - 1),
                        radius: 1.1,
                        colors: [Colors.white.withValues(alpha: _focused ? .22 : .12), Colors.white.withValues(alpha: 0)],
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ]);
      },
      child: widget.builder(context, active),
    );

    return Semantics(
      button: widget.onActivate != null,
      label: widget.semanticLabel,
      child: FocusableActionDetector(
        focusNode: widget.focusNode,
        autofocus: widget.autofocus,
        onShowFocusHighlight: (_) {},
        onFocusChange: _setFocus,
        onShowHoverHighlight: (h) => setState(() => _hovered = h),
        mouseCursor: widget.onActivate != null ? SystemMouseCursors.click : MouseCursor.defer,
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.select): ActivateIntent(),
          SingleActivator(LogicalKeyboardKey.enter): ActivateIntent(),
          SingleActivator(LogicalKeyboardKey.numpadEnter): ActivateIntent(),
          SingleActivator(LogicalKeyboardKey.gameButtonA): ActivateIntent(),
        },
        actions: {
          ActivateIntent: CallbackAction<ActivateIntent>(onInvoke: (_) {
            widget.onActivate?.call();
            return null;
          }),
        },
        child: MouseRegion(
          onHover: (e) {
            final box = context.findRenderObject() as RenderBox?;
            if (box == null || !box.hasSize) return;
            setState(() => _pointer = Offset(e.localPosition.dx / box.size.width, e.localPosition.dy / box.size.height));
          },
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTapDown: d.isPhone ? (_) => setState(() => _pressed = true) : null,
            onTapUp: d.isPhone ? (_) => setState(() => _pressed = false) : null,
            onTapCancel: d.isPhone ? () => setState(() => _pressed = false) : null,
            onTap: widget.onActivate,
            child: AnimatedScale(
              scale: scale,
              duration: _focused ? KMotion.focus : KMotion.focusOut,
              curve: _focused ? KMotion.focusCurve : Curves.easeOut,
              child: AnimatedContainer(
                duration: KMotion.focus,
                decoration: BoxDecoration(
                  borderRadius: radius,
                  boxShadow: showGlow
                      ? [
                          const BoxShadow(color: Color(0x73000000), blurRadius: 24, offset: Offset(0, 10)),
                          BoxShadow(color: KColors.focusGlow, blurRadius: 22, spreadRadius: 1),
                        ]
                      : const [],
                ),
                foregroundDecoration: _focused && !d.isPhone
                    ? BoxDecoration(
                        borderRadius: radius,
                        border: Border.all(color: Colors.white.withValues(alpha: .85), width: d.isTv ? 2 : 1.5),
                      )
                    : null,
                child: ClipRRect(borderRadius: radius, child: content),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Capsule button: white fill + dark label when focused on TV (01 §4.5).
class KButton extends StatefulWidget {
  const KButton({
    super.key,
    required this.label,
    this.icon,
    this.onPressed,
    this.primary = false,
    this.autofocus = false,
    this.focusNode,
  });

  final String label;
  final IconData? icon;
  final VoidCallback? onPressed;
  final bool primary;
  final bool autofocus;
  final FocusNode? focusNode;

  @override
  State<KButton> createState() => _KButtonState();
}

class _KButtonState extends State<KButton> {
  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    return KFocusable(
      autofocus: widget.autofocus,
      focusNode: widget.focusNode,
      onActivate: widget.onPressed,
      focusScale: d.focusScaleButton,
      radius: 999,
      light: false,
      semanticLabel: widget.label,
      builder: (context, active) {
        final focused = active;
        final Color bg, fg;
        if (focused) {
          bg = KColors.focusFill;
          fg = KColors.focusLabel;
        } else if (widget.primary) {
          bg = KColors.accent;
          fg = Colors.white;
        } else {
          bg = Colors.white.withValues(alpha: .12);
          fg = Colors.white;
        }
        final vPad = d.isTv ? 10.0 : 11.0;
        return AnimatedContainer(
          duration: KMotion.focus,
          padding: EdgeInsets.symmetric(horizontal: d.isTv ? 22 : 20, vertical: vPad),
          decoration: BoxDecoration(color: bg, borderRadius: BorderRadius.circular(999)),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            if (widget.icon != null) ...[
              Icon(widget.icon, color: fg, size: d.type.callout + 6),
              const SizedBox(width: 8),
            ],
            Text(widget.label, style: theme.textTheme.labelLarge?.copyWith(color: fg)),
          ]),
        );
      },
    );
  }
}

/// Debounces focus-driven background changes (01 §4.7 crossfade-art).
class Debouncer {
  Debouncer(this.delay);
  final Duration delay;
  Timer? _t;
  void call(VoidCallback f) {
    _t?.cancel();
    _t = Timer(delay, f);
  }

  void dispose() => _t?.cancel();
}
