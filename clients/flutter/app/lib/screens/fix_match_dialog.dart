// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../services/app_services.dart';

/// Manual match correction: title/year or a TMDB id (contract `fixMatch`).
Future<void> showFixMatchDialog(BuildContext context, LibraryItem item) async {
  final title = TextEditingController(text: item.title);
  final year = TextEditingController(text: item.year?.toString() ?? '');
  final tmdb = TextEditingController();
  final ok = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: const Text('Fix match'),
      content: SizedBox(
        width: 420,
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(item.filePath, style: Theme.of(context).textTheme.labelSmall, maxLines: 2, overflow: TextOverflow.ellipsis),
          const SizedBox(height: 16),
          TextField(controller: title, autofocus: true, decoration: const InputDecoration(labelText: 'Title')),
          const SizedBox(height: 12),
          TextField(controller: year, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Year')),
          const SizedBox(height: 12),
          TextField(
            controller: tmdb,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(labelText: 'TMDB ID (optional)', helperText: 'From the address of the title on themoviedb.org'),
          ),
        ]),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('Save')),
      ],
    ),
  );
  if (ok == true && context.mounted) {
    await Services.read(context).library.fixMatch(
          item.id,
          tmdbId: int.tryParse(tmdb.text.trim()),
          title: title.text.trim().isEmpty ? null : title.text.trim(),
          year: int.tryParse(year.text.trim()),
        );
    if (context.mounted) {
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(const SnackBar(content: Text('Match updated')));
    }
  }
  title.dispose();
  year.dispose();
  tmdb.dispose();
}
