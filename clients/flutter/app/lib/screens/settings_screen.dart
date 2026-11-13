// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import 'about_screen.dart';
import 'add_source_dialog.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final services = Services.of(context);
    return ListView(
      padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 90),
      children: [
        const PageTitle('Settings'),
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
                    title: 'Sources',
                    subtitle:
                        'Folders and network shares $kAppName scans for movies, shows and music.',
                    children: [
                      if (services.sources.isEmpty)
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 8),
                          child: Text(
                            'No sources yet.',
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ),
                      for (final s in services.sources) _SourceTile(source: s),
                      const SizedBox(height: 8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: KButton(
                          label: 'Add source',
                          icon: Icons.add,
                          primary: services.sources.isEmpty,
                          onPressed: () => showAddSourceDialog(context),
                        ),
                      ),
                    ],
                  ),
                  const _ScanSection(),
                  const _TmdbSection(),
                  _Section(
                    title: 'About',
                    children: [
                      _NavTile(
                        icon: Icons.info_outline,
                        title: 'About $kAppName',
                        subtitle:
                            'Version $kAppVersion · licences · TMDB attribution',
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
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final VoidCallback? onTap;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return KFocusable(
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
    return _NavTile(
      icon: smb ? Icons.lan_outlined : Icons.folder_outlined,
      title: source.name + (smb ? '  (SMB beta)' : ''),
      subtitle: [
        source.path,
        if (source.username != null) 'as ${source.username}',
      ].join('  ·  '),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: 'Scan this source',
            onPressed: services.scanning
                ? null
                : () => services.scan(sourceId: source.id),
            icon: const Icon(Icons.refresh),
          ),
          IconButton(
            tooltip: 'Remove source',
            onPressed: () async {
              final ok = await showDialog<bool>(
                context: context,
                builder: (c) => AlertDialog(
                  title: const Text('Remove source?'),
                  content: Text(
                    '“${source.name}” and its titles will be removed from the library. Files are not touched.',
                  ),
                  actions: [
                    TextButton(
                      onPressed: () => Navigator.pop(c, false),
                      child: const Text('Cancel'),
                    ),
                    FilledButton(
                      onPressed: () => Navigator.pop(c, true),
                      child: const Text('Remove'),
                    ),
                  ],
                ),
              );
              if (ok == true) await services.removeSource(source.id);
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
    final String status;
    if (p == null) {
      status = 'Not scanned in this session.';
    } else if (services.scanning) {
      status = switch (p.phase) {
        ScanPhase.connecting => 'Connecting…',
        ScanPhase.metadata =>
          'Fetching posters and descriptions… ${p.processed} of ${p.total ?? p.filesFound}',
        _ => 'Scanning… ${p.processed} of ${p.total ?? p.filesFound} files',
      };
    } else if (p.error != null) {
      status = 'Scan finished with an error: ${p.error}';
    } else {
      status = 'Up to date · ${p.filesFound} files';
    }
    return _Section(
      title: 'Library',
      subtitle: 'Scanning finds new files, recognises titles from file names and fetches posters.',
      children: [
        Row(
          children: [
            KButton(
              label: services.scanning ? 'Scanning…' : 'Scan now',
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
    return _Section(
      title: 'Metadata (TMDB)',
      subtitle:
          'Posters, backdrops and descriptions come from The Movie Database. '
          'Without a key $kAppName still works with titles recognised from file names.',
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _key,
                obscureText: _obscure,
                decoration: InputDecoration(
                  labelText: 'TMDB API key (v3) or read access token',
                  suffixIcon: IconButton(
                    tooltip: _obscure ? 'Show' : 'Hide',
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
              label: 'Save',
              onPressed: () async {
                final messenger = ScaffoldMessenger.maybeOf(context);
                await Services.read(context).setTmdbKey(_key.text);
                messenger?.showSnackBar(
                  SnackBar(
                    content: Text(
                      _key.text.trim().isEmpty
                          ? 'TMDB key removed'
                          : 'TMDB key saved',
                    ),
                  ),
                );
              },
            ),
          ],
        ),
        const SizedBox(height: 10),
        Text(
          'Get a free key at themoviedb.org: Settings > API. $kTmdbNotice',
          style: theme.textTheme.labelSmall,
        ),
      ],
    );
  }
}
