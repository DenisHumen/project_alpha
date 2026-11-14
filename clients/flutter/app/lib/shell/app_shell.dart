// SPDX-License-Identifier: MPL-2.0
//
// Adaptive navigation (UI-002; 01 §2.5–2.6, §4.3, §4.10 п.4). The chrome
// follows the window size class (breakpoints.dart), not the device:
// * compact (< 600) — bottom tabs «Home · Library · Twitch · Music ·
//   Search»; Library holds Movies and TV Shows, profile and settings sit
//   behind the avatar on Home;
// * medium (< 1024) — rail (narrow sidebar);
// * expanded and large — full sidebar;
// * Android TV — top menu, D-pad focus; moving focus along the menu switches
//   the section (tvOS tab bar behaviour), Back goes up to the menu, then Home.
// The Twitch section is hidden unless enabled in Settings.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../screens/home_screen.dart';
import '../screens/library_grid_screen.dart';
import '../screens/music_screen.dart';
import '../screens/playback.dart';
import '../screens/search_screen.dart';
import '../screens/settings_screen.dart';
import '../integration/library.dart';
import '../services/app_services.dart';
import '../screens/twitch_screen.dart';
import '../platform/form_factor.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/logo.dart';
import 'breakpoints.dart';

enum Section {
  home(Icons.home_outlined, Icons.home_rounded),
  movies(Icons.movie_outlined, Icons.movie_rounded),
  shows(Icons.tv_outlined, Icons.tv_rounded),
  twitch(Icons.live_tv_outlined, Icons.live_tv_rounded),
  music(Icons.library_music_outlined, Icons.library_music_rounded),
  search(Icons.search, Icons.search),
  settings(Icons.settings_outlined, Icons.settings_rounded);

  const Section(this.icon, this.selectedIcon);
  final IconData icon;
  final IconData selectedIcon;

  String label(AppLocalizations l) => switch (this) {
    home => l.navHome,
    movies => l.navMovies,
    shows => l.navShows,
    twitch => l.navTwitch,
    music => l.navMusic,
    search => l.navSearch,
    settings => l.navSettings,
  };
}

class AppShell extends StatefulWidget {
  const AppShell({super.key, this.initialSection = Section.home});

  final Section initialSection;

  @override
  State<AppShell> createState() => AppShellState();
}

class AppShellState extends State<AppShell> {
  late Section _section = widget.initialSection;
  final _navKeys = {
    for (final s in Section.values) s: GlobalKey<NavigatorState>(),
  };
  late final _observers = {
    for (final s in Section.values) s: _RebuildObserver(() => setState(() {})),
  };
  final _menuFocus = {
    for (final s in Section.values) s: FocusNode(debugLabel: 'menu-${s.name}'),
  };

  Section _lastLibrary = Section.movies;
  AppServices? _services;
  bool _showTwitch = false;

  Section get section => _section;

  /// Whether the Twitch section is in the navigation (Settings toggle).
  bool get showTwitch => _showTwitch;

  /// Sections of the sidebar, rail and TV menu, in order.
  List<Section> get menuSections => [
    Section.home,
    Section.movies,
    Section.shows,
    if (_showTwitch) Section.twitch,
    Section.music,
    Section.search,
    Section.settings,
  ];

  void select(Section s) {
    if (s == Section.twitch && !_showTwitch) s = Section.home;
    if (s == _section) {
      // Re-selecting a section pops it to its root.
      _navKeys[s]!.currentState?.popUntil((r) => r.isFirst);
    }
    if (s == Section.movies || s == Section.shows) _lastLibrary = s;
    setState(() => _section = s);
  }

  /// The phone Library tab: the last of Movies / TV Shows that was open.
  void selectLibrary() => select(
    _section == Section.movies || _section == Section.shows
        ? _section
        : _lastLibrary,
  );

