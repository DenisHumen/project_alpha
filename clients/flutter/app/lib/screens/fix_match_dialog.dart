// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
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
  await Services.read(context).library.fixMatch(
    item.id,
    tmdbId: result.tmdbId,
    title: result.title,
    year: result.year,
  );
  messenger?.showSnackBar(const SnackBar(content: Text('Match updated')));
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
    return AlertDialog(
      title: const Text('Fix match'),
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
                decoration: const InputDecoration(labelText: 'Title'),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _year,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'Year'),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _tmdb,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                  labelText: 'TMDB ID (optional)',
                  helperText: 'From the address of the title on themoviedb.org',
                ),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop<_FixMatch>(context, (
            title: _title.text.trim().isEmpty ? null : _title.text.trim(),
            year: int.tryParse(_year.text.trim()),
            tmdbId: int.tryParse(_tmdb.text.trim()),
          )),
          child: const Text('Save'),
        ),
      ],
    );
  }
}
