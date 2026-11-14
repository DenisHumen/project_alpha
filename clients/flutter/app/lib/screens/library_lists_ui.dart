// SPDX-License-Identifier: MPL-2.0
//
// LIB-010 in the app: favorites, "Add to playlist" and the version picker
// shown on play when a movie has several files (4K, 1080p, Director's Cut).
// Available when the library implements [LibraryLists] (the Rust core and
// the demo library); the v0 fallback has none.

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';

/// The lists of the app's library, or null if it has none.
LibraryLists? libraryListsOf(BuildContext context) =>
    switch (Services.read(context).library) {
      final LibraryLists l => l,
      _ => null,
    };

/// Adds [item] to the favorites or removes it, with a confirmation.
Future<void> setFavorite(
  BuildContext context,
  LibraryLists lists,
  LibraryItem item,
  bool favorite,
) async {
  final messenger = ScaffoldMessenger.maybeOf(context);
  final l = context.l10n;
  await lists.setFavorite(item.id, favorite);
  messenger
    ?..removeCurrentSnackBar()
    ..showSnackBar(
      SnackBar(
        content: Text(
          favorite ? l.libraryFavoriteAdded : l.libraryFavoriteRemoved,
        ),
      ),
    );
}

/// Picks a video playlist (or makes a new one) and adds [item] to it: a
/// movie, an episode or every episode of a show.
Future<void> showAddToPlaylistDialog(
  BuildContext context,
  LibraryLists lists,
  LibraryItem item,
) async {
  final playlists = await lists.playlists();
  if (!context.mounted) return;
  final chosen = await showDialog<_PlaylistChoice>(
    context: context,
    builder: (_) => _PlaylistDialog(playlists: playlists),
  );
  if (chosen == null || !context.mounted) return;
  final messenger = ScaffoldMessenger.maybeOf(context);
  final l = context.l10n;
  final playlist = switch (chosen) {
    _Existing(:final playlist) => playlist,
    _New(:final name) => await lists.createPlaylist(name),
  };
  final added = await lists.addToPlaylist(playlist.id, item.id);
  messenger
    ?..removeCurrentSnackBar()
    ..showSnackBar(
      SnackBar(
        content: Text(
          added > 0
              ? l.libraryPlaylistAdded(playlist.name)
              : l.libraryPlaylistAlready(playlist.name),
        ),
      ),
    );
}

sealed class _PlaylistChoice {
  const _PlaylistChoice();
}

class _Existing extends _PlaylistChoice {
  const _Existing(this.playlist);
  final VideoPlaylist playlist;
}

class _New extends _PlaylistChoice {
  const _New(this.name);
  final String name;
}

class _PlaylistDialog extends StatefulWidget {
  const _PlaylistDialog({required this.playlists});

  final List<VideoPlaylist> playlists;

  @override
  State<_PlaylistDialog> createState() => _PlaylistDialogState();
}

class _PlaylistDialogState extends State<_PlaylistDialog> {
  final _name = TextEditingController();

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  void _create() {
    final name = _name.text.trim();
    if (name.isNotEmpty) Navigator.pop<_PlaylistChoice>(context, _New(name));
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    return AlertDialog(
      title: Text(l.libraryMenuPlaylist),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (widget.playlists.isEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: Text(
                    l.libraryPlaylistEmpty,
                    style: theme.textTheme.bodyMedium?.copyWith(
                      color: KColors.textSecondary,
                    ),
                  ),
                ),
              for (final p in widget.playlists)
                ListTile(
                  key: ValueKey('playlist-${p.id}'),
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.playlist_play_rounded),
                  title: Text(p.name, overflow: TextOverflow.ellipsis),
                  subtitle: Text(l.libraryPlaylistCount(p.itemCount)),
                  onTap: () =>
                      Navigator.pop<_PlaylistChoice>(context, _Existing(p)),
                ),
              const SizedBox(height: 8),
              TextField(
                key: const ValueKey('playlist-new-name'),
                controller: _name,
                autofocus: widget.playlists.isEmpty,
                decoration: InputDecoration(
                  labelText: l.libraryPlaylistNew,
                  hintText: l.libraryPlaylistName,
                ),
                onSubmitted: (_) => _create(),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(l.commonCancel),
        ),
        FilledButton(
          key: const ValueKey('playlist-create'),
          onPressed: _create,
          child: Text(l.libraryPlaylistCreate),
        ),
      ],
    );
  }
}

/// The item to play: [item] itself, or — when it has several versions —
/// the version the user picks, as an item playing that file. Null if the
/// picker was dismissed.
Future<LibraryItem?> chooseVersion(
  BuildContext context,
  LibraryItem item,
) async {
  final lists = libraryListsOf(context);
  if (item.versionCount < 2 || lists == null) return item;
  final List<MediaVersion> versions;
  try {
    versions = await lists.versions(item.id);
  } catch (_) {
    return item;
  }
  if (versions.length < 2 || !context.mounted) return item;
  final picked = await showDialog<MediaVersion>(
    context: context,
    builder: (_) =>
        _VersionDialog(item: item, lists: lists, versions: versions),
  );
  if (picked == null) return null;
  return item.withFile(
    picked.filePath,
    streamUri: picked.streamUri,
    durationMs: picked.durationMs,
    unplayable: picked.unplayable,
  );
}

class _VersionDialog extends StatefulWidget {
  const _VersionDialog({
    required this.item,
    required this.lists,
    required this.versions,
  });

  final LibraryItem item;
  final LibraryLists lists;
  final List<MediaVersion> versions;

  @override
  State<_VersionDialog> createState() => _VersionDialogState();
}

class _VersionDialogState extends State<_VersionDialog> {
  late List<MediaVersion> _versions = widget.versions;

  Future<void> _makeDefault(MediaVersion v) async {
    await widget.lists.setDefaultVersion(widget.item.id, v.id);
    final fresh = await widget.lists.versions(widget.item.id);
    if (mounted) setState(() => _versions = fresh);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    return SimpleDialog(
      title: Text(l.libraryVersionsTitle),
      children: [
        for (final v in _versions)
          ListTile(
            key: ValueKey('version-${v.id}'),
            autofocus: v.isDefault,
            leading: Icon(
              v.isDefault ? Icons.play_circle_fill_rounded : Icons.play_circle,
            ),
            title: Text(v.label, overflow: TextOverflow.ellipsis),
            subtitle: v.isDefault
                ? Text(
                    l.libraryVersionDefault,
                    style: theme.textTheme.labelSmall?.copyWith(
                      color: KColors.textSecondary,
                    ),
                  )
                : null,
            trailing: v.isDefault
                ? null
                : IconButton(
                    key: ValueKey('version-default-${v.id}'),
                    tooltip: l.libraryVersionMakeDefault,
                    icon: const Icon(Icons.push_pin_outlined),
                    onPressed: () => _makeDefault(v),
                  ),
            onTap: () => Navigator.pop<MediaVersion>(context, v),
          ),
      ],
    );
  }
}
