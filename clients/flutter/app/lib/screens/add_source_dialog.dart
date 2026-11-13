// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:permission_handler/permission_handler.dart';

import '../integration/library.dart';
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
    try {
      final dir = await FilePicker.getDirectoryPath(
        dialogTitle: 'Choose a media folder',
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
      setState(
        () => _error =
            'Folder picker is not available: $e. Type the path instead.',
      );
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
      if (_path.text.trim().isEmpty) return 'Choose a folder.';
      return null;
    }
    final u = Uri.tryParse(_smbUrl.text.trim());
    if (u == null || u.scheme != 'smb' || u.host.isEmpty) {
      return 'Use the form smb://host/share/folder';
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
    setState(() => _busy = true);
    if (_kind == SourceKind.localFolder && !await _ensureStoragePermission()) {
      setState(() {
        _busy = false;
        _error = 'Storage permission is needed to read media files.';
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
    return AlertDialog(
      title: const Text('Add source'),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SegmentedButton<SourceKind>(
                segments: const [
                  ButtonSegment(
                    value: SourceKind.localFolder,
                    icon: Icon(Icons.folder_outlined),
                    label: Text('Folder'),
                  ),
                  ButtonSegment(
                    value: SourceKind.smb,
                    icon: Icon(Icons.lan_outlined),
                    label: Text('SMB share (beta)'),
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
                        decoration: const InputDecoration(
                          labelText: 'Folder',
                          hintText: '/home/me/Videos',
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    FilledButton.tonalIcon(
                      onPressed: _browse,
                      icon: const Icon(Icons.folder_open),
                      label: const Text('Browse…'),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Text(
                  'Tip: a network share mounted by your system (e.g. via the file manager) also works as a folder.',
                  style: theme.textTheme.bodySmall,
                ),
              ] else ...[
                TextField(
                  controller: _smbUrl,
                  keyboardType: TextInputType.url,
                  decoration: const InputDecoration(
                    labelText: 'Share address',
                    hintText: 'smb://nas.local/media/Movies',
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _user,
                  decoration: const InputDecoration(
                    labelText: 'Username (optional)',
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _password,
                  obscureText: true,
                  decoration: const InputDecoration(
                    labelText: 'Password (optional)',
                    helperText:
                        'Stored in the system keystore on this device only.',
                  ),
                ),
              ],
              const SizedBox(height: 12),
              TextField(
                controller: _name,
                decoration: const InputDecoration(
                  labelText: 'Display name (optional)',
                ),
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
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: _busy ? null : _save,
          child: const Text('Add and scan'),
        ),
      ],
    );
  }
}