  void _onShowTwitch() {
    final show = _services!.showTwitch.value;
    if (show == _showTwitch) return;
    setState(() {
      _showTwitch = show;
      if (!show && _section == Section.twitch) _section = Section.home;
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final services = Services.of(context);
    if (!identical(services, _services)) {
      _services?.showTwitch.removeListener(_onShowTwitch);
      _services = services..showTwitch.addListener(_onShowTwitch);
      _showTwitch = services.showTwitch.value;
      if (!_showTwitch && _section == Section.twitch) _section = Section.home;
    }
  }

  @override
  void initState() {
    super.initState();
    if (_section == Section.shows) _lastLibrary = Section.shows;
    // Nested navigators grab focus for their first route; on TV the menu tab
    // must own the initial focus instead.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && KDensity.of(context).isTv) {
        _menuFocus[_section]!.requestFocus();
      }
    });
  }

  @override
  void dispose() {
    _services?.showTwitch.removeListener(_onShowTwitch);
    for (final f in _menuFocus.values) {
      f.dispose();
    }
    super.dispose();
  }

  Widget _rootFor(Section s) => switch (s) {
    Section.home => HomeScreen(
      onOpenSection: (i) => select(
        const [Section.home, Section.movies, Section.shows, Section.music][i],
      ),
    ),
    Section.movies => const LibraryGridScreen(kind: ItemKind.movie),
    Section.shows => const LibraryGridScreen(kind: ItemKind.show),
    Section.twitch => const TwitchScreen(),
    Section.music => const MusicScreen(),
    Section.search => const SearchScreen(),
    Section.settings => const SettingsScreen(),
  };

  bool get _nestedCanPop => _navKeys[_section]!.currentState?.canPop() ?? false;

  void _handleBack() {
    final nav = _navKeys[_section]!.currentState;
    if (nav != null && nav.canPop()) {
      nav.pop();
      return;
    }
    final d = KDensity.of(context);
    if (d.isTv && !(_menuFocus[_section]!.hasFocus)) {
      _menuFocus[_section]!.requestFocus();
      return;
    }
    if (_section != Section.home) {
      setState(() => _section = Section.home);
      if (d.isTv) _menuFocus[Section.home]!.requestFocus();
    }
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final size = WindowSize.of(context);
    final layout = NavLayout.resolve(FormFactorScope.of(context), size);

    final stack = IndexedStack(
      index: _section.index,
      children: [
        for (final s in Section.values)
          HeroControllerScope.none(
            child: Navigator(
              key: _navKeys[s],
              observers: [_observers[s]!],
              routeDirectionalTraversalEdgeBehavior:
                  TraversalEdgeBehavior.parentScope,
              onGenerateRoute: (_) =>
                  MaterialPageRoute<void>(builder: (_) => _rootFor(s)),
            ),
          ),
      ],
    );
    final content = NavLayoutScope(layout: layout, child: stack);

    final canExit =
        _section == Section.home &&
        !_nestedCanPop &&
        !(d.isTv && !_menuFocus[Section.home]!.hasFocus);
    final Widget body = switch (layout) {
      NavLayout.topMenu => _TvLayout(shell: this, content: content),
      NavLayout.sidebar => _SidebarLayout(
        shell: this,
        content: content,
        width: size == WindowSize.large ? 264 : 232,
      ),
      NavLayout.rail => _RailLayout(shell: this, content: content),
      NavLayout.tabs => _PhoneLayout(shell: this, content: content),
    };
    return PopScope(
      canPop: canExit,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _handleBack();
      },
      child: Shortcuts(
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.escape): _BackIntent(),
        },
        child: Actions(
          actions: {
            _BackIntent: CallbackAction<_BackIntent>(
              onInvoke: (_) => _handleBack(),
            ),
          },
          child: body,
        ),
      ),
    );
  }
}

class _BackIntent extends Intent {
  const _BackIntent();
}

class _RebuildObserver extends NavigatorObserver {
  _RebuildObserver(this.onChange);
  final VoidCallback onChange;

  void _later() =>
      WidgetsBinding.instance.addPostFrameCallback((_) => onChange());

  @override
  void didPush(Route<dynamic> route, Route<dynamic>? previousRoute) => _later();
  @override
  void didPop(Route<dynamic> route, Route<dynamic>? previousRoute) => _later();
}

/// Mini player docked over the content when music is queued.
class _MiniPlayer extends StatelessWidget {
  const _MiniPlayer({this.maxWidth = 560});

  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    return ListenableBuilder(
      listenable: services.music,
      builder: (context, _) {
        if (services.music.current == null) return const SizedBox.shrink();
        return ConstrainedBox(
          constraints: BoxConstraints(maxWidth: maxWidth),
          child: DecoratedBox(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(14),
              boxShadow: const [
                BoxShadow(
                  color: Color(0x80000000),
                  blurRadius: 24,
                  offset: Offset(0, 8),
                ),
              ],
            ),
            child: NowPlayingBar(
              controller: services.music,
              onOpen: () => openNowPlaying(context),
            ),
          ),
        );
      },
    );
  }
}

