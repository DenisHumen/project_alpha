// SPDX-License-Identifier: MPL-2.0
//
// Adding and editing sources (UI-005): sources found on the network first,
// then the kinds that can be added by hand; forms for a local folder, SMB
// and WebDAV. Other kinds are listed as "soon".

import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:permission_handler/permission_handler.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../sources/discovery.dart';
import '../sources/source_address.dart';
import '../sources/source_manager.dart';
import '../sources/source_text.dart';

/// Opens the add flow. With [type] the kind chooser is skipped. After a
/// source is added it is checked and — if it goes to the library — scanned.
Future<MediaSource?> showAddSourceDialog(
  BuildContext context, {
  bool scanAfter = true,
  SourceType? type,
}) async {
  final services = Services.read(context);
  final added = await showDialog<ManagedSource>(
    context: context,
    builder: (_) => AddSourceDialog(initialType: type),
  );
  if (added == null) return null;
  unawaited(_checkQuietly(services, added.id));
  if (scanAfter && added.type.indexable && added.scanIntoLibrary) {
    unawaited(services.scan(sourceId: added.id));
  }
  return services.sources.where((s) => s.id == added.id).firstOrNull;
}

Future<void> _checkQuietly(AppServices services, String id) async {
  try {
    await services.checkSource(id);
  } on SourceException {
    // The result is in the source status.
  }
}

/// Opens the form of an existing source.
Future<ManagedSource?> showEditSourceDialog(
  BuildContext context,
  ManagedSource source,
) => showDialog<ManagedSource>(
  context: context,
  builder: (_) => _FormDialog(
    child: SourceForm(type: source.type, initial: source),
  ),
);

class _FormDialog extends StatelessWidget {
  const _FormDialog({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) => Dialog(
    child: ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 520),
      child: child,
    ),
  );
}

/// Kind chooser, then the form of the chosen kind.
class AddSourceDialog extends StatefulWidget {
  const AddSourceDialog({super.key, this.initialType});

  final SourceType? initialType;

  @override
  State<AddSourceDialog> createState() => _AddSourceDialogState();
}

class _AddSourceDialogState extends State<AddSourceDialog> {
  SourceType? _type;
  DiscoveredSource? _found;

  @override
  void initState() {
    super.initState();
    _type = widget.initialType;
  }

  @override
  Widget build(BuildContext context) {
    final type = _type;
    return _FormDialog(
      child: type == null
          ? _KindChooser(
              onPick: (t, [found]) => setState(() {
                _type = t;
                _found = found;
              }),
            )
          : SourceForm(
              type: type,
              discovered: _found,
              onBack: widget.initialType == null
                  ? () => setState(() {
                      _type = null;
                      _found = null;
                    })
                  : null,
            ),
    );
  }
}

class _KindChooser extends StatefulWidget {
  const _KindChooser({required this.onPick});

  final void Function(SourceType type, [DiscoveredSource? found]) onPick;

  @override
  State<_KindChooser> createState() => _KindChooserState();
}

class _KindChooserState extends State<_KindChooser> {
  /// One subscription while the chooser is shown: LAN browsing runs as
  /// long as someone listens (NET-020), so rebuilds must not restart it.
  Stream<List<DiscoveredSource>>? _found;

  void onPick(SourceType type, [DiscoveredSource? found]) =>
      widget.onPick(type, found);

  @override
  Widget build(BuildContext context) {
    final services = Services.read(context);
    final discovered = _found ??= services.discovery.watch();
    final manager = services.sourceManager;
    final l = context.l10n;
    final theme = Theme.of(context);

    String? badge(SourceType t) {
      if (!SourceType.available.contains(t)) return l.sourcesSoon;
      // SMB everywhere through the core (libsmb2 in the APK, LIB-019); the
      // v0 fallback manager cannot create it and says so below.
      if (!manager.creatableTypes.contains(t)) {
        return l.sourcesUnavailableReduced;
      }
      return null;
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _DialogTitle(l.addSourceTitle),
        Flexible(
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.only(bottom: 8),
            children: [
              if (services.discovery.supported) ...[
                _Header(l.sourcesAddDiscovered),
                StreamBuilder<List<DiscoveredSource>>(
                  stream: discovered,
                  builder: (context, snap) {
                    final found = [
                      for (final d in snap.data ?? const <DiscoveredSource>[])
                        if (manager.creatableTypes.contains(d.type)) d,
                    ];
                    if (found.isEmpty) {
                      return Padding(
                        padding: const EdgeInsets.fromLTRB(24, 4, 24, 8),
                        child: Text(
                          l.sourcesAddDiscoveredEmpty,
                          style: theme.textTheme.bodySmall,
                        ),
                      );
                    }
                    return Column(
                      children: [
                        for (final d in found)
                          ListTile(
                            leading: Icon(sourceTypeIcon(d.type)),
                            title: Text(d.name),
                            subtitle: Text(
                              '${sourceTypeLabel(l, d.type)} · ${d.host}',
                            ),
                            onTap: () => onPick(d.type, d),
                          ),
                      ],
                    );
                  },
                ),
                _Header(l.sourcesAddManual),
              ],
              for (final t in SourceType.values)
                Builder(
                  builder: (context) {
                    final b = badge(t);
                    return ListTile(
                      enabled: b == null,
                      leading: Icon(sourceTypeIcon(t)),
                      title: Text(sourceTypeLabel(l, t)),
                      trailing: b == null
                          ? const Icon(Icons.chevron_right)
                          : _Badge(b),
                      onTap: b == null ? () => onPick(t) : null,
                    );
                  },
                ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
          child: Align(
            alignment: Alignment.centerRight,
            child: TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(l.commonCancel),
            ),
          ),
        ),
      ],
    );
  }
}

