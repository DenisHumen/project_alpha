<!-- SPDX-License-Identifier: MPL-2.0 -->

# ADR-NNNN: Субтитры — sidecar-файлы из групп ядра, стиль в своём оверлее, «системный стиль» по умолчанию

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта (мейнтейнер); участники обсуждения — по PR
- **Связано:** PLAY-007, LIB-007, LIB-018, PLAY-004, PLAY-006, UI-014, PLAY-DSK-007, PLAY-APL-004; [ADR-0012](0012-release-0-1-player-media-kit.md), [ADR-0035](0035-apple-engines-foreign-player-engine-mpvkit.md), [ADR-0038](0038-network-playback-loopback-proxy.md)

## Контекст и проблема

Нужно (PLAY-007): подхватывать внешние субтитры рядом с файлом — и для локальных, и для сетевых источников; сдвигать их по времени; настраивать размер, шрифт, положение и фон; по умолчанию следовать системным настройкам субтитров (Android captioning, iOS/tvOS MediaAccessibility); запоминать дорожку и задержку на сериал.

Ограничения: плеер приложения — media_kit поверх libmpv (ADR-0012). media_kit по умолчанию выключает рендер субтитров в mpv (`sub-visibility=no`) и рисует текст `sub-text` своим виджетом. libmpv на Android не видит системные шрифты без отдельной настройки. Сетевые файлы играют через loopback-прокси (ADR-0038), а `sub-auto` mpv по HTTP соседние файлы не ищет. Анализ папок ядра (LIB-007) уже знает sidecar-субтитры каждого видео с языком и флагами forced/SDH (`group_assets`).

## Факторы решения

- Одинаково для локальных и сетевых источников, без второго обхода папки в Dart.
- Язык и флаги forced/SDH — для выбора дорожки (forced на языке пользователя).
- Системный стиль Android и Apple применим без форка media_kit.
- Тестируемость на mock-движке (тесты настроек и выбора дорожки без libmpv).
- Не мешать PLAY-006 (libass, PGS, VobSub в ядре) и PLAY-004 (политика дорожек по профилю).

## Рассмотренные варианты

Источник sidecar-файлов:

1. **Ядро:** `Catalog::sidecar_subtitles(source, path)` по группам LIB-007; Dart получает список, сетевые файлы — URL прокси.
2. `sub-auto=fuzzy` в mpv — только локальные файлы, без флагов forced/SDH.
3. Обход папки в Dart — дублирует LIB-007, для сети нужен свой листинг.

Отрисовка и стиль на media_kit:

1. **Свой Flutter-оверлей** (`SubtitleOverlay`) по `sub-text`: размер от высоты кадра, шрифт, цвет, контур/тень, фон строки и окна, отступ снизу.
2. libass в mpv (`libass: true`) и стиль через `sub-*` — на Android нужен шрифт-ассет и `sub-fonts-dir`, текст рисуется в текстуре видео.
3. Встроенный `SubtitleView` media_kit — только `TextStyle`, без контура и окна.

## Решение

Источник — вариант **1**. Плеер получает sidecar-файлы в `PlayRequest.subtitles` и тогда открывает файл с `sub-auto=no` (иначе локальные файлы загрузились бы дважды), добавляет их `sub-add … auto <имя> <язык>` после загрузки файла и выбирает дорожку сам. Если библиотека не знает субтитров (`subtitles == null`: демо-режим, v0), остаётся `sub-auto=fuzzy`.

Отрисовка на media_kit — вариант **1**: текстовые субтитры рисует `SubtitleOverlay` поверх видео, `SubtitleView` media_kit выключен. Растровые (PGS, VobSub) и стили ASS на этом пути не отображаются так же, как и до PLAY-007; их полный рендер — PLAY-006. На Apple (`AlphaPlayerKit`, mpv с libass) тот же стиль переводится в опции `sub-*` (`SubtitleStyle.mpvOptions`).

Стиль: `SubtitleSettings` (pa_player) — «системный стиль» по умолчанию; без системных настроек (десктоп) — стандартный вид (белый текст с контуром, 5,33 % высоты, 8 % от низа). Android: плагин pa_player читает `CaptioningManager` (цвета, края, масштаб, шрифт, язык) по каналу `app.kinolune/captioning`; стиль применяется, только если субтитры включены в системе, как у штатных плееров Android. Apple: `SystemCaptions.current()` читает MediaAccessibility. Положение всегда пользовательское — в системных настройках его нет.

Выбор дорожки (`pickSubtitle`): запомненный для сериала выбор (язык и вид, не id) → при включённых системных субтитрах полные субтитры на языке пользователя (SDH первыми) → аудио на языке пользователя: forced на этом языке, иначе выкл.; аудио на чужом языке: полные, иначе forced. Языки пользователя — язык системных субтитров и язык интерфейса. Это минимальная политика PLAY-007; профили и настройки «Auto/Original» — PLAY-004.

Задержка — `sub-delay` mpv, положительная = субтитры позже; в интерфейсе — «Субтитры позже на 0,3 с» / «раньше на 0,3 с». Выбор и задержка запоминаются на сериал (`seriesKey` = id шоу) в настройках приложения (`Preferences`, ключ `subtitle_series_<id>`) и переходят на следующую серию в той же сессии. Перенос в профиль ядра — вместе с PLAY-004 (LIB-012).

### Последствия

- Хорошо: сетевые sidecar-файлы работают так же, как локальные; флаги forced/SDH из имён файлов участвуют в выборе.
- Хорошо: стиль и выбор дорожки покрыты тестами без libmpv; Android-часть — один небольшой Kotlin-класс.
- Плохо: на media_kit стили ASS упрощаются до текста (как и раньше) до PLAY-006.
- Плохо: на Android 13+ медиаразрешения не дают читать `.srt` в общих папках (`/sdcard/Movies`); sidecar-файлы локальных источников там видны только в папках приложения, в SAF-доступе или на сетевых источниках. Это ограничение скана LIB-007, отдельная задача.
- Нейтрально: выбор на сериал хранится в настройках устройства и не синхронизируется до PLAY-004.

## Ссылки

- `core/crates/pa-catalog/src/subtitles.rs`, `core/crates/pa-frb/src/api/subtitles.rs`
- `clients/flutter/packages/pa_player/lib/src/subtitles/`, `.../android/src/main/kotlin/app/kinolune/player/PaPlayerPlugin.kt`
- `clients/apple/AlphaPlayerKit/Sources/AlphaPlayerKit/SubtitleStyle.swift`
- Тесты: `pa-catalog/tests/subtitles.rs`, `pa_library/test/subtitles_test.dart`, `pa_player/test/subtitle_settings_test.dart`, `subtitle_policy_test.dart`, `player_subtitles_test.dart`, `app/test/subtitles_test.dart`, `AlphaPlayerKitTests/SubtitleStyleTests.swift`
