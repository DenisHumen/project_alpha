// SPDX-License-Identifier: MPL-2.0

import 'models.dart';

/// Group order of the Search screen (UI-011): movies, shows, episodes,
/// music. People, collections and streamers join when the core can search
/// them.
const searchGroupOrder = <ItemKind>[
  ItemKind.movie,
  ItemKind.show,
  ItemKind.episode,
  ItemKind.track,
];

/// Default number of results per group of `Library.searchGrouped`.
const searchPerGroupDefault = 20;

/// Search results of one kind, best first.
class SearchGroup {
  const SearchGroup(this.kind, this.items);

  final ItemKind kind;
  final List<LibraryItem> items;

  @override
  String toString() => 'SearchGroup($kind, ${items.length})';
}

/// Groups ranked [items] by kind in [searchGroupOrder], keeping their order
/// inside a group and at most [perGroup] per group; empty groups are left
/// out. For libraries without a grouped core search (v0, demo).
List<SearchGroup> groupSearchResults(
  Iterable<LibraryItem> items, {
  int perGroup = searchPerGroupDefault,
}) {
  final byKind = <ItemKind, List<LibraryItem>>{};
  for (final i in items) {
    final list = byKind.putIfAbsent(i.kind, () => []);
    if (list.length < perGroup) list.add(i);
  }
  return [
    for (final k in searchGroupOrder)
      if (byKind[k]?.isNotEmpty ?? false) SearchGroup(k, byKind[k]!),
  ];
}
