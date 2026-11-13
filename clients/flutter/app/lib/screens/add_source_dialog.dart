// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:permission_handler/permission_handler.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';

/// Adds a local folder or an SMB share. SMB passwords go to the platform
/// keystore (flutter_secure_storage), never to the library database.
Future<MediaSource?> showAddSourceDialog(
  BuildContext context, {
  bool scanAfter = true,
}) async {
  final added = await showDialog<MediaSource>(
    context: context,
    builder: (_) => const _AddSourceDialog(),
  );
  if (added != null && scanAfter && context.mounted) {
    Services.read(context).scan(sourceId: added.id);
  }
  return added;
}

class _AddSourceDialog extends StatefulWidget {
  const _AddSourceDialog();

  @override
  State<_AddSourceDialog> createState() => _AddSourceDialogState();
}

class _AddSourceDialogState extends State<_AddSourceDialog> {
  SourceKind _kind = SourceKind.localFolder;
  static final _smbUnsupported = Platform.isAndroid;
  final _name = TextEditingController();
  final _path = TextEditingController();
  final _smbUrl = TextEditingController(text: 'smb://');
  final _user = TextEditingController();
  final _password = TextEditingController();
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    for (final c in [_name, _path, _smbUrl, _user, _password]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _browse() async {
    final l = context.l10n;
    try {
      final dir = await FilePicker.getDirectoryPath(
        dialogTitle: l.addSourcePickerTitle,
      );
      if (dir != null) {
        setState(() {
          _path.text = dir;
          if (_name.text.isEmpty) {
            _name.text =
                dir
                    .split(Platform.pathSeparator)
                    .where((s) => s.isNotEmpty)
                    .lastOrNull ??
                dir;
          }
        });
      }
    } catch (e) {
      setState(() => _error = l.addSourcePickerUnavailable('$e'));
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

  String? _validate() {
    if (_kind == SourceKind.localFolder) {
      if (_path.text.trim().isEmpty) return context.l10n.addSourceErrorNoFolder;
      return null;
    }
    final u = Uri.tryParse(_smbUrl.text.trim());
    if (u == null || u.scheme != 'smb' || u.host.isEmpty) {
      return context.l10n.addSourceErrorSmbForm;
    }
    return null;
  }

  Future<void> _save() async {
    final err = _validate();
    if (err != null) {
      setState(() => _error = err);
      return;
    }
    final services = Services.read(context);
    final permissionError = context.l10n.addSourceErrorPermission;
    setState(() => _busy = true);
    if (_kind == SourceKind.localFolder && !await _ensureStoragePermission()) {
      setState(() {
        _busy = false;
        _error = permissionError;
      });
      return;
    }
    final isSmb = _kind == SourceKind.smb;
    final path = isSmb ? _smbUrl.text.trim() : _path.text.trim();
    final uri = isSmb ? Uri.parse(path) : null;
    final source = MediaSource(
      id: '',
      kind: _kind,
      name: _name.text.trim().isNotEmpty
          ? _name.text.trim()
          : (uri != null ? '${uri.host}${uri.path}' : path),
      path: path,
      username: isSmb && _user.text.trim().isNotEmpty
          ? _user.text.trim()
          : null,
    );
    final added = await services.addSource(
      source,
      smbPassword: isSmb ? _password.text : null,
    );
    if (mounted) Navigator.of(context).pop(added);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final l = context.l10n;
    return AlertDialog(
      title: Text(l.addSourceTitle),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SegmentedButton<SourceKind>(
                segments: [
                  ButtonSegment(
                    value: SourceKind.localFolder,
                    icon: const Icon(Icons.folder_outlined),
                    label: Text(l.addSourceFolder),
                  ),
                  // pa_library 0.1 has no SMB client on Android (SmbUnsupportedException).
                  ButtonSegment(
                    value: SourceKind.smb,
                    enabled: !_smbUnsupported,
                    icon: const Icon(Icons.lan_outlined),
                    label: Text(
                      _smbUnsupported ? l.addSourceSmbSoon : l.addSourceSmbBeta,
                    ),
                  ),
                ],
                selected: {_kind},
                onSelectionChanged: (s) => setState(() {
                  _kind = s.first;
                  _error = null;
                }),
              ),
              const SizedBox(height: 16),
              if (_kind == SourceKind.localFolder) ...[
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _path,
                        decoration: InputDecoration(
                          labelText: l.addSourceFolder,
                          hintText: l.addSourceFolderHint,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    FilledButton.tonalIcon(
                      onPressed: _browse,
                      icon: const Icon(Icons.folder_open),
                      label: Text(l.addSourceBrowse),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Text(l.addSourceFolderTip, style: theme.textTheme.bodySmall),
              ] else ...[
                TextField(
                  controller: _smbUrl,
                  keyboardType: TextInputType.url,
                  decoration: InputDecoration(
                    labelText: l.addSourceShareAddress,
                    hintText: l.addSourceShareHint,
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _user,
                  decoration: InputDecoration(labelText: l.addSourceUsername),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _password,
                  obscureText: true,
                  decoration: InputDecoration(
                    labelText: l.addSourcePassword,
                    helperText: l.addSourcePasswordHelper,
                  ),
                ),
              ],
              const SizedBox(height: 12),
              TextField(
                controller: _name,
                decoration: InputDecoration(labelText: l.addSourceDisplayName),
              ),
              if (_error != null) ...[
                const SizedBox(height: 12),
                Text(
                  _error!,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.error,
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _busy ? null : () => Navigator.pop(context),
          child: Text(l.commonCancel),
        ),
        FilledButton(
          onPressed: _busy ? null : _save,
          child: Text(l.addSourceSubmit),
        ),
      ],
    );
  }
}
