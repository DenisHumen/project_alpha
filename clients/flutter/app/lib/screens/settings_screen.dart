// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import 'about_screen.dart';
import 'add_source_dialog.dart';
import 'devices_screen.dart';
import 'sources_screen.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final services = Services.of(context);
    final l = context.l10n;
    return ListView(
      padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 90),
      children: [
        PageTitle(l.navSettings),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
          child: Align(
            alignment: Alignment.topLeft,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 760),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _Section(
                    title: l.settingsSourcesTitle,
                    subtitle: l.settingsSourcesSubtitle(kAppName),
                    children: [
                      if (services.sources.isEmpty)
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 8),
                          child: Text(
                            l.settingsNoSources,
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ),
                      for (final s in services.sources) _SourceTile(source: s),
                      _NavTile(
                        icon: Icons.tune,
                        title: l.sourcesManage,
                        subtitle: l.sourcesManageSubtitle,
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => const SourcesScreen(),
                          ),
                        ),
                      ),
                      const SizedBox(height: 8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: KButton(
                          label: l.addSourceTitle,
                          icon: Icons.add,
                          primary: services.sources.isEmpty,
                          onPressed: () => showAddSourceDialog(context),
                        ),
                      ),
                    ],
                  ),
                  const _ScanSection(),
                  _Section(
                    title: l.settingsDevicesTitle,
                    subtitle: l.settingsDevicesSubtitle,
                    children: [
                      _NavTile(
                        icon: Icons.devices,
                        title: l.devicesTitle,
                        subtitle: l.settingsDevicesCount(
                          services.sync
                              .devices()
                              .where((d) => !d.thisDevice)
                              .length,
                        ),
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => const DevicesScreen(),
                          ),
                        ),
                      ),
                    ],
                  ),
                  const _TmdbSection(),
                  const _NavigationSection(),
                  const _LanguageSection(),
                  _Section(
                    title: l.settingsAboutTitle,
                    children: [
                      _NavTile(
                        icon: Icons.info_outline,
                        title: l.settingsAboutApp(kAppName),
                        subtitle: l.settingsAboutSubtitle(kAppVersion),
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => const AboutScreen(),
                          ),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.children, this.subtitle});

  final String title;
  final String? subtitle;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final d = KDensity.of(context);
    return Padding(
      padding: EdgeInsets.only(bottom: d.isTv ? 18 : 28),
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

class _NavTile extends StatelessWidget {
  const _NavTile({
    required this.icon,
    required this.title,
    this.subtitle,
    this.onTap,
    this.trailing,
    this.selected,
    this.toggled,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final VoidCallback? onTap;
  final Widget? trailing;

  /// Radio-style tiles report their state to screen readers.
  final bool? selected;

  /// Switch-style tiles report on/off to screen readers.
  final bool? toggled;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Semantics(
      selected: selected,
      toggled: toggled,
      inMutuallyExclusiveGroup: selected != null,
      child: KFocusable(
        onActivate: onTap,
        focusScale: 1.0,
        light: false,
        radius: 12,
        semanticLabel: title,
        builder: (context, focused) => AnimatedContainer(
          duration: KMotion.focus,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          color: focused
              ? Colors.white.withValues(alpha: .09)
              : Colors.transparent,
          child: Row(
            children: [
              Icon(icon, color: KColors.textSecondary),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: theme.textTheme.titleMedium),
                    if (subtitle != null)
                      Text(
                        subtitle!,
                        style: theme.textTheme.labelSmall,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                  ],
                ),
              ),
              trailing ??
                  (onTap != null
                      ? Icon(Icons.chevron_right, color: KColors.textTertiary)
                      : const SizedBox.shrink()),
            ],
          ),
        ),
      ),
    );
  }
}

class _SourceTile extends StatelessWidget {
  const _SourceTile({required this.source});

  final MediaSource source;

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final smb = source.kind == SourceKind.smb;
    final l = context.l10n;
    return _NavTile(
      icon: smb ? Icons.lan_outlined : Icons.folder_outlined,
      title: smb ? '${source.name}  ${l.settingsSourceSmbSuffix}' : source.name,
      subtitle: [
        source.path,
        if (source.username != null) l.settingsSourceUser(source.username!),
      ].join('  ·  '),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: l.settingsScanSource,
            onPressed: services.scanning
                ? null
                : () => services.scan(sourceId: source.id),
            icon: const Icon(Icons.refresh),
          ),
          IconButton(
            tooltip: l.settingsRemoveSource,
            onPressed: () async {
              final ok = await showDialog<bool>(
                context: context,
                builder: (c) => AlertDialog(
                  title: Text(l.settingsRemoveSourceTitle),
                  content: Text(l.settingsRemoveSourceBody(source.name)),
                  actions: [
                    TextButton(
                      onPressed: () => Navigator.pop(c, false),
                      child: Text(l.commonCancel),
                    ),
                    FilledButton(
                      onPressed: () => Navigator.pop(c, true),
                      child: Text(l.settingsRemove),
                    ),
                  ],
                ),
              );
              if (ok == true) await services.deleteSource(source.id);
            },
            icon: const Icon(Icons.delete_outline),
          ),
        ],
      ),
    );
  }
}

