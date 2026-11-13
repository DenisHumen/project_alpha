<!-- SPDX-License-Identifier: MPL-2.0 -->

# pa-playback

Контракт плеера ядра (PLAY-003, [ADR-0003](../../../docs/adr/0003-native-player-surfaces-engines.md), [ADR-0017](../../../docs/adr/0017-player-engine-contract.md)): трейт `PlayerEngine`, capability-матрица движков, выбор движка «Auto» с ручным переопределением, mock-движок и общий набор контрактных тестов.

Зависимости — только `serde` и `thiserror`: крейт не тянет рантайм и FFI. Типы следуют правилам `pa-model` (записи и перечисления, миллисекунды `u64`, serde), поэтому `pa-ffi` (UniFFI) и `pa-frb` экспортируют их без обёрток, когда это понадобится платформенной задаче (PLAY-AND-002, PLAY-APL-001, PLAY-DSK-002).

## Контракт `PlayerEngine`

| Команда | Что делает |
|---|---|
| `open(OpenRequest { items, index, start_position_ms, play })` | Заменяет плейлист и открывает `items[index]` с позиции (обрезается до длительности) |
| `play()`, `pause()` | После конца плейлиста `play()` начинает элемент сначала |
| `seek(position_ms)` | В пределах `0..=duration` |
| `set_rate(rate)` | `0.5..=2.0` обязательны; `≤ 0`, NaN, ∞ — `InvalidArgument`; можно до `open` |
| `set_volume(percent)` | `0..=300`, 100 — без изменений; движок без усиления обрезает до 100; можно до `open` |
| `set_repeat(Off / All / One)`, `jump(index)` | Плейлист |
| `set_audio_track(Option<id>)`, `set_subtitle_track(Option<id>)` | `None` — выключить |
| `add_external_subtitle(uri, title)` | Загружает и сразу выбирает |
| `stats()` | Техническая статистика (PLAY-010 расширит модель) |
| `close()` | Идемпотентно; дальше любая команда — `Closed` |
| `kind()`, `state()`, `set_listener()` | Тип движка, снимок состояния, подписка на события |

Правила (их проверяет `contract`):

1. **Команды не блокируют.** Синхронная ошибка значит «команда отклонена, ничего не изменилось»; результат приходит событиями `EngineEvent`.
2. **Состояние — свёртка событий:** `state() == EngineState::from_events(все события)`. UI, который слушает только события, видит ровно то же, что вернёт `state()`.
3. Колбэки приходят без удерживаемых блокировок движка и могут вызывать движок.
4. `Loading` сбрасывает всё, кроме скорости, громкости и повтора; конец элемента посреди плейлиста — переход к следующему без `Ended`; `Ended` — только в конце плейлиста при `RepeatMode::Off`.

События: `Loading`, `Loaded`, `TracksChanged`, `TrackSelected`, `Position` (не реже раза в 500 мс при воспроизведении), `PlayingChanged`, `BufferingChanged`, `RateChanged`, `VolumeChanged`, `RepeatChanged`, `Ended`, `Error`, `Closed`. В JSON — с тегом `event` (`{"event":"position","position_ms":1500,…}`).

Видеоповерхность в контракт не входит: это платформенный UI (ADR-0008, ADR-0009).

## Соответствие Dart `PlayerEngine` из `pa_player` (релиз 0.1)

Контракт повторяет интерфейс `clients/flutter/packages/pa_player/lib/src/engine/player_engine.dart`, поэтому `pa_player` переходит на него без изменения экранов:

| Dart (`pa_player`) | Ядро (`pa-playback`) |
|---|---|
| `ValueListenable<EngineState> state` | `state()` + `set_listener()`; `ValueNotifier` обновляется по `EngineEvent` |
| `open(uris, index:, start:, play:)` | `open(OpenRequest { items, index, start_position_ms, play })` |
| `play`, `pause`, `seek(Duration)`, `setRate` | `play`, `pause`, `seek(ms)`, `set_rate` |
| `setVolume(0..100)` | `set_volume(0..=300)` — диапазон Dart входит целиком |
| `jump`, `setRepeat(QueueRepeat)` | `jump`, `set_repeat(RepeatMode)` — те же значения `off/all/one` |
| `setAudioTrack(MediaTrack)`, `setSubtitleTrack(MediaTrack.off)` | `set_audio_track(Some(id))`, `set_subtitle_track(None)`: `MediaTrack.off` (id `'no'`) ↔ `None` |
| `addExternalSubtitle(uri, title:)` | `add_external_subtitle(uri, title)` |
| `MediaTrack {id, title, language, codec, external}` | `Track` — те же поля плюс `channels`, `default`, `forced` |
| `EngineState` (Duration-поля) | `EngineState` (`*_ms`), плюс `loaded`, `repeat`, `closed`; `error` — `EngineError` вместо строки |
| `EngineState.info` | `stats()` → `PlaybackStats` |
| `buildVideo()` | нет: поверхность — платформенный UI |
| `dispose()` | `close()` |

## Capability-матрица и Auto

`default_capabilities(engine, platform)` — целевая матрица из ADR-0003 и research/02 §2.2–2.6: контейнеры, видео- и аудиокодеки, форматы субтитров, динамический диапазон (`Native` — сигнал уходит на дисплей как есть; `ToneMapped` — тон-маппинг libplacebo). Платформа уточняет её в рантайме (MediaCodecList, режимы HDR дисплея, `AVPlayer.eligibleForHDRPlayback`) и передаёт в `select_engine`.

| Платформа | Цепочка Auto (`auto_chain`) |
|---|---|
| Android, Android TV | Media3 → libmpv |
| iOS, iPadOS, macOS, tvOS | AVFoundation → свой движок (FFmpeg → VideoToolbox) → MPVKit (`EngineKind::Mpv`) |
| Windows, Linux | libmpv |

Auto берёт первый движок цепочки без ограничений. Если таких нет — движок с самым мягким худшим ограничением (`Limitation::severity`: неподдерживаемые субтитры 1, часть аудиодорожек 2, DV показан базовым слоем 3, тон-маппинг 4); при равенстве — более ранний в цепочке. Блокирующие ограничения (контейнер, видеокодец, нет ни одной играемой аудиодорожки, неверные цвета HDR/DV) исключают движок. `EnginePreference::Manual(engine)` выбирает движок, если он есть на платформе, даже когда матрица против (матрица может быть консервативной). `Selection.evaluations` объясняет выбор для stats overlay и логов.

## Тесты

```bash
cargo test -p pa-playback
```

- `tests/contract_mock.rs` — контрактный набор (`contract::CASES`, 16 случаев) на `MockEngine`; проверка, что набор ловит сломанный движок.
- `tests/select.rs` — регрессионная матрица выбора движка.

Новый движок подключает набор так:

```rust
pa_playback::player_engine_contract_tests!(|| Box::new(MyHarness::new()));
```

Движки на Swift и Kotlin гоняют тот же набор через `pa-ffi`: `run_player_engine_contract_case(name, harness)` с харнессом на платформе (пример — `clients/apple/AlphaPlayerKit/Tests`, PLAY-APL-001).

`Harness` отдаёт движок, `Fixture` (два видео не короче 10 с, минимум две аудиодорожки и одни субтитры, битый источник, внешний SRT) и `advance(ms)` — у mock это виртуальные часы, у реального движка — ожидание событий.
