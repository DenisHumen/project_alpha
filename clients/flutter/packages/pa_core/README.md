# pa_core

Dart-привязки к Rust-ядру project_alpha через [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge) 2.13 (ADR-0001, ADR-0013). Rust-сторона — крейт [`core/crates/pa-frb`](../../../../core/crates/pa-frb).

Чистый Dart-пакет: нативная библиотека `libpa_frb` собирается build hook'ом Native Assets (`hook/build.dart`) при `dart test`, `flutter test`, `flutter build` и `flutter run` и попадает в приложение сама (Linux — `bundle/lib/`, Android — `jniLibs` всех ABI). Нужны `rustup` и toolchain из `core/crates/pa-frb/rust-toolchain.toml` (rustup поставит его и цели сам); для Android — NDK из Android SDK.

## API

```dart
import 'package:pa_core/pa_core.dart';

await RustLib.init();                       // один раз, до остальных вызовов

coreVersion();                              // "0.1.0" — синхронно, та же версия, что у UniFFI (pa-ffi)

final r = await parseFilename(path: 'Shows/Dark/Season 1/Dark.S01E02.mkv');
r.hypotheses.first.parsed.title;            // "Dark" — N-best гипотезы парсера META-005, лучшая первой
r.hypotheses.first.weight;                  // вес гипотезы (0, 1]
await parseFilenameWith(path: p, maxHypotheses: 1, useFolders: false);
filenameParserVersion();                    // версия правил парсера, хранить вместе с результатом

tickStream(count: 3);                       // Stream<int> 0,1,2 — проверка Rust → Dart Stream

final lib = await CoreLibrary.open(path: '/…/kinolune-core.db');  // медиатека на ядре (pa-catalog, UI-024)
await lib.addSource(kind: 'local', name: 'Фильмы', location: '/media/movies');
await lib.scan(roots: []).last;              // скан → теги музыки → отпечатки → распознавание → поиск
await lib.movies(); await lib.search(query: 'matrix', limit: 20);
await lib.continueWatching(limit: 50);      // «Продолжить» и Up Next
await lib.importV0(path: '/…/kinolune.db');  // перенос базы pa_library v0
```

Приложение использует медиатеку не напрямую, а через адаптер `RustLibrary` из `pa_library` (тот же интерфейс `Library`).

`parseFilename` — замена Dart-парсера v0 из `pa_library` (релиз 0.2): путь передаётся относительно корня источника, папки дают контекст (сезон, название сериала).

## Команды

```bash
cd clients/flutter/packages/pa_core
dart pub get && dart analyze && dart test     # тесты вызывают настоящее ядро
cd example && flutter build linux --debug      # smoke-приложение: версия ядра и Stream
cd example && flutter build apk --debug
cd example && flutter test integration_test -d linux

just core frb          # перегенерировать привязки после правки pa-frb/src/api
just core frb-check    # то же, что в CI: diff после регенерации должен быть пустым
just core frb-install  # flutter_rust_bridge_codegen 2.13.0 (или `mise install`)
```

## Как добавить функцию

1. Модуль `core/crates/pa-frb/src/api/<domain>.rs` и строка `pub mod <domain>;` в `src/api/mod.rs`. Типы ядра не экспортируются напрямую — зеркальные структуры и `From` (пример — `recognition.rs`).
2. `just core frb`, экспорт `src/rust/api/<domain>.dart` в `lib/pa_core.dart`.
3. Тесты: Rust — в модуле, Dart — в `test/`. Сгенерированное коммитится вместе с изменением.

## Платформы

Сейчас собираются и проверяются в CI (`.github/workflows/pa-core.yml`) Linux x64 и Android (arm64-v8a, armeabi-v7a, x86_64). Windows, macOS и iOS — цели в `rust-toolchain.toml` крейта и smoke-сборки добавит CORE-010.
