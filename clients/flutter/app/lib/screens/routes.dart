// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import 'movie_detail_screen.dart';
import 'show_detail_screen.dart';

/// Opens the detail page for [item] in the current (section) navigator.
void openItem(BuildContext context, LibraryItem item, {LibraryItem? show}) {
  final Widget page = switch (item.kind) {
    ItemKind.movie => MovieDetailScreen(item: item),
    ItemKind.show => ShowDetailScreen(show: item),
    ItemKind.episode when show != null => ShowDetailScreen(
      show: show,
      initialSeason: item.season,
    ),
    _ => MovieDetailScreen(item: item),
  };
  Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => page));
}
