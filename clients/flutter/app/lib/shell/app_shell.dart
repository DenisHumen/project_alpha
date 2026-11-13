// SPDX-License-Identifier: MPL-2.0
//
// Adaptive navigation (01 §2, contract "Навигация"):
// * desktop / wide windows — sidebar;
// * phone — bottom tabs (Settings behind a gear on Home);
// * Android TV — top menu, D-pad focus; moving focus along the menu switches
//   the section (tvOS tab bar behaviour), Back goes up to the menu, then Home.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../integration/player.dart';
import '../screens/home_screen.dart';
import '../screens/library_grid_screen.dart';
import '../screens/music_screen.dart';
import '../screens/playback.dart';
import '../screens/search_screen.dart';
import '../screens/settings_screen.dart';
import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/logo.dart';

enum Section {
  home('Home', Icons.home_outlined, Icons.home_rounded),
  movies('Movies', Icons.movie_outlined, Icons.movie_rounded),
  shows('TV Shows', Icons.tv_outlined, Icons.tv_rounded),
  music('Music', Icons.library_music_outlined, Icons.library_music_rounded),
  search('Search', Icons.search, Icons.search),
  settings('Settings', Icons.settings_outlined, Icons.settings_rounded);

  const Section(this.label, this.icon, this.selectedIcon);
  final String label;
  final IconData icon;
  final IconData selectedIcon;
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

  Section get section => _section;

  void select(Section s) {
    if (s == _section) {
      // Re-selecting a section pops it to its root.
      _navKeys[s]!.currentState?.popUntil((r) => r.isFirst);
    }
    setState(() => _section = s);
  }

  @override
  void initState() {
    super.initState();
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
    final width = MediaQuery.sizeOf(context).width;
    final useSidebar = d.isDesktop || (d.isPhone && width >= 840);

    final content = IndexedStack(
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

    final canExit =
        _section == Section.home &&
        !_nestedCanPop &&
        !(d.isTv && !_menuFocus[Section.home]!.hasFocus);
    final Widget body;
    if (d.isTv) {
      body = _TvLayout(shell: this, content: content);
    } else if (useSidebar) {
      body = _SidebarLayout(shell: this, content: content);
    } else {
      body = _PhoneLayout(shell: this, content: content);
    }
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

// ---------------------------------------------------------------- desktop

class _SidebarLayout extends StatelessWidget {
  const _SidebarLayout({required this.shell, required this.content});

  final AppShellState shell;
  final Widget content;

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final theme = Theme.of(context);
    return Scaffold(
      body: Row(
        children: [
          Container(
            width: 232,
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
                    for (final s in Section.values) ...[
                      if (s == Section.search) const SizedBox(height: 18),
                      _SidebarItem(
                        section: s,
                        selected: shell.section == s,
                        onTap: () => shell.select(s),
                      ),
                    ],
                    const Spacer(),
                    if (services.scanning)
                      Padding(
                        padding: const EdgeInsets.all(20),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              'Scanning library…',
                              style: theme.textTheme.labelMedium,
                            ),
                            const SizedBox(height: 8),
                            LinearProgressIndicator(
                              value: (services.scanProgress?.total ?? 0) == 0
                                  ? null
                                  : services.scanProgress!.processed /
                                        services.scanProgress!.total!,
                            ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
          Expanded(
            child: Stack(
              children: [
                Positioned.fill(child: content),
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
        semanticLabel: section.label,
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
                section.label,
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

// ---------------------------------------------------------------- phone

class _PhoneLayout extends StatelessWidget {
  const _PhoneLayout({required this.shell, required this.content});

  final AppShellState shell;
  final Widget content;

  static const _tabs = [
    Section.home,
    Section.movies,
    Section.shows,
    Section.music,
    Section.search,
  ];

  @override
  Widget build(BuildContext context) {
    final idx = _tabs.indexOf(shell.section);
    return Scaffold(
      body: Stack(
        children: [
          Positioned.fill(child: content),
          if (shell.section == Section.home && !shell._nestedCanPop)
            Positioned(
              right: 8,
              top: MediaQuery.paddingOf(context).top + 4,
              child: IconButton(
                tooltip: 'Settings',
                style: IconButton.styleFrom(backgroundColor: Colors.black38),
                onPressed: () => shell.select(Section.settings),
                icon: const Icon(Icons.settings_outlined),
              ),
            ),
          const Positioned(left: 8, right: 8, bottom: 8, child: _MiniPlayer()),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: idx < 0 ? 0 : idx,
        labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
        onDestinationSelected: (i) => shell.select(_tabs[i]),
        destinations: [
          for (final s in _tabs)
            NavigationDestination(
              icon: Icon(s.icon),
              selectedIcon: Icon(s.selectedIcon),
              label: s == Section.shows ? 'Shows' : s.label,
            ),
        ],
      ),
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
                  for (final s in const [
                    Section.home,
                    Section.movies,
                    Section.shows,
                    Section.music,
                  ]) ...[
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
      semanticLabel: section.label,
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
                  section.label,
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
