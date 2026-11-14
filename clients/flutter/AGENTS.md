<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `clients/flutter/` (Flutter-приложение)

Общие правила — в корневом [AGENTS.md](../../AGENTS.md).

- **Что здесь:** приложение для Android, Android TV, iOS, iPadOS, macOS, Windows, Linux (ADR-0002) и его пакеты в `packages/`. Для релиза 0.1 раскладка и API зафиксированы в [docs/dev/release-0.1-contract.md](../../docs/dev/release-0.1-contract.md): приложение `app/`, пакеты `packages/pa_library/` (чистый Dart), `packages/pa_player/` (media_kit, ADR-0012). Владельцы — по таблице контракта и ROADMAP §0.4.
- **Флейворы** `store` и `full` (ADR-0007, CORE-013, [docs/dev/flavors.md](../../docs/dev/flavors.md)): Dart — `--dart-define=KINOLUNE_FLAVOR=store|full` (`lib/flavor.dart`), Rust-ядро — переменная окружения `CARGO_KINOLUNE_FLAVOR=store|full` для хука `pa_core`. Задавайте обе одинаково; по умолчанию обе `store`. Проверяйте оба флейвора.
- **Инструменты:** Flutter stable (версию закрепит REPO-008, `mise.toml`); для Android — Android SDK и JDK; для Linux — `clang`, `cmake`, `ninja`, `pkg-config`, `libgtk-3-dev`, для media_kit — `libmpv-dev` или наш libmpv (`app/tool/native_libs.sh`, см. ниже).

## Сборка и тесты

Каждый пакет и приложение — отдельный Dart-проект; команды выполняются в его каталоге:

```bash
cd clients/flutter/packages/pa_library && dart pub get && dart analyze && dart test
cd clients/flutter/packages/pa_player  && flutter pub get && flutter analyze && flutter test
cd clients/flutter/app                 && flutter pub get && flutter analyze && flutter test
dart format --output=none --set-exit-if-changed .
CARGO_KINOLUNE_FLAVOR=full flutter build linux --dart-define=KINOLUNE_FLAVOR=full   # из clients/flutter/app
CARGO_KINOLUNE_FLAVOR=full flutter build apk --debug --dart-define=KINOLUNE_FLAVOR=full
```

Golden-тесты: `flutter test --update-goldens` только осознанно, diff приложить к PR.

### Нативные LGPL-библиотеки (libmpv, libsmb2) для локальных сборок

APK без нашего `libmpv.so` в `jniLibs` закрывается при старте (плагин media_kit грузит его сразу), без `libsmb2.so` не работают SMB-источники. В git этих библиотек нет; релиз собирает их в CI (`release-app.yml`). Для локальной или отладочной сборки их кладёт `app/tool/native_libs.sh`:

```bash
clients/flutter/app/tool/native_libs.sh fetch     # свежие CI-артефакты libmpv-* и libsmb2-* с main (нужен gh auth)
clients/flutter/app/tool/native_libs.sh build     # или собрать из tools/mpv и tools/libsmb2 (Docker; libmpv — около часа на таргет)
clients/flutter/app/tool/native_libs.sh status    # что лежит в кэше (~/.cache/kinolune-native) и в jniLibs
cd clients/flutter/app && flutter build apk --debug
```

Артефакты CI живут 7 дней; если их нет, `fetch` подскажет запустить `gh workflow run libmpv.yml` и `gh workflow run libsmb2.yml` на `main`. Linux: `eval "$(app/tool/native_libs.sh env)"` перед `flutter run -d linux` — сборка возьмёт наш libmpv вместо `libmpv-dev`, ядро — наш libsmb2 (`PA_SMB2_LIB`); `native_libs.sh install --linux-bundle build/linux/x64/release/bundle` кладёт обе библиотеки в собранный бандл, как `packaging/app/linux/package.sh`.

## Правила области

- В каждом `.dart`, `.kt`, `.cc` первой строкой SPDX-заголовок (`// SPDX-License-Identifier: MPL-2.0`). Сгенерированные Flutter-файлы платформ (registrant, шаблоны `flutter create`, иконки по умолчанию) описываются в [REUSE.toml](../../REUSE.toml).
- Горячие файлы (ROADMAP §0.5): `pubspec.yaml` — зависимости по алфавиту, одна строка; роутер — одна строка на фичу (`routes.dart`); `.arb` — ключи с префиксом фичи, только добавление.
- Зависимости pub — только с совместимой лицензией; плееры — только LGPL-сборки libmpv/FFmpeg.
- Строки UI — через l10n (русский и английский), без хардкода (UI-020):
  - тексты — в `app/lib/l10n/app_en.arb` (шаблон, с описаниями и плейсхолдерами) и `app_ru.arb`; ключ начинается с префикса фичи (`settings…`, `music…`); в виджетах — `context.l10n.ключ` (`lib/l10n/l10n.dart`);
  - множественное число — ICU `plural`; в русском обязательны формы `one`, `few`, `many`, `other`;
  - Dart-код генерируется `flutter pub get` (`generate: true`, `l10n.yaml`) и не коммитится;
  - после правки `app_en.arb` выполнить `dart run tool/pseudo_l10n.dart` (псевдолокаль `en-XA`, доступна в настройках отладочной сборки);
  - `test/l10n_test.dart` падает на непереведённом ключе, устаревшей псевдолокали и захардкоженной строке в `lib/` (непереводимый литерал — пометка `// l10n-ignore`); `test/l10n_layout_test.dart` проверяет вёрстку экранов на ru и en-XA без переполнений.
