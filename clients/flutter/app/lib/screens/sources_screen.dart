// SPDX-License-Identifier: MPL-2.0
//
// The Sources screen (UI-005): every source with its status (online, last
// scan, last error in plain words), indexing progress and actions — rescan,
// check connection, speed test, edit, remove.

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../sources/source_manager.dart';
import '../sources/source_text.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import 'add_source_dialog.dart';

class SourcesScreen extends StatefulWidget {
  const SourcesScreen({super.key, this.checkOnOpen = true});

  /// Check every source when the screen opens (tests turn it off).
  final bool checkOnOpen;

  @override
  State<SourcesScreen> createState() => _SourcesScreenState();
}

class _SourcesScreenState extends State<SourcesScreen> {
  final Set<String> _checking = {};

  @override
  void initState() {
    super.initState();
    if (widget.checkOnOpen) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _checkAll());
    }
  }

  Future<void> _checkAll() async {
    if (!mounted) return;
    final services = Services.read(context);
    await services.refreshSources();
    await Future.wait([
      for (final s in services.managedSources)
        if (s.enabled) _check(s.id, quiet: true),
    ]);
  }

  /// Checks a source; the result shows in its status.
  Future<void> _check(String id, {bool quiet = false}) async {
    final services = Services.read(context);
    setState(() => _checking.add(id));
    try {
      await services.checkSource(id);
    } on SourceException catch (e) {
      if (!quiet && mounted) _showError(e);
    } finally {
      if (mounted) setState(() => _checking.remove(id));
    }
  }

  void _showError(SourceException e) => showDialog<void>(
    context: context,
    builder: (c) => AlertDialog(
      content: SizedBox(
        width: 420,
        child: SourceErrorView(code: e.code, detail: e.detail),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(c),
          child: Text(c.l10n.sourcesClose),
        ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final manager = services.sourceManager;
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final l = context.l10n;
    final sources = services.managedSources;
    final secretWarning = manager.secretWarning;
    final listError = services.managedSourcesError;
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
            PageTitle(l.sourcesTitle),
            Padding(
              padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
              child: Align(
                alignment: Alignment.topLeft,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 760),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      if (manager.reduced)
                        _Notice(
                          icon: Icons.info_outline,
                          text: l.sourcesReducedBanner(kAppName),
                        ),
                      if (secretWarning != null)
                        _Notice(
                          icon: Icons.lock_outline,
                          text: l.sourcesSecretWarning(secretWarning),
                        ),
                      if (listError != null)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 16),
                          child: SourceErrorView(
                            code: listError is SourceException
                                ? listError.code
                                : SourceErrorCode.other,
                            detail: listError is SourceException
                                ? listError.detail
                                : '$listError',
                          ),
                        ),
                      if (sources.isEmpty)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 16),
                          child: Text(
                            l.sourcesEmpty,
                            style: theme.textTheme.bodyMedium,
                          ),
                        ),
                      for (final s in sources)
                        _SourceCard(
                          key: ValueKey('source-card-${s.id}'),
                          source: s,
                          checking: _checking.contains(s.id),
                          onCheck: () => _check(s.id),
                          onError: _showError,
                        ),
                      const SizedBox(height: 8),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: KButton(
                          label: l.addSourceTitle,
                          icon: Icons.add,
                          primary: sources.isEmpty,
                          onPressed: () => showAddSourceDialog(context),
                        ),
                      ),
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

class _Notice extends StatelessWidget {
  const _Notice({required this.icon, required this.text});

  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      margin: const EdgeInsets.only(bottom: 16),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: KColors.bgElevated,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: KColors.stroke),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, color: KColors.textSecondary),
          const SizedBox(width: 12),
          Expanded(child: Text(text, style: theme.textTheme.bodySmall)),
        ],
      ),
    );
  }
}

enum _Online { online, offline, unknown, checking, scanning }

