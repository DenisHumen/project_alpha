<!-- SPDX-License-Identifier: MPL-2.0 -->

# ADR-0041: Флейвор сборки — один крейт `pa-flavor`, cfg через `links`, Dart-константа и переменная окружения для хука

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта (мейнтейнер); участники обсуждения — по PR
- **Связано:** `CORE-013`, `TWI-004`, `TWI-017`, `TWI-018`, `UI-016`, `PKG-014`, [ADR-0007](0007-store-full-build-flavors.md), [ADR-0011](0011-maintainer-decisions-release-0-1.md) §5, [ADR-0015](0015-frb-native-assets-committed-bindings.md), [ADR-0035](0035-apple-engines-foreign-player-engine-mpvkit.md)

## Контекст и проблема

ADR-0007 решил, что `store` и `full` — флейворы времени компиляции, а store-бинарь физически не содержит архива Twitch и донат-ссылок. Как именно это собрать, не было решено. TWI-004 завёл временную фичу `full` в одном `pa-twitch`. Нужен один механизм на весь workspace и на все клиенты, флейвор во время работы и CI-проверка store-артефактов (`nm`/`strings`).

Ограничения:

- cargo унифицирует фичи: если каждый крейт заведёт свою фичу `full`, половина сборки может оказаться в другом флейворе.
- `rust.yml` собирает и тестирует с `--all-features`.
- Flutter собирает Rust-ядро Native Assets хуком (ADR-0015). Хук не получает `--dart-define` и `--flavor`; из окружения hooks runner пропускает только белый список и префиксы (`CARGO_`, `RUSTUP_`, …) и перезапускает хук при их изменении.
- `flutter build --flavor` (и `default-flavor` в pubspec) меняет пути выходных файлов на всех платформах: `app-<flavor>-release.apk`, `build/linux/x64/<flavor>/release/bundle`. На эти пути завязаны `release-app.yml`, `flutter.yml`, `pa-core.yml`, скрипты агентов на сборочном сервере.

## Факторы решения

- Один флаг флейвора на весь Rust-граф, рассогласование невозможно.
- По умолчанию — безопасный `store`: забытый флаг даёт урезанную, а не запрещённую сборку.
- Не ломать существующие сборки, пути артефактов и имена файлов релиза.
- Проверка store-артефактов с позитивным контролем: доказать, что проверка видит full-код в артефакте такого вида.

## Рассмотренные варианты

1. **Крейт `pa-flavor` с фичами и `links`-метаданными**: фичи `store`, `full`, возможности `twitch-native`, `twitch-archive`, `donation-links`; build-скрипт публикует итог как `DEP_PA_FLAVOR_*`, крейты с full-only кодом переводят его в `cfg` своим `build.rs`.
2. Своя фича `full` в каждом крейте с пробросом вниз по графу.
3. Глобальный `--cfg pa_flavor="full"` через `RUSTFLAGS`.
4. Флейвор только во время работы (ADR-0007 его уже отверг).

Для Flutter:

- A. `--dart-define=KINOLUNE_FLAVOR` (Dart) + `CARGO_KINOLUNE_FLAVOR` (ядро через хук).
- B. `flutter build --flavor` с Gradle productFlavors и `default-flavor` (Dart-константа `FLUTTER_APP_FLAVOR`) + `CARGO_KINOLUNE_FLAVOR`.
- C. `hooks.user_defines` в pubspec приложения.

## Решение

Выбраны вариант **1** и вариант **A**.

**Rust.** Крейт `core/crates/pa-flavor`:

- фичи `store` (маркер), `full = [twitch-native, twitch-archive, donation-links]` и сами возможности. Сборка — `store` ровно тогда, когда не включена ни одна возможность; без флагов это `store`, `--all-features` — `full`;
- `links = "pa_flavor"`: build-скрипт отдаёт флейвор и возможности как `DEP_PA_FLAVOR_*`; `pa_flavor::build::emit_cfgs()` в `build.rs` зависимого крейта ставит `cfg(pa_flavor = "store" | "full")`, `cfg(pa_twitch_native)`, `cfg(pa_twitch_archive)`, `cfg(pa_donation_links)` с `check-cfg`. Фичи `pa-flavor` унифицируются, поэтому все крейты одной сборки видят один флейвор;
- рантайм: `Flavor::CURRENT`, `enabled_capabilities()`, маркеры `kinolune-full-only:<id>`, которые компилируются только в full;
- артефактные крейты `pa-frb` и `pa-ffi` имеют фичи `store`/`full` (проброс в `pa-flavor`) и экспортируют `build_flavor()` и `build_capabilities()` в Dart (FRB) и Swift/Kotlin (UniFFI). `build_capabilities()` возвращает маркеры и держит их в библиотеке;
- временная фича `full` крейта `pa-twitch` удалена: GQL/usher/HLS теперь под `cfg(pa_twitch_native)`.

