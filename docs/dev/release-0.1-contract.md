# Релиз 0.1 — контракт между частями приложения

Цель (ADR-0011): наглядный первый релиз **Kinolune 0.1** для **Linux-десктопа и Android** (телефон + Android TV с пультом), собранный в CI и опубликованный в GitHub Releases.

Решение по плееру для 0.1 (ADR-0012): **media_kit** (libmpv, LGPL) на обеих платформах — быстрый общий плеер. Нативные поверхности с HDR/DV/AFR (ADR-0003, 0008, 0009) приходят позже, за тем же API `pa_player`.

## Состав и владение

| Путь | Что | Владелец (задача) |
|---|---|---|
| `clients/flutter/app/` | Flutter-приложение `kinolune` (applicationId `app.kinolune`), экраны, навигация, тема | R01-APP |
| `clients/flutter/packages/pa_library/` | Чистый Dart: источники, сканер, парсер имён v0, TMDB, БД (SQLite через `sqflite_common_ffi`/`sqflite` или drift), прогресс | R01-LIB |
| `clients/flutter/packages/pa_player/` | Flutter-пакет: экран видеоплеера, музыкальный плеер, на media_kit | R01-PLAYER |
| `.github/workflows/release-app.yml`, `packaging/app/` | Сборка Linux (tar.gz + AppImage) и Android (APK), публикация по тегу `app-v*` | R01-REL |

Все пакеты подключаются в приложение через `path:`-зависимости. Каждый пакет самодостаточен и тестируется отдельно (`flutter test` / `dart test`).

## API `pa_library` (Dart)

```dart
// Источники
enum SourceKind { localFolder, smb }
class MediaSource { String id; SourceKind kind; String name; String path; // localFolder: абсолютный путь; smb: smb://host/share/path
  String? username; /* пароль хранится через flutter_secure_storage на стороне app, сюда передаётся при scan */ }

// Медиатека
enum ItemKind { movie, show, episode, track }
class LibraryItem { String id; ItemKind kind; String title; String? originalTitle; int? year;
  String? overview; String? posterUrl; String? backdropUrl; String? showId; int? season; int? episode;
  String? artist; String? album; String filePath; int? durationMs; double? matchConfidence; }
class Progress { String itemId; int positionMs; int durationMs; DateTime updatedAt; bool watched; }

abstract class Library {
  static Future<Library> open(String dbPath);
  Future<List<MediaSource>> sources();
  Future<MediaSource> addSource(MediaSource s);
  Future<void> removeSource(String id);
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword}); // находит файлы, парсит имена, тянет метаданные
  Future<void> setTmdbApiKey(String? key);                             // без ключа — работает по распознанным названиям
  Stream<List<LibraryItem>> movies();
  Stream<List<LibraryItem>> shows();
  Stream<List<LibraryItem>> episodes(String showId);
  Stream<List<LibraryItem>> tracks();
  Stream<List<(LibraryItem, Progress)>> continueWatching();            // 2%..95%, по updatedAt
  Future<void> saveProgress(Progress p);
  Future<void> fixMatch(String itemId, {int? tmdbId, String? title, int? year}); // ручное исправление
}
```

Парсер имён v0 — на Dart внутри `pa_library` (S01E01, 1x01, аниме `[Group] Title - 01`, год, качество/кодеки отбрасываются). В 0.2 заменяется Rust-парсером META-005 через FFI.

SMB: на Linux — через `smb://` с помощью `gio`/`gvfs` mount или системного `mount.cifs`-пути, выбранного пользователем как локальная папка; на Android — Dart/Java SMB2-клиент с совместимой лицензией (MIT/Apache/LGPL-динамически). Если в 0.1 не успевает — источник «папка» + инструкция, SMB помечен как бета.

## API `pa_player`

```dart
class PlayRequest { String uri; String title; int startPositionMs; String? subtitleUri; }
class PlayerScreen extends StatefulWidget { // полноэкранный плеер, управление мышью/клавиатурой/пультом D-pad
  const PlayerScreen({required PlayRequest request, required void Function(int positionMs, int durationMs) onProgress, VoidCallback? onFinished});
}
class MusicQueue { /* очередь треков, gapless, shuffle/repeat */ }
class MusicPlayerController extends ChangeNotifier { Future<void> play(List<PlayRequest> queue, {int index = 0}); void pause(); void resume(); void next(); void previous(); void seek(Duration d); }
class NowPlayingBar extends StatelessWidget { /* мини-плеер внизу, в стиле Spotify */ }
class NowPlayingScreen extends StatelessWidget { /* полноэкранный «сейчас играет» */ }
```

`onProgress` вызывается каждые 5 с, на паузе, перемотке и выходе.

## Приложение (экраны 0.1)

Главная (ряды: Продолжить просмотр, Недавно добавленные фильмы, Сериалы) · Фильмы (сетка постеров, сортировка/фильтры) · Сериалы → сезоны → эпизоды · Музыка (альбомы/треки + мини-плеер) · Поиск · Настройки (источники, TMDB-ключ, о программе/лицензии/атрибуция TMDB). Тёмная тема в духе tvOS (токены `design/tokens`). Навигация: десктоп — боковая панель; телефон — нижние вкладки; Android TV — верхнее меню и фокус с увеличением карточки (D-pad).

Twitch, профили, синхронизация между устройствами — после 0.1.
