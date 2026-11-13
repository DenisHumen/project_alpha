# ADR-0015: Собирать pa_core через Native Assets и коммитить привязки FRB

- **Статус:** Принят
- **Дата:** 2026-11-13
- **Решают:** владелец проекта (решение делегировано агенту, ADR-0011); участники обсуждения — по PR
- **Связано:** `CORE-002`, `CORE-010`, ADR-0001

## Контекст и проблема

ADR-0001 выбрал flutter_rust_bridge (FRB) для Dart и пакет `clients/flutter/packages/pa_core`. Нужно решить два вопроса реализации:

1. Как собирать Rust-библиотеку под каждую Flutter-платформу. У FRB 2.13 два бэкенда: **cargokit** (скрипты и платформенные каркасы — CMake, Gradle, Podspec — внутри плагина) и **Native Assets** (build hook `hook/build.dart` на `native_toolchain_rust`; Dart и Flutter сами запускают его на `dart test`, `flutter test`, `flutter build`, `flutter run`).
2. Коммитить ли сгенерированные привязки. ADR-0001 и `core/AGENTS.md` говорят «сгенерированные привязки не коммитятся». Для UniFFI это работает: Kotlin и Swift генерируются на этапе сборки. Но у FRB без сгенерированного `frb_generated.rs` не компилируется сам крейт (`cargo clippy --workspace` в CI упадёт), а без Dart-части не анализируется ни `pa_core`, ни приложение, которое от него зависит.

## Факторы решения

- `flutter build` приложения работает «из коробки», без ручных шагов и без codegen на машине.
- Dart-тесты вызывают настоящее ядро на Linux без отдельной сборки.
- Минимум платформенного кода в репозитории (меньше файлов для REUSE и ревью).
- Общий Cargo workspace и закреплённый toolchain (`rust-toolchain.toml`).
- Workflow `rust.yml` (clippy по всему workspace) и Flutter CI остаются зелёными без установки codegen в каждом job.

## Рассмотренные варианты

1. Native Assets + сгенерированный код в git, CI проверяет актуальность.
2. cargokit + сгенерированный код в git.
3. Native Assets, сгенерированный код не коммитится; codegen в каждом CI-job и у каждого разработчика.

## Решение

Выбран вариант **1**.

- Крейт `core/crates/pa-frb` (член workspace по глобу) — фасад ядра для Dart; модули `src/api/<domain>.rs`. Swift и Kotlin по-прежнему идут через `pa-ffi` (UniFFI).
- Пакет `pa_core` — чистый Dart-пакет (без зависимости от Flutter) с `hook/build.dart`, который собирает `pa-frb` через `rustup run <channel> cargo build --target <triple>`. Каналу нужен `rust-toolchain.toml` рядом с крейтом: `core/crates/pa-frb/rust-toolchain.toml` с тем же `channel`, что и корневой, и списком целей. Совпадение каналов проверяет CI (`pa-core.yml`).
- Сгенерированные файлы `core/crates/pa-frb/src/frb_generated.rs` и `clients/flutter/packages/pa_core/lib/src/rust/**` **коммитятся** (исключение из правила ADR-0001, как и файлы для Xcode). Генерация — `just core frb`, проверка — `just core frb-check` (job `bindings` в `pa-core.yml`). `rustfmt` их не трогает (`#[rustfmt::skip]`), `dart analyze` исключает.
- Версия FRB закреплена точно (`=2.13.0` в Cargo, `2.13.0` в pubspec и `mise.toml`); обновлять все три вместе.

### Последствия

- Хорошо: приложение подключает `pa_core` через `path:` и собирается на Linux и Android без лишних шагов; `dart test` сам собирает ядро; в репозитории нет cargokit-каркасов.
- Плохо: ревьюеры видят diff сгенерированного кода; второй `rust-toolchain.toml`; Native Assets моложе cargokit, а FRB загружает библиотеку по имени, так что для macOS/iOS/Windows (CORE-010) загрузку надо проверить отдельно.
- Требует работы: Windows, macOS, iOS — добавить цели в `core/crates/pa-frb/rust-toolchain.toml` и smoke-сборки (CORE-010).

### Как проверяем соблюдение

- `pa-core.yml`: регенерация без diff, совпадение каналов toolchain, `dart test`, сборка example под Linux (с integration-тестом) и Android.
- `rust.yml`: clippy и тесты `pa-frb` вместе со всем workspace.

## Плюсы и минусы вариантов

### cargokit

- Плюс: проверен годами, шаблон FRB по умолчанию.
- Минус: десятки файлов каркаса и скриптов в каждом плагине; свой выбор toolchain (по умолчанию `stable`, у нас закреплён 1.99.0); `dart test` без Flutter-сборки не находит библиотеку.

### Не коммитить сгенерированное

- Плюс: следует букве ADR-0001.
- Минус: `cargo clippy --workspace` и `flutter analyze` приложения ломаются без codegen; установка codegen (`cargo install`, минуты) в каждый CI-job и у каждого участника.

## Ссылки

- [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge), [native_toolchain_rust](https://github.com/GregoryConrad/native_toolchain_rust)
- [Dart hooks / Native Assets](https://dart.dev/tools/hooks)