class _SourceCard extends StatelessWidget {
  const _SourceCard({
    super.key,
    required this.source,
    required this.checking,
    required this.onCheck,
    required this.onError,
  });

  final ManagedSource source;
  final bool checking;
  final VoidCallback onCheck;
  final void Function(SourceException e) onError;

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final manager = services.sourceManager;
    final theme = Theme.of(context);
    final d = KDensity.of(context);
    final l = context.l10n;
    final st = source.status;
    final scanningHere =
        services.scanning && services.scanningSources.contains(source.id);
    final online = scanningHere
        ? _Online.scanning
        : checking
        ? _Online.checking
        : switch (st.online) {
            true => _Online.online,
            false => _Online.offline,
            null => _Online.unknown,
          };

    final session = services.sessionScans[source.id];
    final lastScan = [
      st.lastScanFinishedAt,
      session?.at,
    ].whereType<DateTime>().fold<DateTime?>(
      null,
      (a, b) => a == null || b.isAfter(a) ? b : a,
    );
    final rawError =
        st.currentError ??
        (session != null && session.at == lastScan ? session.error : null);

    final lines = <String>[
      source.type == SourceType.local
          ? _displayPath(source.address)
          : source.address,
      if (source.username != null) l.settingsSourceUser(source.username!),
      lastScan == null
          ? l.sourcesNeverScanned
          : l.sourcesLastScan(formatAgo(l, lastScan)),
      if (st.speedBytesPerSec != null && st.latencyMs != null)
        l.sourcesSpeedLine(
          formatSpeed(l, st.speedBytesPerSec!),
          l.sourcesLatencyMs(st.latencyMs!),
        ),
      if (!source.type.indexable)
        l.sourcesNotIndexable
      else if (!source.scanIntoLibrary)
        l.sourcesBrowseOnly,
    ];

    final p = services.scanProgress;
    final canScan = !services.scanning;

