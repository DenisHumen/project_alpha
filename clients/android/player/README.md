<!-- SPDX-License-Identifier: MPL-2.0 -->

# pa-player-android — движок Media3 для контракта `PlayerEngine`

PLAY-AND-002. Android-библиотека (Kotlin, AndroidX Media3 1.11) с `Media3Engine` — реализацией контракта `PlayerEngine` ядра (ADR-0017) через UniFFI-трейт из `pa-ffi` (`core/crates/pa-ffi/src/playback.rs`). Встраивание в UI — вариант B из ADR-0008: Flutter-плагин [`pa_player_android`](../../flutter/packages/pa_player_android/README.md) с отдельной `PlayerActivity`.

## Как устроено

- **Команды** (`open`, `play`, `pause`, `seek`, `setRate`, `setVolume`, `setRepeat`, `jump`, `setAudioTrack`, `setSubtitleTrack`, `addExternalSubtitle`, `stats`, `closeEngine`) можно звать из любого потока. Аргументы и состояние проверяются сразу (отказ — `EngineException`, ничего не меняется), работа идёт на главном потоке, которому принадлежит `ExoPlayer`.
- **Состояние — свёртка событий** редьюсером ядра `engineStateApply`: `state()` всегда равен свёртке отправленных `EngineEvent`. События уходят с главного потока, кроме `Closed` — его `closeEngine()` отправляет до возврата.
- `close` в UniFFI-трейте называется `closeEngine`: сгенерированные Kotlin-объекты — `AutoCloseable`, их `close()` освобождает handle.
- **Позиция** — тик раз в 200 мс во время воспроизведения, плюс после seek, паузы и переходов.
- **Дорожки**: только поддерживаемые устройством; id — номер дорожки контейнера (MKV), префикс источника Media3 (`0:`, `1:`) отбрасывается, поэтому id не меняются после добавления внешних субтитров. Внешние субтитры — `ext-N`; Media3 не умеет добавлять дорожку в подготовленный элемент, поэтому элемент перезагружается с той же позиции (без `Loading`). Языки Media3 нормализует в BCP 47 (`rus` → `ru`).
- **Конец плейлиста**: `Ended`, плеер ставится на паузу; `play()` начинает элемент сначала.
- **Ошибки** Media3 → `EngineError`: IO (`2xxx`) → `Source`, разбор/декодер/аудио (`3xxx–5xxx`) → `Decoder`, неподдерживаемый формат и DRM → `Unsupported`.
- **Громкость** выше 100 % (boost, PLAY-004) пока не поддерживается: значение ограничивается 100 и об этом сообщается событием.
- **Видео**: `setVideoSurfaceView(SurfaceView)` (своя поверхность SurfaceFlinger — HDR и AFR не ломаются) или `exoPlayer` для Media3 `PlayerView`.
- Пока не входит: AFR и HDR-режимы (PLAY-AND-005), FFmpeg-аудио и passthrough (PLAY-AND-004), `ByteSource` как `DataSource` (PLAY-AND-003), MediaSession/PiP (PLAY-AND-008), туннелирование.

## Сборка и тесты

Ядро берётся из сборки CORE-004 (как в `pa-core-android`): сгенерированные Kotlin-привязки и `libpa_ffi.so` из `paGenDir` (по умолчанию `target/android`).

```bash
tools/bindings/build-aar.sh --debug x86_64   # ядро для эмулятора (или `just core aar` — все ABI)
just android test                            # JUnit: маппинг Media3 → контракт
ANDROID_SERIAL=emulator-5554 just android test-device   # instrumented-тесты на эмуляторе
```

`test-device` сначала генерирует тестовые медиа (`tools/make-fixtures.sh`: H.264 и HEVC MKV 12 с, 320x240, две AAC-дорожки `eng`/`rus`, SRT-субтитры, внешний SRT). Нужен ffmpeg с libx264 и libx265 (`FFMPEG` переопределяет команду); файлы не коммитятся, энкодеры используются только на машине сборки.

Instrumented-тесты:

- `Media3ContractTest` — весь контрактный набор ядра (`pa_playback::contract`, 16 случаев) через `playerEngineContractRun`: Rust гоняет те же случаи, что проходит mock, против `Media3Engine` с видео на настоящем `SurfaceView`.
- `Media3PlaybackTest` — «done when» PLAY-AND-002: H.264 и HEVC MKV воспроизводятся (кадры декодируются и выводятся), seek, переключение аудио и субтитров.

Проверено на эмуляторе Android 14 (x86_64, образ `default`, программные декодеры `c2.android.*`/`c2.goldfish.*`); на реальных устройствах — нет.
