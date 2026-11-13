# pa-ffi

Фасад ядра для Kotlin и Swift на [UniFFI](https://mozilla.github.io/uniffi-rs/) (proc-macro, ADR-0001).

- Каждый домен экспортируется своим модулем `src/<domain>.rs`; в `src/lib.rs` добавляется одна строка `mod`.
- Сейчас экспортирована одна функция: `core_version()` → версия крейта (`coreVersion()` в Kotlin и Swift).
- Kotlin: пакет `dev.projectalpha.core`, библиотека `pa_ffi`. Swift: модуль `PACore`, C-модуль `PACoreFFI` (`uniffi.toml`).

## Команды

```bash
just core test-ffi      # Rust-тесты, включая вызов через C ABI
just core bindings      # target/bindings/{kotlin,swift}
just core smoke-jvm     # Kotlin на JVM через JNA (JDK 21 и gradle в PATH)
just core smoke-swift   # Swift на хосте (swiftc, Linux или macOS)
```

Сгенерированные привязки не коммитятся. Сборка под Android (cargo-ndk, AAR) — CORE-004, xcframework и SwiftPM — CORE-003.