    return Container(
      margin: EdgeInsets.only(bottom: d.isTv ? 12 : 16),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: KColors.bgElevated,
        borderRadius: BorderRadius.circular(d.panelRadius * .75),
        border: Border.all(color: KColors.stroke),
      ),
      child: FocusTraversalGroup(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(sourceTypeIcon(source.type), color: KColors.textSecondary),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        source.name,
                        style: theme.textTheme.titleMedium,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        sourceTypeLabel(l, source.type),
                        style: theme.textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                _StatusChip(online),
              ],
            ),
            const SizedBox(height: 8),
            for (final line in lines)
              Text(
                line,
                style: theme.textTheme.bodySmall,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
            if (rawError != null) ...[
              const SizedBox(height: 8),
              SourceErrorView(
                code: classifySourceMessage(rawError),
                detail: rawError,
              ),
            ],
            if (scanningHere && p != null) ...[
              const SizedBox(height: 10),
              ClipRRect(
                borderRadius: BorderRadius.circular(4),
                child: LinearProgressIndicator(
                  value: (p.total ?? 0) == 0 ? null : p.processed / p.total!,
                  minHeight: 6,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                switch (p.phase) {
                  ScanPhase.connecting => l.settingsScanConnecting,
                  ScanPhase.metadata => l.settingsScanMetadata(
                    p.processed,
                    p.total ?? p.filesFound,
                  ),
                  _ => l.settingsScanFiles(
                    p.processed,
                    p.total ?? p.filesFound,
                  ),
                },
                style: theme.textTheme.labelSmall,
              ),
            ],
            const SizedBox(height: 10),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                if (source.type.indexable)
                  KButton(
                    label: l.sourcesRescan,
                    icon: Icons.sync,
                    onPressed: canScan
                        ? () => services.scan(sourceId: source.id)
                        : null,
                  ),
                KButton(
                  label: l.sourcesCheck,
                  icon: Icons.network_check,
                  onPressed: checking ? null : onCheck,
                ),
                if (manager.canSpeedTest)
                  KButton(
                    label: l.sourcesSpeedTest,
                    icon: Icons.speed,
                    onPressed: () => _speedTest(context),
                  ),
                if (manager.canEdit)
                  KButton(
                    label: l.sourcesEdit,
                    icon: Icons.edit_outlined,
                    onPressed: () => showEditSourceDialog(context, source),
                  ),
                KButton(
                  label: l.settingsRemove,
                  icon: Icons.delete_outline,
                  onPressed: services.scanning ? null : () => _remove(context),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  /// `file:///a/b` (how the core stores folders) as a path.
  static String _displayPath(String address) {
    if (!address.startsWith('file://')) return address;
    try {
      return Uri.parse(address).toFilePath();
    } on UnsupportedError {
      return address;
    }
  }

  Future<void> _remove(BuildContext context) async {
    final l = context.l10n;
    final services = Services.read(context);
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
    if (ok != true) return;
    try {
      await services.deleteSource(source.id);
    } on SourceException catch (e) {
      onError(e);
    }
  }

  Future<void> _speedTest(BuildContext context) => showDialog<void>(
    context: context,
    builder: (_) => SpeedTestDialog(source: source),
  );
}

class _StatusChip extends StatelessWidget {
  const _StatusChip(this.state);

  final _Online state;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final (text, color) = switch (state) {
      _Online.online => (l.sourcesStatusOnline, const Color(0xFF4CAF7A)),
      _Online.offline => (l.sourcesStatusOffline, theme.colorScheme.error),
      _Online.unknown => (l.sourcesStatusUnknown, KColors.textTertiary),
      _Online.checking => (l.sourcesStatusChecking, KColors.textSecondary),
      _Online.scanning => (l.settingsScanning, theme.colorScheme.primary),
    };
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 150),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(999),
          border: Border.all(color: color),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.circle, size: 8, color: color),
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                text,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.labelSmall?.copyWith(color: color),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Runs a speed test and shows the result in plain words.
class SpeedTestDialog extends StatefulWidget {
  const SpeedTestDialog({super.key, required this.source});

  final ManagedSource source;

  @override
  State<SpeedTestDialog> createState() => _SpeedTestDialogState();
}

class _SpeedTestDialogState extends State<SpeedTestDialog> {
  SpeedResult? _result;
  SourceException? _error;

  @override
  void initState() {
    super.initState();
    unawaited(_run());
  }

  Future<void> _run() async {
    final services = Services.read(context);
    try {
      final r = await services.speedTest(widget.source.id);
      if (mounted) setState(() => _result = r);
    } on SourceException catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final r = _result;
    final e = _error;
    final Widget body;
    if (e != null) {
      body = SourceErrorView(code: e.code, detail: e.detail);
    } else if (r == null) {
      body = Row(
        children: [
          const SizedBox.square(
            dimension: 20,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text(l.sourcesSpeedRunning)),
        ],
      );
    } else {
      final speed = r.bytesPerSec;
      body = Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (speed != null)
            Text(
              l.sourcesSpeedRead(formatSpeed(l, speed)),
              style: theme.textTheme.titleMedium,
            ),
          Text(l.sourcesSpeedResponse(l.sourcesLatencyMs(r.latencyMs))),
          const SizedBox(height: 8),
          if (speed == null)
            Text(l.sourcesSpeedNoFile, style: theme.textTheme.bodySmall)
          else ...[
            if (speed < slowFor4kBytesPerSec)
              Text(l.sourcesSpeedSlow, style: theme.textTheme.bodySmall),
            if (r.file != null)
              Text(
                l.sourcesSpeedFile(r.file!),
                style: theme.textTheme.labelSmall,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
          ],
        ],
      );
    }
    return AlertDialog(
      title: Text(l.sourcesSpeedResultTitle(widget.source.name)),
      content: SizedBox(width: 420, child: body),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(l.sourcesClose),
        ),
      ],
    );
  }
}
