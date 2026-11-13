// SPDX-License-Identifier: MPL-2.0
import 'dart:math';

import '../engine/player_engine.dart';
import '../play_request.dart';

/// Music queue: the tracks, the play order (shuffled or not), the current
/// position and the repeat mode. Pure logic, no I/O.
class MusicQueue {
  MusicQueue({Random? random}) : _random = random ?? Random();

  final Random _random;
  List<PlayRequest> _items = const [];
  List<int> _order = const [];
  int _position = 0;
  bool _shuffle = false;
  QueueRepeat _repeat = QueueRepeat.off;

  /// Tracks in their original order.
  List<PlayRequest> get items => List.unmodifiable(_items);

  /// Tracks in play order.
  List<PlayRequest> get ordered =>
      List.unmodifiable([for (final i in _order) _items[i]]);

  /// Index into [ordered] of the current track.
  int get position => _position;
  bool get shuffle => _shuffle;
  QueueRepeat get repeat => _repeat;
  bool get isEmpty => _items.isEmpty;
  int get length => _items.length;

  PlayRequest? get current => _items.isEmpty ? null : _items[_order[_position]];

  /// Tracks after the current one, in play order.
  List<PlayRequest> get upNext => [
    for (var i = _position + 1; i < _order.length; i++) _items[_order[i]],
  ];

  /// Already played tracks of this queue, in play order.
  List<PlayRequest> get history => [
    for (var i = 0; i < _position; i++) _items[_order[i]],
  ];

  /// Replaces the queue and starts at [index] (index into [items]).
  void replace(List<PlayRequest> items, {int index = 0}) {
    _items = List.of(items);
    if (_items.isEmpty) {
      _order = const [];
      _position = 0;
      return;
    }
    index = index.clamp(0, _items.length - 1);
    if (_shuffle) {
      _order = _shuffled(first: index);
      _position = 0;
    } else {
      _order = List.generate(_items.length, (i) => i);
      _position = index;
    }
  }

  List<int> _shuffled({required int first}) {
    final rest = [
      for (var i = 0; i < _items.length; i++)
        if (i != first) i,
    ]..shuffle(_random);
    return [first, ...rest];
  }

  /// Turns shuffle on/off, keeping the current track playing.
  void setShuffle(bool on) {
    if (on == _shuffle) return;
    _shuffle = on;
    if (_items.isEmpty) return;
    final cur = _order[_position];
    if (on) {
      _order = _shuffled(first: cur);
      _position = 0;
    } else {
      _order = List.generate(_items.length, (i) => i);
      _position = cur;
    }
  }

  void setRepeat(QueueRepeat mode) => _repeat = mode;

  /// off → all → one → off.
  QueueRepeat cycleRepeat() {
    _repeat =
        QueueRepeat.values[(_repeat.index + 1) % QueueRepeat.values.length];
    return _repeat;
  }

  /// Sets the current position (index into [ordered]), e.g. from the engine.
  void moveTo(int position) {
    if (_items.isEmpty) return;
    _position = position.clamp(0, _order.length - 1);
  }

  /// Position of the next track for a user "next" press, or `null` at the
  /// end without repeat. (Repeat-one does not trap the user on "next".)
  int? nextPosition() {
    if (_items.isEmpty) return null;
    if (_position + 1 < _order.length) return _position + 1;
    return _repeat == QueueRepeat.off ? null : 0;
  }

  /// Position of the previous track, or `null` at the start without repeat.
  int? previousPosition() {
    if (_items.isEmpty) return null;
    if (_position > 0) return _position - 1;
    return _repeat == QueueRepeat.off ? null : _order.length - 1;
  }
}