/// System insets (edge to edge on Android, the notch on iOS). The shell
/// does not pad its pages: their headers start below the status bar
/// ([KDensity.safeTop], used by `PageTitle` and the Home hero) while
/// backdrops run under it. This scrim keeps the clock and the status icons
/// legible over content scrolled beneath them. Without a top inset (desktop,
/// TV) it draws nothing.
class _StatusBarScrim extends StatelessWidget {
  const _StatusBarScrim();

  @override
  Widget build(BuildContext context) {
    final top = MediaQuery.paddingOf(context).top;
    // Always positioned: a non-positioned child would size the Stack.
    return Positioned(
      left: 0,
      top: 0,
      right: 0,
      height: top,
      child: top <= 0
          ? const SizedBox.shrink()
          : IgnorePointer(
              child: ColoredBox(
                key: const ValueKey('status-bar-scrim'),
                color: KColors.bgBase.withValues(alpha: .72),
              ),
            ),
    );
  }
}

// ---------------------------------------------------------------- desktop

/// Library scan progress at the bottom of the sidebar and the rail.
class _ScanProgress extends StatelessWidget {
  const _ScanProgress({this.compact = false});

  final bool compact;

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    if (!services.scanning) return const SizedBox.shrink();
    final total = services.scanProgress?.total ?? 0;
    final bar = LinearProgressIndicator(
      value: total == 0 ? null : services.scanProgress!.processed / total,
    );
    if (compact) {
      return Tooltip(
        message: context.l10n.shellScanningLibrary,
        child: Padding(padding: const EdgeInsets.all(16), child: bar),
      );
    }
    return Padding(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            context.l10n.shellScanningLibrary,
            style: Theme.of(context).textTheme.labelMedium,
          ),
          const SizedBox(height: 8),
          bar,
        ],
      ),
    );
  }
}

class _SidebarLayout extends StatelessWidget {
  const _SidebarLayout({
    required this.shell,
    required this.content,
    this.width = 232,
  });