class _ScanSection extends StatelessWidget {
  const _ScanSection();

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final p = services.scanProgress;
    final theme = Theme.of(context);
    final l = context.l10n;
    final String status;
    if (p == null) {
      status = l.settingsScanNever;
    } else if (services.scanning) {
      status = switch (p.phase) {
        ScanPhase.connecting => l.settingsScanConnecting,
        ScanPhase.metadata => l.settingsScanMetadata(
          p.processed,
          p.total ?? p.filesFound,
        ),
        _ => l.settingsScanFiles(p.processed, p.total ?? p.filesFound),
      };
    } else if (p.error != null) {
      status = l.settingsScanFailed(p.error!);
    } else {
      status = l.settingsScanUpToDate(p.filesFound);
    }
    return _Section(
      title: l.settingsLibraryTitle,
      subtitle: l.settingsLibrarySubtitle,
      children: [
        Row(
          children: [
            KButton(
              label: services.scanning ? l.settingsScanning : l.settingsScanNow,
              icon: Icons.sync,
              onPressed: services.scanning || services.sources.isEmpty
                  ? null
                  : () => services.scan(),
            ),
            const SizedBox(width: 16),
            Expanded(child: Text(status, style: theme.textTheme.bodySmall)),
          ],
        ),
        if (p != null && services.scanning) ...[
          const SizedBox(height: 12),
          ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: LinearProgressIndicator(
              value: (p.total ?? 0) == 0 ? null : p.processed / p.total!,
              minHeight: 6,
            ),
          ),
          if (p.currentPath != null) ...[
            const SizedBox(height: 6),
            Text(
              p.currentPath!,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.labelSmall,
            ),
          ],
        ],
      ],
    );
  }
}

class _TmdbSection extends StatefulWidget {
  const _TmdbSection();

  @override
  State<_TmdbSection> createState() => _TmdbSectionState();
}

class _TmdbSectionState extends State<_TmdbSection> {
  final _key = TextEditingController();
  bool _loaded = false;
  bool _obscure = true;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_loaded) return;
    _loaded = true;
    Services.read(context).tmdbKey().then((k) {
      if (mounted && k != null) setState(() => _key.text = k);
    });
  }

  @override
  void dispose() {
    _key.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final l = context.l10n;
    return _Section(
      title: l.settingsTmdbTitle,
      subtitle: l.settingsTmdbSubtitle(kAppName),
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _key,
                obscureText: _obscure,
                decoration: InputDecoration(
                  labelText: l.settingsTmdbKeyLabel,
                  suffixIcon: IconButton(
                    tooltip: _obscure ? l.commonShow : l.commonHide,
                    onPressed: () => setState(() => _obscure = !_obscure),
                    icon: Icon(
                      _obscure
                          ? Icons.visibility_outlined
                          : Icons.visibility_off_outlined,
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 12),
            KButton(
              label: l.commonSave,
              onPressed: () async {
                final messenger = ScaffoldMessenger.maybeOf(context);
                await Services.read(context).setTmdbKey(_key.text);
                messenger?.showSnackBar(
                  SnackBar(
                    content: Text(
                      _key.text.trim().isEmpty
                          ? l.settingsTmdbKeyRemoved
                          : l.settingsTmdbKeySaved,
                    ),
                  ),
                );
              },
            ),
          ],
        ),
        const SizedBox(height: 10),
        Text(
          '${l.settingsTmdbHint} ${l.commonTmdbNotice}',
          style: theme.textTheme.labelSmall,
        ),
      ],
    );
  }
}

/// Navigation options (UI-002): the Twitch section can be hidden.
class _NavigationSection extends StatelessWidget {
  const _NavigationSection();

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final l = context.l10n;
    return ValueListenableBuilder<bool>(
      valueListenable: services.showTwitch,
      builder: (context, show, _) => _Section(
        title: l.settingsNavigationTitle,
        children: [
          _NavTile(
            icon: Icons.live_tv_outlined,
            title: l.settingsShowTwitch,
            subtitle: l.settingsShowTwitchSubtitle,
            onTap: () => services.setShowTwitch(!show),
            trailing: ExcludeFocus(
              child: ExcludeSemantics(
                child: Switch(
                  value: show,
                  onChanged: (v) => services.setShowTwitch(v),
                ),
              ),
            ),
            toggled: show,
          ),
        ],
      ),
    );
  }
}

/// Interface language: the system locale by default, or a manual choice that
/// is kept in Preferences (UI-020).
class _LanguageSection extends StatelessWidget {
  const _LanguageSection();

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final l = context.l10n;
    return ValueListenableBuilder<Locale?>(
      valueListenable: services.locale,
      builder: (context, current, _) {
        final options = <(Locale?, String)>[
          (null, l.settingsLanguageSystem),
          (const Locale('en'), l.settingsLanguageEnglish),
          (const Locale('ru'), l.settingsLanguageRussian),
          if (kDebugMode || current == kPseudoLocale)
            (kPseudoLocale, l.settingsLanguagePseudo),
        ];
        return _Section(
          title: l.settingsLanguageTitle,
          subtitle: l.settingsLanguageSubtitle,
          children: [
            for (final (locale, name) in options)
              _NavTile(
                icon: locale == current
                    ? Icons.radio_button_checked
                    : Icons.radio_button_unchecked,
                title: name,
                selected: locale == current,
                onTap: () => services.setLocale(locale),
                trailing: const SizedBox.shrink(),
              ),
          ],
        );
      },
    );
  }
}