**Flutter.** Dart — константа `KINOLUNE_FLAVOR` (`--dart-define`, по умолчанию `store`, `lib/flavor.dart`); full-only Dart-код пишется под этой константой, AOT выбрасывает его из store. Ядро — переменная `CARGO_KINOLUNE_FLAVOR` (по умолчанию `store`), которую хук `pa_core` передаёт как фичу `pa-frb`; префикс `CARGO_` нужен, чтобы её пропустил hooks runner. Приложение при старте сравнивает Dart-флейвор с `buildFlavor()` ядра.

**Gradle productFlavors пока не вводим** (вариант B отложен): с ними `flutter build` без `--flavor` не находит APK, а `default-flavor` меняет пути артефактов на всех платформах и ломает действующие сборки. Сейчас Android-ресурсы и манифест у флейворов одинаковые, Gradle-флейвору нечего менять. Вводить productFlavors — когда store-сборке понадобится свой манифест или ресурсы (PKG-* для Google Play); тогда же переносить пути в `release-app.yml`, `flutter.yml`, `pa-core.yml`.

**Apple.** Build setting `PA_FLAVOR` (project.yml) задаёт Swift-условие `PA_FLAVOR_STORE`/`PA_FLAVOR_FULL`; без него — ошибка компиляции. `tools/bindings/build-xcframework.sh` и `build-aar.sh` берут `PA_FLAVOR` (по умолчанию `store`). tvOS — только `store`; Apple store-релизы по-прежнему заблокированы до LGPL-2.1 FFmpeg (ADR-0035) — этот ADR этого не меняет.

**Проверка.** `tools/flavor/check-artifacts.sh store|full` (`strings -a` + `nm -C` по ELF, Mach-O, PE, ar внутри APK/AAB/zip/tar/каталогов) против `tools/flavor/full-only-patterns.txt`. В режиме `full` — позитивный контроль: падает, если ничего не нашёл. Запуски: `flavors.yml` (Rust-библиотеки и `pa-twitch`-пробник обоих флейворов на Linux, store AAB приложения), `apple-core.yml` (store xcframework + full `libpa_ffi.a`), `release-app.yml` (full-артефакты релиза).

**Релиз.** `release-app.yml` собирает `full` для GitHub Releases; имена файлов не изменились, флейвор указан в заметках релиза. Store-сборки для магазинов — PKG-*; пока `flavors.yml` собирает store AAB как проверочную сборку.

### Последствия

- Хорошо: один флаг на весь Rust-граф; безопасный флейвор по умолчанию; существующие пути и имена артефактов не изменились; проверка доказывает, что видит full-код.
- Плохо: у Flutter-сборки две ручки (`--dart-define` и `CARGO_KINOLUNE_FLAVOR`) — расхождение ловит проверка при старте и CI, но не компилятор. Крейту с full-only кодом нужен `build.rs` и двойная зависимость от `pa-flavor`. Список паттернов надо пополнять руками вместе с новым full-only кодом.
- Требует работы: TWI-017/018 кладут код под `cfg(pa_twitch_archive)` и добавляют паттерны; UI-016 — донат-ссылки под `AppFlavor.donationLinks`; PKG-014/PKG-* — store-сборки для магазинов и, при необходимости, Gradle productFlavors.

### Как проверяем соблюдение

- CI: `flavors.yml`, шаг в `apple-core.yml`, позитивный контроль в `release-app.yml`.
- Тесты: `pa-flavor` (store без маркеров, full со всеми), `pa-ffi`/`pa-frb` (`build_flavor` согласован с возможностями), `pa_core` (`buildFlavor()` равен флейвору хука), `app/test/flavor_test.dart`, `TVAppTests.testRustCoreIsTheSameFlavor`.
- Ревью: новый full-only код без cfg или без строки в `full-only-patterns.txt` — замечание к PR.

## Плюсы и минусы вариантов

### Своя фича `full` в каждом крейте

- Плюс: без `build.rs`.
- Минус: унификация фич даёт рассогласование (`pa-flavor` в full, крейт — нет или наоборот); каждый артефакт должен знать все крейты с full-кодом.

### `RUSTFLAGS="--cfg …"`

- Плюс: глобально и просто.
- Минус: сбрасывает кэш сборки, не виден cargo-фичам и `--all-features`, хук FRB его не передаёт.

### `flutter build --flavor` + productFlavors

- Плюс: одна ручка для Dart и Android, стандартный путь Flutter.
- Минус: меняет пути артефактов на всех платформах, без `--flavor` сборка ломается; ядро всё равно нужно флейворить отдельно.

### `hooks.user_defines`

- Плюс: официальный канал в хук.
- Минус: значение живёт в `pubspec.yaml` — для смены флейвора надо править файл.