class _DialogTitle extends StatelessWidget {
  const _DialogTitle(this.text, {this.onBack});

  final String text;
  final VoidCallback? onBack;

  @override
  Widget build(BuildContext context) => Padding(
    padding: EdgeInsets.fromLTRB(onBack == null ? 24 : 8, 20, 24, 8),
    child: Row(
      children: [
        if (onBack != null)
          IconButton(
            tooltip: context.l10n.commonBack,
            onPressed: onBack,
            icon: const Icon(Icons.arrow_back),
          ),
        Expanded(
          child: Text(text, style: Theme.of(context).textTheme.headlineSmall),
        ),
      ],
    ),
  );
}

class _Header extends StatelessWidget {
  const _Header(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(24, 12, 24, 4),
    child: Text(text, style: Theme.of(context).textTheme.labelLarge),
  );
}

class _Badge extends StatelessWidget {
  const _Badge(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 140),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(8),
          border: Border.all(color: theme.colorScheme.outline),
        ),
        child: Text(
          text,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          textAlign: TextAlign.center,
          style: theme.textTheme.labelSmall,
        ),
      ),
    );
  }
}

/// Readable error with the technical message under "Details".
class SourceErrorView extends StatelessWidget {
  const SourceErrorView({super.key, required this.code, this.detail});

