# pa-ffi

Фасад ядра для Kotlin и Swift на [UniFFI](https://mozilla.github.io/uniffi-rs/) (proc-macro, ADR-0001).

- Каждый домен экспортируется своим модулем `src/<domain>.rs`; в `src/lib.rs` добавляется одна строка `mod`.
- `core_version()` → версия крейта (`coreVersion()` в Kotlin и Swift).
- Жизненный цикл и настройки (`src/runtime.rs`, CORE-006, ADR-0014): `core_init(config, callbacks)`, `core_enter_foreground()`, `core_enter_background()`, `core_shutdown()`, `core_lifecycle()`, `core_dirs()`; колбэк-интерфейс `CoreCallbacks`; `settings_get/set/remove/keys/export/import` (значения — JSON-текст). На Android в `CoreConfig.dirs` передаются `filesDir` и `cacheDir`; на iOS и tvOS по умолчанию — `Library/Application Support` и `Library/Caches`.
- Kotlin: пакет `dev.projectalpha.core`, библиотека `pa_ffi`. Swift: модуль `PACore`, C-модуль `PACoreFFI` (`uniffi.toml`).

## Команды

```bash
just core test-ffi      # Rust-тесты, включая вызов через C ABI
just core bindings      # target/bindings/{kotlin,swift}
just core smoke-jvm     # Kotlin на JVM через JNA (JDK 21 и gradle в PATH)
just core smoke-swift   # Swift на хосте (swiftc, Linux или macOS)
just core xcframework   # macOS + Xcode: PACoreFFI.xcframework и PACore.swift в tools/bindings/PACore
just core test-pacore ios   # XCTest PACore: macos | ios | tvos (симулятор)
```

Сгенерированные привязки не коммитятся. Сборка под Android (cargo-ndk, AAR) — CORE-004.

## Apple: SwiftPM-пакет `PACore` (CORE-003)

`tools/bindings/build-xcframework.sh` собирает `libpa_ffi.a` под iOS, iOS Simulator (arm64 + x86_64), tvOS, tvOS Simulator (arm64) и macOS (arm64 + x86_64) и упаковывает их в `PACoreFFI.xcframework` (binary target) рядом с `Package.swift`; Swift-обёртка UniFFI кладётся в `Sources/PACore/PACore.swift`. tvOS — tier 3 в Rust: std собирается из `rust-src` (`-Zbuild-std` при `RUSTC_BOOTSTRAP=1`) той же закреплённой версии компилятора. Минимальные версии: iOS 15, tvOS 15, macOS 12. Клиенты подключают пакет локально: `.package(path: "…/tools/bindings/PACore")`, продукт `PACore`. В CI — `.github/workflows/apple-core.yml` на `macos-15`.
