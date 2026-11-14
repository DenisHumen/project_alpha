// SPDX-License-Identifier: MPL-2.0

import 'package:pa_library/pa_library.dart';
import 'package:test/test.dart';

LibraryItem _item(String id, ItemKind kind) =>
    LibraryItem(id: id, kind: kind, title: id, filePath: '/media/$id.mkv');

void main() {
  test('groups keep the rank order, the group order and the cap', () {
    final items = [
      _item('t1', ItemKind.track),
      _item('e1', ItemKind.episode),
      _item('m1', ItemKind.movie),
      _item('s1', ItemKind.show),
      _item('m2', ItemKind.movie),
      _item('m3', ItemKind.movie),
      _item('e2', ItemKind.episode),
    ];
    final groups = groupSearchResults(items, perGroup: 2);
    expect(groups.map((g) => g.kind), searchGroupOrder);
    expect(groups[0].items.map((i) => i.id), ['m1', 'm2']);
    expect(groups[1].items.map((i) => i.id), ['s1']);
    expect(groups[2].items.map((i) => i.id), ['e1', 'e2']);
    expect(groups[3].items.map((i) => i.id), ['t1']);
  });

  test('empty groups are left out', () {
    final groups = groupSearchResults([_item('t1', ItemKind.track)]);
    expect(groups.single.kind, ItemKind.track);
    expect(groupSearchResults(const []), isEmpty);
  });
}