  final AppShellState shell;
  final Widget content;
  final double width;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      key: const ValueKey('nav-sidebar'),
      body: Row(
        children: [
          Container(
            width: width,
            decoration: BoxDecoration(
              color: KColors.bgElevated,
              border: Border(right: BorderSide(color: KColors.stroke)),
            ),
            child: SafeArea(
              right: false,
              child: FocusTraversalGroup(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Padding(
                      padding: EdgeInsets.fromLTRB(20, 22, 20, 26),
                      child: KinoluneLogo(size: 30),
                    ),
                    for (final s in shell.menuSections) ...[
                      if (s == Section.search) const SizedBox(height: 18),
                      _SidebarItem(
                        section: s,
                        selected: shell.section == s,
                        onTap: () => shell.select(s),
                      ),
                    ],
                    const Spacer(),
                    const _ScanProgress(),
                  ],
                ),
              ),
            ),
          ),
          Expanded(
            child: Stack(
              children: [
                Positioned.fill(child: content),
                const _StatusBarScrim(),
                const Positioned(
                  left: 24,
                  right: 24,
                  bottom: 20,
                  child: Center(child: _MiniPlayer()),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _SidebarItem extends StatelessWidget {
  const _SidebarItem({
    required this.section,
    required this.selected,
    required this.onTap,
  });

  final Section section;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
      child: KFocusable(
        onActivate: onTap,
        focusScale: 1.0,
        light: false,
        radius: 10,
        semanticLabel: section.label(context.l10n),
        builder: (context, hovered) => AnimatedContainer(
          duration: KMotion.focus,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          color: selected
              ? Colors.white.withValues(alpha: .1)
              : hovered
              ? Colors.white.withValues(alpha: .05)
              : Colors.transparent,
          child: Row(
            children: [
              Icon(
                selected ? section.selectedIcon : section.icon,
                size: 20,
                color: selected ? KColors.accentSoft : KColors.textSecondary,
              ),
              const SizedBox(width: 14),
              Text(
                section.label(context.l10n),
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: selected ? FontWeight.w700 : FontWeight.w500,
                  color: selected ? Colors.white : KColors.textSecondary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------- rail

/// Medium windows (tablet in portrait, narrow desktop window): icons with
/// short labels in a narrow column.
class _RailLayout extends StatelessWidget {
  const _RailLayout({required this.shell, required this.content});

  final AppShellState shell;
  final Widget content;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      key: const ValueKey('nav-rail'),
      body: Row(
        children: [
          Container(
            width: 88,
            decoration: BoxDecoration(
              color: KColors.bgElevated,
              border: Border(right: BorderSide(color: KColors.stroke)),
            ),
            child: SafeArea(
              right: false,
              child: FocusTraversalGroup(
                child: Column(
                  children: [
                    const Padding(
                      padding: EdgeInsets.fromLTRB(0, 20, 0, 18),
                      child: KinoluneLogo(size: 32, showName: false),
                    ),
                    Expanded(
                      child: SingleChildScrollView(
                        child: Column(
                          children: [
                            for (final s in shell.menuSections) ...[
                              if (s == Section.search)
                                const SizedBox(height: 14),
                              _RailItem(
                                section: s,
                                selected: shell.section == s,
                                onTap: () => shell.select(s),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                    const _ScanProgress(compact: true),
                  ],
                ),
              ),
            ),
          ),
          Expanded(
            child: Stack(
              children: [
                Positioned.fill(child: content),
                const _StatusBarScrim(),
                const Positioned(
                  left: 16,
                  right: 16,
                  bottom: 16,
                  child: Center(child: _MiniPlayer()),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _RailItem extends StatelessWidget {
  const _RailItem({
    required this.section,
    required this.selected,
    required this.onTap,
  });

  final Section section;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final l = context.l10n;
    final label = section == Section.shows ? l.navShowsShort : section.label(l);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      child: KFocusable(
        onActivate: onTap,
        focusScale: 1.0,
        light: false,
        radius: 12,
        semanticLabel: section.label(l),
        builder: (context, hovered) => Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Column(
            children: [
              AnimatedContainer(
                duration: KMotion.focus,
                width: 56,
                height: 32,
                decoration: BoxDecoration(
                  color: selected
                      ? Colors.white.withValues(alpha: .14)
                      : hovered
                      ? Colors.white.withValues(alpha: .06)
                      : Colors.transparent,
                  borderRadius: BorderRadius.circular(16),
                ),
                child: Icon(
                  selected ? section.selectedIcon : section.icon,
                  size: 22,
                  color: selected ? KColors.accentSoft : KColors.textSecondary,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: theme.textTheme.labelSmall?.copyWith(
                  fontWeight: selected ? FontWeight.w700 : FontWeight.w500,
                  color: selected ? Colors.white : KColors.textSecondary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------- phone

class _PhoneLayout extends StatelessWidget {
  const _PhoneLayout({required this.shell, required this.content});

  final AppShellState shell;
  final Widget content;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    // Section.movies stands for the Library tab (Movies and TV Shows).
    final tabs = [
      Section.home,
      Section.movies,
      if (shell.showTwitch) Section.twitch,
      Section.music,
      Section.search,
    ];
    final current = shell.section == Section.shows
        ? Section.movies
        : shell.section;
    final idx = tabs.indexOf(current);
    // Settings has no tab (it sits behind the avatar on Home). NavigationBar
    // always marks some index as selected, so with no matching tab every
    // destination is drawn in the unselected style (UI-023).
    final none = idx < 0;
    final scheme = Theme.of(context).colorScheme;
    final navTheme = NavigationBarTheme.of(context);
    Widget bar = NavigationBar(
      selectedIndex: none ? 0 : idx,
      labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
      onDestinationSelected: (i) => tabs[i] == Section.movies
          ? shell.selectLibrary()
          : shell.select(tabs[i]),
      destinations: [
        for (final s in tabs)
          if (s == Section.movies)
            NavigationDestination(
              icon: const Icon(Icons.video_library_outlined),
              selectedIcon: Icon(
                none
                    ? Icons.video_library_outlined
                    : Icons.video_library_rounded,
              ),
              label: l.navLibrary,
            )
          else
            NavigationDestination(
              icon: Icon(s.icon),
              selectedIcon: Icon(none ? s.icon : s.selectedIcon),
              label: s.label(l),
            ),
      ],
    );
    if (none) {
      bar = NavigationBarTheme(
        data: navTheme.copyWith(
          indicatorColor: Colors.transparent,
          iconTheme: WidgetStatePropertyAll(
            IconThemeData(color: scheme.onSurfaceVariant, size: 24),
          ),
          labelTextStyle: WidgetStatePropertyAll(
            (navTheme.labelTextStyle?.resolve(const {}) ??
                    Theme.of(context).textTheme.labelMedium)
                ?.copyWith(color: scheme.onSurfaceVariant),
          ),
        ),
        child: bar,
      );
    }
    return Scaffold(
      key: const ValueKey('nav-tabs'),
      body: Stack(
        children: [
          Positioned.fill(child: content),
          const _StatusBarScrim(),
          if (shell.section == Section.home && !shell._nestedCanPop)
            Positioned(
              right: 8,
              top: MediaQuery.paddingOf(context).top + 4,
              child: IconButton(
                tooltip: l.navProfile,
                onPressed: () => shell.select(Section.settings),
                icon: CircleAvatar(
                  radius: 16,
                  backgroundColor: KColors.bgElevated2,
                  foregroundColor: KColors.textPrimary,
                  child: const Icon(Icons.person_rounded, size: 20),
                ),
              ),
            ),
          const Positioned(left: 8, right: 8, bottom: 8, child: _MiniPlayer()),
        ],
      ),
      bottomNavigationBar: bar,
    );
  }
}

// ---------------------------------------------------------------- TV

class _TvLayout extends StatelessWidget {
  const _TvLayout({required this.shell, required this.content});

  final AppShellState shell;
  final Widget content;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Scaffold(
      key: const ValueKey('nav-top-menu'),
      body: Column(
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              18,
              d.pageInsets.right,
              6,
            ),
            child: FocusTraversalGroup(
              child: Row(
                children: [
                  const KinoluneLogo(size: 26),
                  const SizedBox(width: 36),
                  for (final s in shell.menuSections)
                    if (s != Section.search && s != Section.settings) ...[
                      _TvTab(shell: shell, section: s),
                      const SizedBox(width: 6),
                    ],
                  const Spacer(),
                  _TvTab(shell: shell, section: Section.search, iconOnly: true),
                  const SizedBox(width: 6),
                  _TvTab(
                    shell: shell,
                    section: Section.settings,
                    iconOnly: true,
                  ),
                ],
              ),
            ),
          ),
          Expanded(
            child: Stack(
              children: [
                Positioned.fill(child: content),
                const Positioned(
                  right: 40,
                  bottom: 24,
                  child: _MiniPlayer(maxWidth: 380),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _TvTab extends StatelessWidget {
  const _TvTab({
    required this.shell,
    required this.section,
    this.iconOnly = false,
  });

  final AppShellState shell;
  final Section section;
  final bool iconOnly;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final selected = shell.section == section;
    return KFocusable(
      focusNode: shell._menuFocus[section],
      autofocus: section == shell.widget.initialSection,
      onActivate: () => shell.select(section),
      onFocusChange: (f) {
        if (f && shell.section != section) shell.select(section);
      },
      radius: 999,
      light: false,
      focusScale: 1.06,
      semanticLabel: section.label(context.l10n),
      builder: (context, focused) {
        final fg = focused
            ? KColors.focusLabel
            : (selected ? Colors.white : KColors.textSecondary);
        return AnimatedContainer(
          duration: KMotion.focus,
          padding: EdgeInsets.symmetric(
            horizontal: iconOnly ? 10 : 16,
            vertical: 7,
          ),
          decoration: BoxDecoration(
            color: focused
                ? KColors.focusFill
                : (selected
                      ? Colors.white.withValues(alpha: .14)
                      : Colors.transparent),
            borderRadius: BorderRadius.circular(999),
          ),
          child: iconOnly
              ? Icon(
                  selected ? section.selectedIcon : section.icon,
                  size: 20,
                  color: fg,
                )
              : Text(
                  section.label(context.l10n),
                  style: theme.textTheme.titleMedium?.copyWith(
                    color: fg,
                    fontWeight: selected || focused
                        ? FontWeight.w700
                        : FontWeight.w500,
                  ),
                ),
        );
      },
    );
  }
}
