<!-- SPDX-License-Identifier: MPL-2.0 -->

# ADR-NNNN: Адаптеры ByteSource для FFmpeg и mpv не линкуют библиотеки, а получают функции указателями

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта (мейнтейнер); участники обсуждения — по PR
- **Связано:** NET-008, PLAY-001, PLAY-002, PLAY-DSK-002, PLAY-APL-003, PLAY-APL-005; [ADR-0003](0003-native-player-surfaces-engines.md), [ADR-0013](0013-pa-vfs-blocking-positional-reads.md); `docs/research/02` §3.1, §4

## Контекст и проблема

Плееры должны читать медиа прямо из `pa-vfs` (SMB, WebDAV, NFS) без временных файлов: FFmpeg — через свой `AVIOContext`, libmpv — через протокол `stream_cb` (`pa://<id>`). Колбэки — синхронный C ABI, и ADR-0013 уже дал под них `ByteSource::read_at`. Открытый вопрос — откуда адаптер берёт функции FFmpeg и libmpv (`avio_alloc_context`, `mpv_stream_cb_add_ro`). FFmpeg и libmpv у нас — только LGPL-сборки и только динамически (§0.3 п.10). При этом libmpv в релизе 0.1 загружает media_kit, а на Android и Apple библиотеки упаковываются платформенными задачами.

## Факторы решения

- LGPL: библиотеки должны оставаться отдельными заменяемыми `.so`, `.dll` и `.dylib`.
- Один экземпляр библиотеки в процессе: тот, что уже загрузил плеер (media_kit, нативный хост, AlphaPlayerKit).
- `cargo test --workspace` и CI ядра работают без FFmpeg и libmpv на машине.
- Без новых зависимостей: `ffmpeg-sys`/`rsmpeg` и `libmpv-sys` требуют bindgen и библиотек на этапе сборки.

## Рассмотренные варианты

1. Линковать FFmpeg и libmpv из крейта через `*-sys`-крейты.
2. Загружать библиотеки в крейте сами (`dlopen`, `libloading`).
3. Крейт даёт только колбэки, а нужные функции библиотек принимает указателями от вызывающего кода.

## Решение

Выбран вариант **3**: адаптер не знает, откуда взялась библиотека. Тот, кто уже загрузил FFmpeg или libmpv (Dart FFI в media_kit, нативный хост, Swift), передаёт указатели на функции, и мы работаем с тем же экземпляром.

Конкретика — крейт `pa-player-io` (зависит только от `pa-vfs`):

- `ffmpeg`: колбэки `read_packet`, `seek` (`SEEK_SET`, `SEEK_CUR`, `SEEK_END`, `AVSEEK_SIZE`) и `interrupt` для `AVIOInterruptCB`. `AvioContext` владеет `AVIOContext`, его буфером и курсором и освобождает их в `Drop`. Функции `av_malloc`, `av_free`, `avio_alloc_context` и `avio_context_free` передаются в `AvioApi`.
- `mpv`: `MpvProtocol` (схема + `SourceResolver`) регистрируется через переданный `mpv_stream_cb_add_ro`. Каждый открытый поток держит свой `Arc<dyn ByteSource>`, поэтому удаление id из реестра не ломает воспроизведение. `cancel_fn` и закрытие поддерживаются.
- Ошибка источника отдаётся плееру как `AVERROR(EIO)` или `-1`, а исходный `VfsError` с `ErrorKind` сохраняется для UI (`take_error`). Паника источника перехватывается и не уходит в C.
- Read-ahead и prefetch — внутри `ByteSource` сетевого бэкенда (ADR-0013). Адаптер не буферизует сверх буфера `AVIOContext`.

Не входит: Media3 `DataSource` (PLAY-AND), экспорт адаптеров через FFI для Dart и Swift (делают задачи интеграции плеера: PLAY-DSK-002, PLAY-APL-003).

### Последствия

- Хорошо: никакой линковки FFmpeg и libmpv из Rust, LGPL-библиотеки остаются заменяемыми. Тесты ядра не требуют библиотек. Работает с любой сборкой с тем же ABI.
- Плохо (цена решения): сигнатуры функций и смещения `AVIOContext.buffer` (второе поле) повторены вручную. Защищено тем, что это стабильный публичный ABI, и интеграционными тестами против нашей сборки.
- Нейтрально: интеграционные тесты (`tests/ffmpeg_avio.rs`, `tests/mpv_stream.rs`) загружают библиотеки через `dlopen` из `PA_PLAYER_IO_LIB_DIR` и без неё пропускаются. Запуск в CI появится, когда CI-008 начнёт раздавать готовые FFmpeg и libmpv другим workflow.

### Как проверяем соблюдение

`cargo tree -p pa-player-io` — только `pa-vfs`. Интеграционные тесты на Linux с `tools/mpv/out/linux-x86_64/lib`.
