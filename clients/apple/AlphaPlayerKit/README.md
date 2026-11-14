<!-- SPDX-License-Identifier: MPL-2.0 -->

# AlphaPlayerKit

Apple-движки воспроизведения за контрактом ядра `PlayerEngine` (PLAY-APL, [ADR-0003](../../../docs/adr/0003-native-player-surfaces-engines.md), [ADR-0017](../../../docs/adr/0017-player-engine-contract.md), [ADR-NNNN](../../../docs/adr/0035-apple-engines-foreign-player-engine-mpvkit.md)). SwiftPM-пакет для iOS 15+, tvOS 15+, macOS 12+; его используют Flutter-плагин `pa_player_apple` (PLAY-APL-004) и tvOS-приложение.

Этап 1 (PLAY-APL-001) — **`MPVEngine`** на libmpv из [MPVKit](https://github.com/mpvkit/MPVKit) (`EngineKind.mpv`, последний движок цепочки Auto на Apple). Дальше здесь появятся AVPlayer (PLAY-APL-002) и свой движок FFmpeg → VideoToolbox (PLAY-APL-005).

## Использование

```swift
import AlphaPlayerKit
import PACore

let view = MPVVideoView()                       // CAMetalLayer, vo=gpu-next через MoltenVK
let engine = try MPVEngine(configuration: .init(videoOutput: .metal(view.metalLayer)))
engine.setListener(listener: myListener)        // EngineListener: события EngineEvent
try engine.openMedia(request: OpenRequest(
    items: [MediaSource(uri: fileURL.absoluteString, title: "Episode 1")],
    index: 0, startPositionMs: 0, play: true))
// Ядру: ForeignEngine(engine) — тот же pa_playback::PlayerEngine, что и остальные движки.
```

`openMedia` — это `open` контракта (UniFFI экранирует ключевое слово Swift `open` прямо в C-заголовке foreign trait, и заголовок не компилируется), `closeEngine` — `close` (в Kotlin `close()` занят `AutoCloseable`).

Как `MPVEngine` ложится на контракт:

- Состояние и команды — на одной последовательной очереди; команды только запускают работу в mpv, результат приходит событиями. Каждое событие проходит через `engineStateApply` (редьюсер ядра), поэтому `state()` — всегда свёртка событий. Слушатель вызывается на очереди движка и может вызывать движок (вызовы с очереди выполняются сразу).
- В mpv всегда один файл, `keep-open=yes`: плейлист, `RepeatMode`, переход к следующему элементу и `Ended` делает движок. `playing` следует командам, а не свойству `pause` mpv.
- Внешние субтитры и аудио выбирает ядро (LIB-007): `sub-auto=no`, `audio-file-auto=no`. Громкость до 300 % (`volume-max=300`).
- `Position` — каждые 250 мс при воспроизведении, после seek, паузы и смены скорости.
- `settle(timeout:)` ждёт окончания загрузки и seek и шлёт свежий `Position` — для тестов и UI-тестов.
- `MPVBuildInfo.report()` — версии mpv и FFmpeg и `mpv-configuration` для экрана лицензий.

Субтитры (PLAY-007, [ADR-NNNN](../../../docs/adr/NNNN-subtitle-style-and-sidecars.md)): `addSidecarSubtitle` добавляет sidecar-файл ядра без выбора; `setSubtitleStyle(_:)` переводит `SubtitleStyle` в опции `sub-*` mpv (размер — доля высоты кадра, как в pa_player); «системный стиль» — `SystemCaptions.current().style()` из MediaAccessibility (iOS, tvOS, macOS; перечитывать по `kMACaptionAppearanceSettingsChangedNotification`); `setSubtitleDelay(milliseconds:)` — положительная задержка показывает субтитры позже.

Не входит в этап 1: `AVAudioSession`, PiP, Now Playing (PLAY-APL-004), `ByteSource` → AVIO (PLAY-APL-003, сейчас — URI и loopback `pa-proxy`), HDR-вывод и AFR (PLAY-APL-006, -009).

## Лицензия MPVKit

Только продукт `MPVKit` (LGPL), не `MPVKit-GPL`; версия закреплена `exact:` в `Package.swift`. Сборка `1.1.0-n9.0.2`: mpv 0.41 с `-Dgpl=false`, FFmpeg 8.0 с `--enable-version3` (LGPLv3). `scripts/check-license.sh`:

- `manifest` — зависимость только от `MPVKit` с `exact:`, нигде в коде нет `-GPL` (в CI ещё и канарейка: манифест с `MPVKit-GPL` обязан не пройти);
- `binary <dir>` — во всех Mach-O собранных продуктов есть `-Dgpl=false` и нет `-Dgpl=true`, `--enable-gpl`, `--enable-nonfree`, «GPL version» (кроме LGPL).

**Сборка с MPVKit — только для разработки и CI, не для релиза.** Для App Store политика проекта — только LGPL-2.1 (CLAUDE.md, ADR-0005), а FFmpeg в MPVKit — LGPL-3.0. Профиль `--store` (`check-license.sh manifest --store`, `binary --store`, или `PA_LICENSE_PROFILE=store scripts/test.sh …`) запрещает MPVKit в манифесте и `--enable-version3` / «LGPL version 3» в бинарниках и сейчас падает намеренно; CI проверяет, что он падает, и помечает сборку как нерелизную. Релизный конвейер Apple обязан использовать `--store`. Переход на свою LGPL-2.1-сборку FFmpeg + libmpv — PLAY-APL-013.

## Тесты

Тестовые медиа генерируются, в git их нет:

```bash
tools/bindings/build-xcframework.sh                      # PACoreFFI.xcframework (macOS + Xcode)
clients/apple/AlphaPlayerKit/scripts/make-fixtures.sh /tmp/pa-fixtures      # контрактные MKV (ffmpeg)
clients/apple/AlphaPlayerKit/scripts/make-qa-media.py /tmp/pa-qa-media      # MKV из QA-002
clients/apple/AlphaPlayerKit/scripts/test.sh ios  /tmp/pa-fixtures /tmp/pa-qa-media
clients/apple/AlphaPlayerKit/scripts/test.sh tvos /tmp/pa-fixtures /tmp/pa-qa-media
```

- `MPVEngineContractTests` — 16 случаев `pa_playback::contract::CASES` на `MPVEngine`: Rust-раннер `playerEngineContractRun` вызывает Swift-харнесс (`advance` — реальное время плюс `settle`). Звук `ao=null`, видео `vo=null`, программное декодирование — детерминированно на симуляторах CI.
- `MockEngineRoundTripTests` — mock ядра через те же FFI-трейты: привязки и раннер работают на платформе.
- `MPVMediaTests` — MKV из QA-002 (H.264 Hi10P, HEVC Main10, HDR10, AC-3, E-AC-3, TrueHD, DTS, FLAC 7.1, PGS, VobSub, мультидорожечный `t-multi`) открываются и играют; libmpv собран с `-Dgpl=false`.

Без переменных `PA_PLAYER_FIXTURES` и `PA_QA_MEDIA` медиатесты пропускаются. В CI — job `player` в `.github/workflows/apple-core.yml` (macos-15).

Без Mac: `scripts/linux-dev-test.sh docker` собирает те же исходники и тесты с libmpv дистрибутива в Docker (`swift:6.1-noble`). Это быстрая проверка логики движка, не замена Apple CI: другая версия mpv, нет MPVKit и проверки LGPL.
