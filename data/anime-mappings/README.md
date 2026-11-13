# Маппинги аниме (META-008)

Кросс-ID маппинг аниме (AniDB ↔ AniList ↔ MAL ↔ Kitsu ↔ TVDB ↔ TMDB) и нумерация эпизодов AniDB ↔ TVDB/TMDB. Код — крейт [`pa-anime-mappings`](../../core/crates/pa-anime-mappings) (MPL-2.0). Юридическое обоснование — [docs/legal/anime-mappings.md](../../docs/legal/anime-mappings.md).

## Что встраивается, а что нет

| Источник | Лицензия | Как попадает на устройство |
|---|---|---|
| [anime-offline-database](https://github.com/cedya77/anime-offline-database) (форк cedya77, продолжение [manami-project](https://github.com/manami-project/anime-offline-database)) | ODbL-1.0 + DbCL-1.0 | **Встраивается** в бандл как производная база `anime-offline-ids.jsonl` (под ODbL) + `LICENSE` upstream; обновление — из релизов upstream |
| [Anime-Lists](https://github.com/Anime-Lists/anime-lists) `anime-list-master.xml` | не указана | **Не встраивается и не зеркалится.** Приложение скачивает файл напрямую с upstream на устройство пользователя |
| [Fribb/anime-lists](https://github.com/Fribb/anime-lists) `anime-list-full.json` | не указана | **Не встраивается и не зеркалится.** Скачивается с upstream на устройство; необязателен |

В бандл и в этот репозиторий не попадает ни одной строки из Anime-Lists или Fribb. Слияние слоёв выполняется только в памяти на устройстве пользователя (`MappingIndex`), поэтому чужие данные без лицензии мы не распространяем и не смешиваем с ODbL-базой.

Резервный форк в организации проекта допустим **только для anime-offline-database**. Для Anime-Lists и Fribb — только после появления лицензии (решение пересматривается PR в `docs/legal/anime-mappings.md`).

## Встроенная база (ODbL)

Сборка воспроизводима:

```bash
data/anime-mappings/build.sh            # → data/anime-mappings/dist/
```

Скрипт скачивает зафиксированный релиз из [`upstream.env`](upstream.env), проверяет SHA-256, кладёт рядом `LICENSE.anime-offline-database` и собирает `anime-offline-ids.jsonl` бинарём `anime-mappings-build`. Каталог `dist/` в git не коммитится (~15 МБ); его кладут в бандл сборочные задачи (PKG/UI).

Формат — открытый JSONL без шифрования (ODbL §4.7): первая строка — заголовок (`format`, `license`, `attribution`, тег и SHA-256 исходного релиза), далее по строке на аниме: ID провайдеров, название, тип, число эпизодов, год и синонимы (`synonyms`) — только латиницей, кириллицей, каной и иероглифами: по ним локальный индекс названий (META-007) находит аниме по ромадзи, английскому и русскому названию без сети. Только данные ODbL. Производная база распространяется **под ODbL-1.0** (share-alike), а этот скрипт и код крейта — способ её получить (ODbL §4.6).

Обновление версии: поменять `AOD_TAG` и `AOD_SHA256` (поле `digest` ассета в релизе GitHub) одним коммитом.

## Загрузка на устройство (Anime-Lists, Fribb)

`pa_anime_mappings::update::Updater`:

- URL зашиты в коде и указывают на upstream (`raw.githubusercontent.com/Anime-Lists/…`, `…/Fribb/…`), никогда на наши ресурсы;
- не чаще раза в сутки на источник, `If-None-Match`/ETag, 304 — без скачивания;
- проверка целостности: разбор файла, минимальное число записей, отказ при сокращении больше чем вдвое, SHA-256 в `manifest.json`, проверка SHA-256 при каждом чтении;
- атомарная замена: новый файл пишется под именем с хэшем, затем одним `rename` подменяется `manifest.json`, старый файл удаляется; при любой ошибке остаётся прежняя версия;
- без сети и без этих файлов маппинг работает на встроенной ODbL-базе (кросс-ID между аниме-сайтами), а TVDB/TMDB-нумерация недоступна.

HTTP-клиент реализует вызывающая сторона (трейт `HttpClient`), тесты сеть не используют.

## Атрибуция

Строки для экрана «О программе» и `THIRD_PARTY_NOTICES` (LEGAL-009) — константа `pa_anime_mappings::ATTRIBUTION`. Первая строка обязательна при поставке встроенной базы, две другие показываются, если пользователь скачал соответствующий список.
