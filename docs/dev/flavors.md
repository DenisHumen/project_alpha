# Флейворы сборки `store` и `full`

Что входит в какой флейвор и почему — [ADR-0007](../adr/0007-store-full-build-flavors.md) и [ADR-0011](../adr/0011-maintainer-decisions-release-0-1.md) §5. Как это устроено в сборке — [ADR-NNNN](../adr/0041-build-flavor-mechanism.md) (CORE-013). Здесь — практика.

| Флейвор | Каналы | Что есть |
|---|---|---|
| `store` (по умолчанию) | App Store, Google Play, Microsoft Store | всё, кроме возможностей ниже |
| `full` | GitHub Releases, F-Droid, Flathub, AUR, winget, ISO, Docker | плюс `twitch-native` (нативное воспроизведение Twitch, TWI-004), `twitch-archive` (загрузки и запись Twitch, TWI-017/018), `donation-links` (внешние донат-ссылки) |

Флейвор не меняет форматы данных и протоколы.

## Как выбрать флейвор

| Где | store | full |
|---|---|---|
| Rust (тесты, инструменты) | без флагов | `--features pa-flavor/full` |
| Rust-артефакты `pa-frb`, `pa-ffi` | `--features store` | `--features full` |
| Flutter, Dart-код | `--dart-define=KINOLUNE_FLAVOR=store` (по умолчанию) | `--dart-define=KINOLUNE_FLAVOR=full` |
| Flutter, Rust-ядро (хук `pa_core`) | `CARGO_KINOLUNE_FLAVOR=store` (по умолчанию) | `CARGO_KINOLUNE_FLAVOR=full` |
| Apple xcframework, Android AAR | `PA_FLAVOR=store` (по умолчанию) | `PA_FLAVOR=full` для `tools/bindings/build-*.sh` |
| Xcode (TVApp) | build setting `PA_FLAVOR = store` | `PA_FLAVOR = full` |

Полная сборка приложения:

```bash
cd clients/flutter/app
CARGO_KINOLUNE_FLAVOR=full flutter build apk --release --dart-define=KINOLUNE_FLAVOR=full
CARGO_KINOLUNE_FLAVOR=full flutter build linux --release --dart-define=KINOLUNE_FLAVOR=full
```

У Flutter-сборки две половины: Dart (константа времени компиляции) и Rust-ядро (собирает Native Assets хук). Хук не видит `--dart-define`, а из окружения пропускает только переменные с известными префиксами — поэтому `CARGO_KINOLUNE_FLAVOR`. Задавайте обе одинаково. Приложение при старте сравнивает их (`checkCoreFlavor` в `lib/flavor.dart`): в debug расхождение — `assert`, в release — запись в лог. TVApp делает то же в `TVAppTests.testRustCoreIsTheSameFlavor`.

`--all-features` (так работает `rust.yml`) — это `full`.

## Как узнать флейвор во время работы

- Rust: `pa_flavor::Flavor::CURRENT`, `pa_flavor::enabled_capabilities()`.
- Dart: `AppFlavor.current`, `AppFlavor.donationLinks` и т. д. (`clients/flutter/app/lib/flavor.dart`); ядро — `buildFlavor()`, `buildCapabilities()` из `pa_core`.
- Swift/Kotlin: `buildFlavor()`, `buildCapabilities()` из PACore / `pa_ffi`; в TVApp — `BuildInfo.flavor`.

## Как писать full-only код

Rust: крейт зависит от `pa-flavor` и в `[dependencies]`, и в `[build-dependencies]`, а `build.rs` вызывает `pa_flavor::build::emit_cfgs()`. Код возможности — под её cfg:

```rust
#[cfg(pa_twitch_archive)]
mod archive;
```

Доступны `cfg(pa_flavor = "store" | "full")`, `cfg(pa_twitch_native)`, `cfg(pa_twitch_archive)`, `cfg(pa_donation_links)`. Своих фич `full` в крейтах не заводите: флаг один на весь workspace, cargo унифицирует фичи `pa-flavor`, поэтому половина сборки не может оказаться в другом флейворе. Пример — `pa-twitch` (`playback::{gql,usher,hls}`).

Dart: под константой, чтобы AOT-компилятор выбросил код из store-сборки:

```dart
if (AppFlavor.donationLinks) DonateLinks(),
```

Swift: `#if PA_FLAVOR_FULL`.

**В том же PR** добавьте отличительные строки и путь модуля нового кода в [`tools/flavor/full-only-patterns.txt`](../../tools/flavor/full-only-patterns.txt), иначе проверка их не увидит.

## Проверка артефактов

`tools/flavor/check-artifacts.sh store|full PATH...` раскрывает APK/AAB/zip/tar и каталоги (`.xcframework`), прогоняет `strings -a` и `nm -C` по каждому бинарю (ELF, Mach-O, PE, статические архивы) и ищет строки из `full-only-patterns.txt`:

- `store` — падает на любой находке;
- `full` — позитивный контроль: падает, если не нашёл ничего (значит, это не full-сборка или проверка «не видит» внутрь такого артефакта).

Маркеры `kinolune-full-only:<capability>` есть в каждом full-ядре (их возвращает `build_capabilities()`), поэтому позитивный контроль работает и для библиотек, где full-only кода пока нет.

Где запускается:

| Платформа | store-артефакт | Позитивный контроль |
|---|---|---|
| Linux, Rust | `flavors.yml` / `rust`: `libpa_frb.so`, `libpa_ffi.so`, `flavor_probe` (`pa-twitch`) | те же в full |
| Android | `flavors.yml` / `android`: store AAB целиком (ядро и `libapp.so` всех ABI), артефакт `store-android` | full APK в `release-app.yml` |
| Apple | `apple-core.yml`: store `PACoreFFI.xcframework`, все слайсы | full `libpa_ffi.a` для macOS там же |
| Linux, приложение | store-канала нет (Flathub — full) | full-бандл в `release-app.yml` |
| Windows | сборки ещё нет | — |

`release-app.yml` собирает `full` для GitHub Releases; имена файлов релиза не изменились. Store-сборки для магазинов (подпись, загрузка) — задачи PKG-*; пока `flavors.yml` собирает store AAB как проверочную сборку без libmpv.
