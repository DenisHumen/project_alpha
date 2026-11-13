<!-- SPDX-License-Identifier: MPL-2.0 -->
# pa_library

Медиатека Kinolune 0.1 на чистом Dart: источники, сканер, парсер имён v0, TMDB, SQLite, прогресс просмотра. API — по контракту [docs/dev/release-0.1-contract.md](../../../../docs/dev/release-0.1-contract.md).

```dart
final lib = await Library.open('${appSupportDir}/library.sqlite');
await lib.addSource(MediaSource(id: '', kind: SourceKind.localFolder, name: 'Фильмы', path: '/home/me/Videos'));
await lib.setTmdbApiKey(userOrProjectKey);         // необязательно
await for (final p in lib.scan()) { print(p); }   // ScanPhase: connecting → scanning → metadata → done
lib.movies().listen(...);                          // потоки пересылают данные при каждом изменении
```

Сверх контракта: `tmdbApiKey()`, `setMetadataLanguage('ru-RU')`, `recentlyAdded()`, `item()`, `progress()`, `search()`, `close()`, а также `LibraryItem.tmdbId/trackNumber/addedAt`.

## Что внутри

| Модуль | Что делает |
|---|---|
| `parser/filename_parser.dart` | Парсер имён v0: фильмы с годом, `S01E01`, `1x01`, `S01E01E02`, «Season 1 Episode 2», «1 сезон 5 серия», аниме `[Group] Title - 01 [1080p]`, кириллица, двойные названия `Рус / Orig`, отбрасывание качества/кодеков/групп. Контекст папок: `Show/Season N/…`, `Сезон N`, `Specials` → 0, `Название (Год)/файл`. Заменяется Rust-парсером META-005 в 0.2. |
| `scanner.dart` | Рекурсивный обход (симлинки с защитой от циклов), видео/аудио по расширению; пропускает sample/trailer/extras, `BDMV`/`VIDEO_TS`, служебные папки NAS, скрытые файлы. |
| `tmdb.dart` | Клиент TMDB v3 (ключ v3 или токен v4): поиск фильмов/сериалов, детали, эпизоды, картинки `image.tmdb.org`, ретраи на 429. |
| `music_tags.dart` | Теги ID3/Vorbis/MP4 через `audio_metadata_reader` (MIT), иначе `Исполнитель/Альбом/NN - Название`; обложки `cover.jpg`/`folder.jpg`, постеры `<имя>-poster.jpg`. |
| `db.dart`, `library.dart` | SQLite через пакет `sqlite3` (MIT; библиотека SQLite собирается/подтягивается build hook-ом пакета для Linux и Android). |

## TMDB: условия

- Без ключа всё работает по распознанным названиям (без постеров и описаний).
- Атрибуция: приложение показывает `tmdbAttribution` и логотип TMDB в «О программе».
- Кэш ответов — 30 дней; данные TMDB в элементах старше 180 дней (≤ 6 месяцев по условиям) удаляются при открытии БД и запрашиваются заново при сканировании.
- В тестах сети нет: ответы — фикстуры `test/fixtures/tmdb/*.json` (написаны вручную в формате TMDB).

## SMB (бета)

- **Linux:** источник `smb://host/share/path` открывается через GVfs (`/run/user/<uid>/gvfs/smb-share:…`). Если шара не смонтирована, библиотека вызывает `gio mount` и передаёт логин/пароль через stdin. Надёжный путь — смонтировать шару в файловом менеджере (или `mount.cifs`) и добавить смонтированную папку как источник «папка».
- **Android:** SMB-клиента с совместимой лицензией в 0.1 нет (известные чисто-Dart клиенты — порты LGPL/GPL-кода). Сканирование SMB-источника завершается понятной ошибкой `SmbUnsupportedException`; в приложении источник SMB на Android следует скрыть или пометить как «скоро».

## Ограничения 0.1

- Запросы к SQLite синхронные и выполняются в том же isolate (пакетные транзакции по 200 файлов); для очень больших медиатек позже вынести в фоновый isolate.
- Нет «следующей серии» в «Продолжить просмотр», нет сезонных постеров, диски BDMV/ISO не поддерживаются.
- Группировка сериалов — по нормализованному названию (`showIdFor`), без учёта года.

## Проверка

```sh
dart pub get && dart analyze && dart test
```
