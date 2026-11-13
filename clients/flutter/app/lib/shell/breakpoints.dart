// SPDX-License-Identifier: MPL-2.0
//
// Window size classes and the navigation chrome they get (UI-002,
// 01 §2.5–2.6, §4.3): compact < 600, medium < 1024, expanded < 1440, large.

import 'package:flutter/widgets.dart';

import '../platform/form_factor.dart';

/// Width class of the app window in logical pixels.
enum WindowSize {
  compact,
  medium,
  expanded,
  large;

  static const compactMax = 600.0;
  static const mediumMax = 1024.0;
  static const expandedMax = 1440.0;

  static WindowSize fromWidth(double width) {
    if (width < compactMax) return compact;
    if (width < mediumMax) return medium;
    if (width < expandedMax) return expanded;
    return large;
  }

  static WindowSize of(BuildContext context) =>
      fromWidth(MediaQuery.sizeOf(context).width);
}

/// Which navigation chrome the shell draws.
///
/// * [tabs] — bottom tab bar (phone, compact windows);
/// * [rail] — narrow sidebar with icons and short labels (medium: tablets
///   in portrait, small desktop windows);
/// * [sidebar] — full sidebar (expanded and large: tablets in landscape,
///   desktop);
/// * [topMenu] — Android TV top menu, D-pad focus (TV-002 takes it over).
enum NavLayout {
  tabs,
  rail,
  sidebar,
  topMenu;

  static NavLayout resolve(FormFactor formFactor, WindowSize size) {
    if (formFactor == FormFactor.tv) return topMenu;
    return switch (size) {
      WindowSize.compact => tabs,
      WindowSize.medium => rail,
      WindowSize.expanded || WindowSize.large => sidebar,
    };
  }

  static NavLayout of(BuildContext context) =>
      NavLayoutScope.maybeOf(context) ??
      resolve(FormFactorScope.of(context), WindowSize.of(context));
}

/// Publishes the shell's current [NavLayout] to the screens (e.g. the
/// library grid shows a Movies/Shows switch under the single Library tab).
class NavLayoutScope extends InheritedWidget {
  const NavLayoutScope({super.key, required this.layout, required super.child});

  final NavLayout layout;

  static NavLayout? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<NavLayoutScope>()?.layout;

  @override
  bool updateShouldNotify(NavLayoutScope oldWidget) =>
      oldWidget.layout != layout;
}
