
# ADR-NNNN: Apple-движки — Swift-реализации `PlayerEngine` через UniFFI foreign trait; MPVKit только LGPL с точной версией

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта
- **Связано:** `PLAY-APL-001`; дальше `PLAY-APL-002…011`, `PLAY-AND-002`; ADR-0001, ADR-0003, ADR-0017; `docs/research/02` §2.2, §3.1, §7

## Контекст и проблема

ADR-0017 ввёл контракт `PlayerEngine` в ядре (`pa-playback`) и оставил экспорт через `pa-ffi` первой платформенной задаче. Apple-движки (MPVKit, AVPlayer, свой FFmpeg → VideoToolbox) пишутся на Swift, а контрактный набор (`contract::CASES`) — на Rust. Нужно решить: как Swift-движок становится движком ядра, как на нём гонять тот же контрактный набор и какой MPVKit брать, чтобы не втянуть GPL (CLAUDE.md, research/02 §3.1).

## Факторы решения

- Один контрактный набор для всех движков, без переписывания на Swift и Kotlin.
- Один редьюсер состояния (`EngineState::apply`) — иначе инвариант `state() == fold(events)` расходится по языкам.
- В клиентах только LGPL-сборки FFmpeg и mpv; проверка автоматическая, а не на доверии.
- Сборка Apple только в CI (macOS); разработчикам без Mac нужен способ проверять Swift-код.

## Рассмотренные варианты

1. **Foreign trait `PlayerEngine` в `pa-ffi` + раннер контрактного набора в Rust, который вызывает Swift-харнесс.**
2. Переписать контрактный набор на Swift (XCTest) и Kotlin.
3. Движки на платформе без связи с ядром до PLAY-005.

## Решение

Выбран вариант **1**.

- `pa-ffi/src/playback.rs` повторяет типы `pa-playback` один в один (UniFFI-записи и перечисления, конверсии в обе стороны) и экспортирует трейты `PlayerEngine`, `EngineListener` с `with_foreign`. `ForeignEngine` превращает Swift/Kotlin-движок в `pa_playback::PlayerEngine`, `CoreEngine` — наоборот (mock для UI-тестов).
- Команда `open` в FFI называется **`open_media`**: UniFFI 0.32 экранирует ключевое слово Swift `open` обратными кавычками прямо в C-заголовке vtable foreign trait, и заголовок не компилируется.
- `EngineStateFold` — объект ядра с `apply(event)`/`state()`: Swift-движок применяет каждое своё событие через него, поэтому редьюсер один.
- `run_player_engine_contract_case(name, harness)` запускает случай `contract::CASES` против `PlayerEngineHarness` (foreign trait: `engine()`, `fixture()`, `advance(ms)`); паника случая возвращается как `ContractError::Failed`. XCTest вызывает его по случаю на метод.
- `clients/apple/AlphaPlayerKit` — SwiftPM-пакет: `MPVEngine` (libmpv из MPVKit, один файл в mpv с `keep-open=yes`, плейлист/повтор/конец — в движке), `MPVVideoView` (CAMetalLayer, `vo=gpu-next` через MoltenVK). Его используют Flutter-плагин (PLAY-APL-004) и tvOS-приложение.
- **MPVKit:** продукт `MPVKit` (не `MPVKit-GPL`), версия закреплена `exact:` (`1.1.0-n9.0.2`: mpv 0.41, FFmpeg 8.0, `-Dgpl=false`, FFmpeg `--enable-version3` → LGPLv3). `scripts/check-license.sh` проверяет манифест (с канарейкой `-GPL`, которая обязана упасть) и строки собранных Mach-O: нужен `-Dgpl=false`, запрещены `-Dgpl=true`, `--enable-gpl`, `--enable-nonfree`, «GPL version» без «L». Тест `testLinkedMPVIsLGPL` дополнительно читает `mpv-configuration` в рантайме, без возможности отключить проверку на Apple.
- **MPVKit — только для разработки и CI, не для релиза.** Политика проекта для сборок в App Store — только LGPL-2.1 (CLAUDE.md, ADR-0005; LGPLv3 и App Store — известный конфликт, research/02 §3.1), а FFmpeg в MPVKit собран с `--enable-version3` (LGPL-3.0). Поэтому у лицензионной проверки два профиля: `dev` (по умолчанию, MPVKit разрешён) и `--store` (`PA_LICENSE_PROFILE=store` в `scripts/test.sh`): в нём падает любая зависимость от MPVKit в манифесте и любые `--enable-version3` / «LGPL version 3» в бинарниках. CI явно проверяет, что `--store` отвергает текущий манифест, и помечает сборку как нерелизную. Релизный конвейер Apple (CI-окружение `store`) обязан запускать проверку в профиле `--store`. Замена — PLAY-APL-013: своя LGPL-2.1-сборка FFmpeg (`tools/ffmpeg`, цели `ios-*`/`tvos-*` уже есть) и libmpv (`tools/mpv`, расширить на iOS и tvOS) в xcframework.
- Для разработки без Mac — `scripts/linux-dev-test.sh`: те же исходники и тесты против libmpv дистрибутива в Docker (не замена Apple CI, без LGPL-проверки).

Не входит: AVPlayer и свой движок (PLAY-APL-002, -005), `ByteSource` → AVIO (PLAY-APL-003), PiP/Now Playing/AVAudioSession (PLAY-APL-004), HDR-вывод и AFR (PLAY-APL-006, -009), Kotlin-харнесс (PLAY-AND-002 использует тот же экспорт).

### Последствия

- Хорошо: Swift-движок проходит ровно тот же набор, что mock и будущие Kotlin-движки; состояние сворачивается одним редьюсером; GPL-сборка MPVKit не пройдёт CI ни через манифест, ни через бинарник.
- Плохо: каждое событие — вызов через FFI (`EngineStateFold.apply`), это микросекунды при десятках событий в секунду. Имя `open_media` в FFI отличается от `open` ядра. Пока AlphaPlayerKit на MPVKit, Apple-сборки с ним нельзя выпускать в App Store (FFmpeg LGPL-3.0); профиль `--store` это блокирует.
- Требует работы: PLAY-APL-013 (своя LGPL-2.1-сборка FFmpeg + libmpv для iOS и tvOS, переход AlphaPlayerKit на неё) — до первого релиза на Apple. Обновление MPVKit до тех пор — новый `exact:` и прогон CI (лицензия проверяется заново).

### Как проверяем соблюдение

- Job `player` в `.github/workflows/apple-core.yml`: контрактный набор и сэмплы QA-002 (MKV) на симуляторах iOS и tvOS, проверка лицензии манифеста и бинарников.
- `cargo test -p pa-ffi`: набор через цепочку mock → FFI-трейт → ядро и экспортированный раннер.

## Ссылки

- `clients/apple/AlphaPlayerKit/README.md`
- `core/crates/pa-ffi/README.md` — раздел «Плеер»
- [MPVKit](https://github.com/mpvkit/MPVKit)
