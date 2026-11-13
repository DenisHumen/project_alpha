# pa-ffi

Фасад ядра для Kotlin и Swift на [UniFFI](https://mozilla.github.io/uniffi-rs/) (proc-macro, ADR-0001).

- Каждый домен экспортируется своим модулем `src/<domain>.rs`; в `src/lib.rs` добавляется одна строка `mod`.
- `core_version()` → версия крейта (`coreVersion()` в Kotlin и Swift).
- Жизненный цикл и настройки (`src/runtime.rs`, CORE-006, ADR-0014): `core_init(config, callbacks)`, `core_enter_foreground()`, `core_enter_background()`, `core_shutdown()`, `core_lifecycle()`, `core_dirs()`; колбэк-интерфейс `CoreCallbacks`; `settings_get/set/remove/keys/export/import` (значения — JSON-текст). На Android в `CoreConfig.dirs` передаются `filesDir` и `cacheDir`; на iOS и tvOS по умолчанию — `Library/Application Support` и `Library/Caches`.
- Контракт плеера (`src/playback.rs`, PLAY-AND-002, ADR-0017): типы `pa-playback` экспортируются как есть (remote-типы UniFFI); foreign-трейты `PlayerEngine` (метод `close` назван `close_engine` — в Kotlin он конфликтует с `AutoCloseable.close`) и `EngineListener`; `ForeignEngine` превращает платформенный движок в `pa_playback::PlayerEngine`; общий редьюсер `engine_state_apply`, JSON событий и состояния; `player_engine_contract_cases/run` с `PlayerEngineHarness` — контрактный набор ядра против Kotlin/Swift-движка на устройстве.
- Kotlin: пакет `dev.projectalpha.core`, библиотека `pa_ffi`. Swift: модуль `PACore`, C-модуль `PACoreFFI` (`uniffi.toml`).

## Команды

```bash
just core test-ffi      # Rust-тесты, включая вызов через C ABI
just core bindings      # target/bindings/{kotlin,swift}
just core smoke-jvm     # Kotlin на JVM через JNA (JDK 21 и gradle в PATH)
just core smoke-swift   # Swift на хосте (swiftc, Linux или macOS)
just core xcframework   # macOS + Xcode: PACoreFFI.xcframework и PACore.swift в tools/bindings/PACore
just core test-pacore ios   # XCTest PACore: macos | ios | tvos (симулятор)
just core aar           # Android: target/android/pa-core-android.aar (cargo-ndk, Android SDK + NDK)
just core check-aar     # проверка содержимого AAR без устройства
just core test-aar      # instrumented-тест на запущенном эмуляторе или устройстве
```

Сгенерированные привязки не коммитятся.

## Apple: SwiftPM-пакет `PACore` (CORE-003)

`tools/bindings/build-xcframework.sh` собирает `libpa_ffi.a` под iOS, iOS Simulator (arm64 + x86_64), tvOS, tvOS Simulator (arm64) и macOS (arm64 + x86_64) и упаковывает их в `PACoreFFI.xcframework` (binary target) рядом с `Package.swift`; Swift-обёртка UniFFI кладётся в `Sources/PACore/PACore.swift`. tvOS — tier 3 в Rust: std собирается из `rust-src` (`-Zbuild-std` при `RUSTC_BOOTSTRAP=1`) той же закреплённой версии компилятора. Минимальные версии: iOS 15, tvOS 15, macOS 12. Клиенты подключают пакет локально: `.package(path: "…/tools/bindings/PACore")`, продукт `PACore`. В CI — `.github/workflows/apple-core.yml` на `macos-15`.

## Android: AAR `pa-core-android` (CORE-004)

`tools/bindings/build-aar.sh` собирает `libpa_ffi.so` через cargo-ndk под arm64-v8a, armeabi-v7a и x86_64 (minSdk 24, NDK r29 — версия закреплена в скрипте и в `build.gradle.kts`), генерирует Kotlin-привязки UniFFI (пакет `dev.projectalpha.core`) и собирает Gradle-модуль `tools/bindings/pa-core-android` (AGP 9, Gradle 9.6+, JDK 17+). Результат — `target/android/pa-core-android.aar`; сгенерированные `jniLibs/` и `kotlin/` лежат рядом и не коммитятся. Рантайм привязок — JNA (`net.java.dev.jna:jna@aar`, `api`-зависимость; приложение должно её подтянуть, если подключает AAR файлом); правила R8 для JNA и привязок едут внутри AAR (`consumer-rules.pro`). 64-битные `.so` выровнены под 16 КБ страницы (Android 15+), это проверяет `tools/bindings/check-aar.sh`.

Instrumented-тест (`src/androidTest`) вызывает `coreVersion()`, запускает ядро в `filesDir`/`cacheDir` приложения, пишет настройку и читает её после перезапуска ядра. `test-aar.sh` гоняет его через `connectedDebugAndroidTest`; если подключено несколько устройств, нужен `ANDROID_SERIAL`. В CI — `.github/workflows/android-core.yml`: AAR, проверка содержимого и тест на эмуляторе API 34 x86_64.
