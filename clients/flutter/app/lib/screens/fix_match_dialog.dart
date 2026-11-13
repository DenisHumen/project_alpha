// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';

typedef _FixMatch = ({String? title, int? year, int? tmdbId});

/// Manual match correction: title/year or a TMDB id (contract `fixMatch`).
Future<void> showFixMatchDialog(BuildContext context, LibraryItem item) async {
  final result = await showDialog<_FixMatch>(
    context: context,
    builder: (_) => _FixMatchDialog(item: item),
  );
  if (result == null || !context.mounted) return;
  final messenger = ScaffoldMessenger.maybeOf(context);
  final updated = context.l10n.fixMatchUpdated;
  await Services.read(context).library.fixMatch(
    item.id,
    tmdbId: result.tmdbId,
    title: result.title,
    year: result.year,
  );
  messenger?.showSnackBar(SnackBar(content: Text(updated)));
}

class _FixMatchDialog extends StatefulWidget {
  const _FixMatchDialog({required this.item});
  final LibraryItem item;

  @override
  State<_FixMatchDialog> createState() => _FixMatchDialogState();
}

class _FixMatchDialogState extends State<_FixMatchDialog> {
  late final _title = TextEditingController(text: widget.item.title);
  late final _year = TextEditingController(
    text: widget.item.year?.toString() ?? '',
  );
  final _tmdb = TextEditingController();

  @override
  void dispose() {
    _title.dispose();
    _year.dispose();
    _tmdb.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      title: Text(l.commonFixMatch),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                widget.item.filePath,
                style: Theme.of(context).textTheme.labelSmall,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
              const SizedBox(height: 16),
              TextField(
                controller: _title,
                autofocus: true,
                decoration: InputDecoration(labelText: l.fixMatchFieldTitle),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _year,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(labelText: l.fixMatchFieldYear),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _tmdb,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(
                  labelText: l.fixMatchFieldTmdbId,
                  helperText: l.fixMatchTmdbHelper,
                ),
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
          onPressed: () => Navigator.pop<_FixMatch>(context, (
            title: _title.text.trim().isEmpty ? null : _title.text.trim(),
            year: int.tryParse(_year.text.trim()),
            tmdbId: int.tryParse(_tmdb.text.trim()),
          )),
          child: Text(l.commonSave),
        ),
      ],
    );
  }
}
