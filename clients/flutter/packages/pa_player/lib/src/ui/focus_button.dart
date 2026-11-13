// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/material.dart';

import 'tokens.dart';

/// Round icon button with a tvOS-style focus state (white fill, dark icon,
/// slight zoom). Works with mouse, touch, keyboard and D-pad.
class FocusIconButton extends StatefulWidget {
  const FocusIconButton({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.size = 48,
    this.iconSize,
    this.autofocus = false,
    this.focusNode,
    this.active = false,
    this.filled = false,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final double size;
  final double? iconSize;
  final bool autofocus;
  final FocusNode? focusNode;

  /// Highlights the icon (e.g. shuffle on).
  final bool active;

  /// Solid background even without focus (primary action).
  final bool filled;

  @override
  State<FocusIconButton> createState() => _FocusIconButtonState();
}

class _FocusIconButtonState extends State<FocusIconButton> {
  bool _focused = false;
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final hi = _focused || _hovered;
    final bg = _focused
        ? PaTokens.focusRing
        : widget.filled
        ? PaTokens.text
        : hi
        ? const Color(0x33FFFFFF)
        : const Color(0x00000000);
    final fg = (_focused || widget.filled)
        ? PaTokens.bg
        : widget.active
        ? PaTokens.accent
        : PaTokens.text;
    return Tooltip(
      message: widget.tooltip,
      waitDuration: const Duration(milliseconds: 600),
      child: Semantics(
        button: true,
        label: widget.tooltip,
        child: FocusableActionDetector(
          focusNode: widget.focusNode,
          autofocus: widget.autofocus,
          enabled: widget.onPressed != null,
          mouseCursor: SystemMouseCursors.click,
          onShowHoverHighlight: (v) => setState(() => _hovered = v),
          onFocusChange: (v) => setState(() => _focused = v),
          actions: {
            ActivateIntent: CallbackAction<ActivateIntent>(
              onInvoke: (_) {
                widget.onPressed?.call();
                return null;
              },
            ),
          },
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: widget.onPressed,
            child: AnimatedScale(
              scale: _focused ? 1.12 : 1.0,
              duration: const Duration(milliseconds: 150),
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                width: widget.size,
                height: widget.size,
                decoration: BoxDecoration(color: bg, shape: BoxShape.circle),
                child: Icon(
                  widget.icon,
                  color: widget.onPressed == null ? PaTokens.textSecondary : fg,
                  size: widget.iconSize ?? widget.size * 0.55,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// A focusable row used in side panels and the queue.
class FocusListRow extends StatefulWidget {
  const FocusListRow({
    super.key,
    required this.title,
    this.subtitle,
    this.leading,
    this.trailing,
    this.selected = false,
    this.autofocus = false,
    required this.onPressed,
  });

  final String title;
  final String? subtitle;
  final Widget? leading;
  final Widget? trailing;
  final bool selected;
  final bool autofocus;
  final VoidCallback? onPressed;

  @override
  State<FocusListRow> createState() => _FocusListRowState();
}

class _FocusListRowState extends State<FocusListRow> {
  bool _focused = false;
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final fg = _focused ? PaTokens.bg : PaTokens.text;
    return FocusableActionDetector(
      autofocus: widget.autofocus,
      mouseCursor: SystemMouseCursors.click,
      onShowHoverHighlight: (v) => setState(() => _hovered = v),
      onFocusChange: (v) {
        setState(() => _focused = v);
        if (v) {
          Scrollable.ensureVisible(
            context,
            alignment: 0.5,
            duration: const Duration(milliseconds: 150),
          );
        }
      },
      actions: {
        ActivateIntent: CallbackAction<ActivateIntent>(
          onInvoke: (_) {
            widget.onPressed?.call();
            return null;
          },
        ),
      },
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onPressed,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 120),
          margin: const EdgeInsets.symmetric(vertical: 2),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
          decoration: BoxDecoration(
            color: _focused
                ? PaTokens.focusRing
                : _hovered
                ? const Color(0x1FFFFFFF)
                : const Color(0x00000000),
            borderRadius: BorderRadius.circular(10),
          ),
          child: Row(
            children: [
              if (widget.leading != null) ...[
                IconTheme(
                  data: IconThemeData(color: fg, size: 22),
                  child: widget.leading!,
                ),
                const SizedBox(width: 12),
              ],
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      widget.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: widget.selected && !_focused
                            ? PaTokens.accent
                            : fg,
                        fontSize: 16,
                        fontWeight: widget.selected
                            ? FontWeight.w600
                            : FontWeight.w400,
                      ),
                    ),
                    if (widget.subtitle != null)
                      Text(
                        widget.subtitle!,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: _focused
                              ? const Color(0xFF3F3F46)
                              : PaTokens.textSecondary,
                          fontSize: 13,
                        ),
                      ),
                  ],
                ),
              ),
              if (widget.trailing != null)
                IconTheme(
                  data: IconThemeData(color: fg, size: 20),
                  child: widget.trailing!,
                )
              else if (widget.selected)
                Icon(
                  Icons.check_rounded,
                  color: _focused ? PaTokens.bg : PaTokens.accent,
                  size: 20,
                ),
            ],
          ),
        ),
      ),
    );
  }
}