  final SourceErrorCode code;
  final String? detail;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final style = theme.textTheme.bodySmall?.copyWith(
      color: theme.colorScheme.error,
    );
    final d = detail;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(sourceErrorText(context.l10n, code), style: style),
        if (d != null && d.isNotEmpty)
          // Own Material: the view also sits on decorated cards.
          Theme(
            data: theme.copyWith(dividerColor: Colors.transparent),
            child: Material(
              type: MaterialType.transparency,
              child: ExpansionTile(
                tilePadding: EdgeInsets.zero,
                dense: true,
                title: Text(
                  context.l10n.sourcesErrorDetails,
                  style: theme.textTheme.labelSmall,
                ),
                children: [
                  Align(
                    alignment: Alignment.centerLeft,
                    child: SelectableText(d, style: theme.textTheme.labelSmall),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

/// Form of one source kind: add (no [initial]) or edit.
class SourceForm extends StatefulWidget {
  const SourceForm({
    super.key,
    required this.type,
    this.initial,
    this.discovered,
    this.onBack,
  });

  final SourceType type;
  final ManagedSource? initial;
  final DiscoveredSource? discovered;
  final VoidCallback? onBack;

  @override
  State<SourceForm> createState() => _SourceFormState();
}

class _SourceFormState extends State<SourceForm> {
  final _name = TextEditingController();
  final _folder = TextEditingController();
  final _host = TextEditingController();
  final _share = TextEditingController();
  final _subfolder = TextEditingController();
  final _url = TextEditingController();
  final _user = TextEditingController();
  final _password = TextEditingController();
  final _domain = TextEditingController();
  final _pin = TextEditingController();
  String _smbVersion = 'any';
  String _smbEncryption = 'auto';
  bool _signing = false;
  bool _scanIntoLibrary = true;
  bool _autoScan = true;
  bool _forgetPassword = false;

  /// Field errors are shown after the first submit.
  bool _submitted = false;
  bool _busy = false;
  SourceException? _error;
  String? _pickerError;

  bool get _editing => widget.initial != null;

  @override
  void initState() {
    super.initState();
    final s = widget.initial;
    final d = widget.discovered;
    if (s != null) {
      _name.text = s.name;
      _user.text = s.username ?? '';
      _scanIntoLibrary = s.scanIntoLibrary;
      _autoScan = s.autoScan;
      _smbVersion = s.smb.version;
      _smbEncryption = s.smb.encryption;
      _signing = s.smb.signingRequired;
      _domain.text = s.smb.domain ?? '';
      _pin.text = s.tlsPin ?? '';
      switch (s.type) {
        case SourceType.smb:
          final p = splitSmb(s.address);
          _host.text = p?.host ?? '';
          _share.text = p?.share ?? '';
          _subfolder.text = p?.folder ?? '';
        case SourceType.webdav:
          _url.text = s.address;
        default:
          _folder.text = _localPath(s.address);
      }
    } else if (d != null) {
      _name.text = d.name;
      // The core's URL is built from the IP address (NET-020); the host
      // name is the fallback.
      if (d.type == SourceType.smb) {
        _host.text = splitSmb(d.url ?? '')?.host ?? d.host;
      } else if (d.type == SourceType.webdav) {
        final port = d.port == null ? '' : ':${d.port}';
        final scheme = d.secure ? 'https' : 'http';
        _url.text = d.url ?? '$scheme://${d.host}$port${d.path ?? '/'}';
      }
    }
    for (final c in [_host, _share, _subfolder, _url, _folder]) {
      c.addListener(_changed);
    }
  }

  void _changed() => setState(() {});

  /// `file:///a/b` (how the core stores folders) as a path.
  static String _localPath(String address) {
    if (!address.startsWith('file://')) return address;
    try {
      return Uri.parse(address).toFilePath();
    } on UnsupportedError {
      return address;
    }
  }

  @override
  void dispose() {
    for (final c in [
      _name,
      _folder,
      _host,
      _share,
      _subfolder,
      _url,
      _user,
      _password,
      _domain,
      _pin,
    ]) {
      c.dispose();
    }
    super.dispose();
  }

  AddressResult _address() => switch (widget.type) {
    SourceType.smb => smbAddress(
      host: _host.text,
      share: _share.text,
      folder: _subfolder.text,
    ),
    SourceType.webdav => webdavAddress(_url.text),
    _ => localAddress(_folder.text),
  };

  AddressResult? _pinResult() =>
      widget.type == SourceType.webdav ? tlsPin(_pin.text) : null;

  String? _fieldError(SourceFormError? e, Set<SourceFormError> mine) {
    if (!_submitted || e == null || !mine.contains(e)) return null;
    return sourceFormErrorText(context.l10n, e);
  }

  Future<void> _browse() async {
    final l = context.l10n;
    try {
      final dir = await FilePicker.getDirectoryPath(
        dialogTitle: l.addSourcePickerTitle,
      );
      if (dir != null) setState(() => _folder.text = dir);
    } catch (e) {
      setState(() => _pickerError = l.addSourcePickerUnavailable('$e'));
    }
  }

  Future<bool> _ensureStoragePermission() async {
    if (!Platform.isAndroid) return true;
    final statuses = await [
      Permission.videos,
      Permission.audio,
      Permission.storage,
    ].request();
    return statuses.values.any((s) => s.isGranted || s.isLimited);
  }

  Future<void> _submit() async {
    setState(() {
      _submitted = true;
      _error = null;
    });
    final address = _address();
    final pin = _pinResult();
    if (address.error != null || pin?.error != null) return;
    final services = Services.read(context);
    final permissionError = context.l10n.addSourceErrorPermission;
    setState(() => _busy = true);
    if (widget.type == SourceType.local && !await _ensureStoragePermission()) {
      setState(() {
        _busy = false;
        _pickerError = permissionError;
      });
      return;
    }
    final user = _user.text.trim();
    final String? password;
    if (widget.type == SourceType.local) {
      password = null;
    } else if (_editing) {
      password = _forgetPassword
          ? ''
          : (_password.text.isEmpty ? null : _password.text);
    } else {
      password = _password.text.isEmpty ? null : _password.text;
    }
    final draft = SourceDraft(
      type: widget.type,
      name: _name.text.trim().isNotEmpty
          ? _name.text.trim()
          : suggestSourceName(address.address!),
      address: address.address!,
      username: widget.type == SourceType.local || user.isEmpty ? null : user,
      password: password,
      smb: SmbSettings(
        domain: _domain.text.trim().isEmpty ? null : _domain.text.trim(),
        version: _smbVersion,
        encryption: _smbEncryption,
        signingRequired: _signing,
      ),
      tlsPin: pin?.address,
      scanIntoLibrary: widget.type.indexable && _scanIntoLibrary,
      autoScan: _autoScan,
    );
    try {
      final saved = _editing
          ? await services.updateSource(widget.initial!.id, draft)
          : await services.createSource(draft);
      if (mounted) Navigator.of(context).pop(saved);
    } on SourceException catch (e) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = e;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final address = _address();
    final addrError = address.error;
    final type = widget.type;
    final title = _editing
        ? l.sourcesEditTitle
        : '${l.addSourceTitle}: ${sourceTypeLabel(l, type)}';
    final submitLabel = _editing
        ? l.commonSave
        : (type.indexable && _scanIntoLibrary
              ? l.addSourceSubmit
              : l.sourcesFormAdd);
    const gap = SizedBox(height: 12);

    final fields = <Widget>[
      if (type == SourceType.local) ...[
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: TextField(
                key: const ValueKey('source-folder'),
                controller: _folder,
                decoration: InputDecoration(
                  labelText: l.addSourceFolder,
                  hintText: l.addSourceFolderHint,
                  errorText: _fieldError(addrError, {
                    SourceFormError.folderEmpty,
                    SourceFormError.folderNotAbsolute,
                  }),
                  errorMaxLines: 3,
                ),
              ),
            ),
            const SizedBox(width: 8),
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: IconButton.filledTonal(
                tooltip: l.addSourceBrowse,
                onPressed: _busy ? null : _browse,
                icon: const Icon(Icons.folder_open),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Text(l.addSourceFolderTip, style: theme.textTheme.bodySmall),
        if (_pickerError != null) ...[
          const SizedBox(height: 8),
          Text(
            _pickerError!,
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.error,
            ),
          ),
        ],
      ],
      if (type == SourceType.smb) ...[
        TextField(
          key: const ValueKey('source-host'),
          controller: _host,
          keyboardType: TextInputType.url,
          decoration: InputDecoration(
            labelText: l.sourcesFormHost,
            hintText: l.sourcesFormHostHint,
            errorText: _fieldError(addrError, {
              SourceFormError.hostEmpty,
              SourceFormError.credentialsInAddress,
            }),
            errorMaxLines: 3,
          ),
        ),
        gap,
        TextField(
          key: const ValueKey('source-share'),
          controller: _share,
          decoration: InputDecoration(
            labelText: l.sourcesFormShare,
            hintText: l.sourcesFormShareHint,
            errorText: _fieldError(addrError, {SourceFormError.shareEmpty}),
            errorMaxLines: 3,
          ),
        ),
        gap,
        TextField(
          key: const ValueKey('source-subfolder'),
          controller: _subfolder,
          decoration: InputDecoration(
            labelText: l.sourcesFormSubfolder,
            hintText: l.sourcesFormSubfolderHint,
          ),
        ),
        if (address.address != null) ...[
          const SizedBox(height: 6),
          Text(
            l.sourcesFormAddress(address.address!),
            style: theme.textTheme.labelSmall,
          ),
        ],
      ],
      if (type == SourceType.webdav)
        TextField(
          key: const ValueKey('source-url'),
          controller: _url,
          keyboardType: TextInputType.url,
          decoration: InputDecoration(
            labelText: l.sourcesFormUrl,
            hintText: l.sourcesFormUrlHint,
            errorText: _fieldError(addrError, {
              SourceFormError.urlEmpty,
              SourceFormError.urlScheme,
              SourceFormError.credentialsInAddress,
            }),
            errorMaxLines: 3,
          ),
        ),
      if (type != SourceType.local) ...[
        gap,
        TextField(
          key: const ValueKey('source-user'),
          controller: _user,
          autocorrect: false,
          decoration: InputDecoration(labelText: l.addSourceUsername),
        ),
        gap,
        TextField(
          key: const ValueKey('source-password'),
          controller: _password,
          obscureText: true,
          enabled: !_forgetPassword,
          decoration: InputDecoration(
            labelText: l.addSourcePassword,
            helperText: _editing && widget.initial!.hasPassword
                ? l.sourcesFormPasswordKeep
                : l.addSourcePasswordHelper,
            helperMaxLines: 3,
          ),
        ),
        if (_editing && widget.initial!.hasPassword)
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            value: _forgetPassword,
            title: Text(l.sourcesFormForgetPassword),
            onChanged: (v) => setState(() => _forgetPassword = v),
          ),
        Theme(
          data: theme.copyWith(dividerColor: Colors.transparent),
          child: ExpansionTile(
            key: const ValueKey('source-advanced'),
            tilePadding: EdgeInsets.zero,
            childrenPadding: const EdgeInsets.only(bottom: 8),
            title: Text(l.sourcesFormAdvanced),
            expandedCrossAxisAlignment: CrossAxisAlignment.start,
            children: type == SourceType.smb
                ? _smbAdvanced(l)
                : _webdavAdvanced(l),
          ),
        ),
      ],
      gap,
      TextField(
        key: const ValueKey('source-name'),
        controller: _name,
        decoration: InputDecoration(labelText: l.addSourceDisplayName),
      ),
      if (type.indexable)
        SwitchListTile(
          key: const ValueKey('source-scan-into-library'),
          contentPadding: EdgeInsets.zero,
          value: _scanIntoLibrary,
          title: Text(l.sourcesFormScanIntoLibrary),
          subtitle: Text(l.sourcesFormScanIntoLibraryHint),
          onChanged: (v) => setState(() => _scanIntoLibrary = v),
        ),
      // LIB-005 watcher: events for local folders, a timer for the rest.
      if (type.indexable && _scanIntoLibrary)
        SwitchListTile(
          key: const ValueKey('source-auto-scan'),
          contentPadding: EdgeInsets.zero,
          value: _autoScan,
          title: Text(l.sourcesFormAutoScan),
          subtitle: Text(l.sourcesFormAutoScanHint),
          onChanged: (v) => setState(() => _autoScan = v),
        ),
      if (!type.indexable) ...[
        gap,
        Text(l.sourcesNotIndexable, style: theme.textTheme.bodySmall),
      ],
      if (_error != null) ...[
        gap,
        SourceErrorView(code: _error!.code, detail: _error!.detail),
      ],
    ];

    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _DialogTitle(title, onBack: _busy ? null : widget.onBack),
        Flexible(
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(24, 8, 24, 8),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: fields,
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 4, 16, 12),
          child: Wrap(
            alignment: WrapAlignment.end,
            spacing: 8,
            runSpacing: 4,
            children: [
              TextButton(
                onPressed: _busy ? null : () => Navigator.pop(context),
                child: Text(l.commonCancel),
              ),
              FilledButton(
                key: const ValueKey('source-submit'),
                onPressed: _busy ? null : _submit,
                child: Text(submitLabel),
              ),
            ],
          ),
        ),
      ],
    );
  }

  List<Widget> _smbAdvanced(AppLocalizations l) => [
    TextField(
      key: const ValueKey('source-domain'),
      controller: _domain,
      decoration: InputDecoration(labelText: l.sourcesFormDomain),
    ),
    const SizedBox(height: 12),
    Text(l.sourcesFormSmbVersion),
    _Choices(
      value: _smbVersion,
      options: {
        'any': l.sourcesFormSmbVersionAny, // l10n-ignore
        'smb2': l.sourcesFormSmbVersionSmb2, // l10n-ignore
        'smb3': l.sourcesFormSmbVersionSmb3, // l10n-ignore
      },
      onChanged: (v) => setState(() => _smbVersion = v),
    ),
    const SizedBox(height: 8),
    Text(l.sourcesFormEncryption),
    _Choices(
      value: _smbEncryption,
      options: {
        'auto': l.sourcesFormEncryptionAuto, // l10n-ignore
        'required': l.sourcesFormEncryptionRequired, // l10n-ignore
        'off': l.sourcesFormEncryptionOff, // l10n-ignore
      },
      onChanged: (v) => setState(() => _smbEncryption = v),
    ),
    SwitchListTile(
      contentPadding: EdgeInsets.zero,
      value: _signing,
      title: Text(l.sourcesFormSigning),
      onChanged: (v) => setState(() => _signing = v),
    ),
  ];

  List<Widget> _webdavAdvanced(AppLocalizations l) {
    final pin = _pinResult();
    return [
      TextField(
        key: const ValueKey('source-tls-pin'),
        controller: _pin,
        autocorrect: false,
        onChanged: (_) => setState(() {}),
        decoration: InputDecoration(
          labelText: l.sourcesFormTlsPin,
          helperText: l.sourcesFormTlsPinHelper,
          helperMaxLines: 3,
          errorText: _fieldError(pin?.error, {SourceFormError.tlsPin}),
          errorMaxLines: 3,
        ),
      ),
    ];
  }
}

class _Choices extends StatelessWidget {
  const _Choices({
    required this.value,
    required this.options,
    required this.onChanged,
  });

  final String value;
  final Map<String, String> options;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 6),
    child: Wrap(
      spacing: 8,
      runSpacing: 6,
      children: [
        for (final MapEntry(:key, value: label) in options.entries)
          ChoiceChip(
            label: Text(label),
            selected: key == value,
            onSelected: (_) => onChanged(key),
          ),
      ],
    ),
  );
}
