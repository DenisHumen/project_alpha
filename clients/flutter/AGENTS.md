<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `clients/flutter/` (Flutter-приложение)

Общие правила — в корневом [AGENTS.md](../../AGENTS.md).

- **Что здесь:** приложение для Android, Android TV, iOS, iPadOS, macOS, Windows, Linux (ADR-0002) и его пакеты в `packages/`. Для релиза 0.1 раскладка и API зафиксированы в [docs/dev/release-0.1-contract.md](../../docs/dev/release-0.1-contract.md): приложение `app/`, пакеты `packages/pa_library/` (чистый Dart), `packages/pa_player/` (media_kit, ADR-0012). Владельцы — по таблице контракта и ROADMAP §0.4.
- **Флейворы** `store` и `full` (ADR-0007): `--dart-define=FLAVOR=store|full`, Gradle product flavors, Xcode schemes. Проверяйте оба.
- **Инструменты:** Flutter stable (версию закрепит REPO-008, `mise.toml`); для Android — Android SDK и JDK; для Linux — `clang`, `cmake`, `ninja`, `pkg-config`, `libgtk-3-dev`, для media_kit — `libmpv-dev`.

## Сборка и тесты

Каждый пакет и приложение — отдельный Dart-проект; команды выполняются в его каталоге:

```bash
cd clients/flutter/packages/pa_library && dart pub get && dart analyze && dart test
cd clients/flutter/packages/pa_player  && flutter pub get && flutter analyze && flutter test
cd clients/flutter/app                 && flutter pub get && flutter analyze && flutter test
dart format --output=none --set-exit-if-changed .
flutter build linux --dart-define=FLAVOR=full     # из clients/flutter/app
flutter build apk --debug --dart-define=FLAVOR=full
```

Golden-тесты: `flutter test --update-goldens` только осознанно, diff приложить к PR.

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
