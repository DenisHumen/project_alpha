<!-- SPDX-License-Identifier: MPL-2.0 -->
# packaging/app — сборка и релиз Flutter-приложения Kinolune

Задача R01-REL (релиз 0.1, ADR-0011, ADR-0012). Код приложения — `clients/flutter/app`, здесь только упаковка.

| Файл | Что делает |
|---|---|
| `.github/workflows/flutter.yml` | PR в `clients/flutter/**`: `dart format`, `flutter analyze`, тесты для приложения и каждого пакета; smoke-сборки Linux (с упаковкой) и Android debug. Обязательная проверка — `flutter-green` |
| `.github/workflows/release-app.yml` | Тег `app-v*` → сборка и GitHub Release; `workflow_dispatch` → dry run (артефакт `release-<версия>`, без публикации) |
| `linux/package.sh` | Linux-бандл Flutter → `kinolune-<v>-linux-x86_64.tar.gz` и `kinolune-<v>-x86_64.AppImage` (appimagetool 1.9.1 и runtime закреплены по SHA-256) |
| `android/sign.sh` | Переподпись APK ключом из секретов (apksigner, v1+v2+v3) или оставляет debug-подпись |
| `android/check-native-licences.sh` | Лицензионный гейт APK: только наш набор libmpv из `tools/mpv`, FFmpeg — отдельными `libav*/libsw*` с «LGPL version 2.1 or later» без `--enable-gpl/nonfree/version3`, libmpv — `-Dgpl=false` без вшитого FFmpeg; иначе релиз падает |
| `lgpl-sources.sh` | Исходники libmpv/FFmpeg для релиза: архив `tools/mpv/build.sh sources`, сверенный с `BUILDINFO.txt` вложенных сборок (см. ниже) |
| `THIRD-PARTY-NOTICES.md`, `fetch-licences.sh` | Уведомления и тексты лицензий (скачиваются из SPDX license-list-data по закреплённому тегу с проверкой SHA-256); кладутся в tar.gz, AppImage и в `kinolune-<v>-licenses.tar.gz` |

Версия Flutter закреплена в `env.FLUTTER_VERSION` обоих workflow (сейчас 3.50.0) — менять вместе.

## Артефакты релиза

`kinolune-<v>-x86_64.AppImage`, `kinolune-<v>-linux-x86_64.tar.gz`, `kinolune-<v>-android-{arm64-v8a,armeabi-v7a,x86_64,universal}.apk`, `kinolune-<v>-licenses.tar.gz`, `kinolune-<v>-libmpv-sources.tar.xz`, `SHA256SUMS`. Заметки релиза: текст аннотированного тега + таблица загрузок, статус подписи, состав LGPL-компонентов и отчёт гейта.

- `versionCode` Android = номер запуска workflow (`github.run_number`), монотонно растёт; split-APK получают `+1000×ABI` (так делает Flutter). Обновляться стоит в пределах одного вида APK (split или universal).
- Версия с суффиксом (`0.1.0-rc.1`) публикуется как pre-release.

## Лицензии libmpv / FFmpeg (LGPL)

Оба платформенных артефакта несут **нашу** сборку libmpv + FFmpeg под LGPL-2.1+ из
[`tools/mpv`](../../tools/mpv/README.md) (PLAY-002, PLAY-013). Релиз собирает её в том же запуске:
`release-app.yml` вызывает `libmpv.yml` (`workflow_call`, `release: true` — только Linux x86_64,
три Android-ABI и архив исходников; кэш сборок с `main` переиспользуется).

- **Android.** Приложение подменяет `media_kit_libs_android_video` локальным пакетом
  [`clients/flutter/packages/media_kit_libs_android_video`](../../clients/flutter/packages/media_kit_libs_android_video/README.md)
  (`dependency_overrides`): Java-обвязка media_kit та же, но готовые jar (FFmpeg LGPL-3.0 внутри
  `libmpv.so`) не скачиваются. Перед `flutter build apk` шаг релиза кладёт наш набор в `jniLibs`:
  `tools/mpv/bundle.sh android <jniLibs> <libmpv-android-*>`. Release-сборка без набора падает.
  Гейт `check-native-licences.sh` проверяет каждый APK и ABI: только наш набор, FFmpeg — «LGPL
  version 2.1 or later» отдельными библиотеками, `libmpv.so` — `-Dgpl=false` без вшитого FFmpeg.
- **Linux.** `flutter build linux` идёт с `PKG_CONFIG_PATH=<libmpv-linux-x86_64>/lib/pkgconfig`
  (без `libmpv-dev`), `package.sh` с `PA_LIBMPV_DIR=<libmpv-linux-x86_64>` вызывает
  `tools/mpv/bundle.sh linux`: `libmpv.so.2` и FFmpeg — в `lib/`, лицензии и `BUILDINFO.txt` —
  в `licenses/libmpv/`, у `libmedia_kit_video_plugin.so` — `RUNPATH=$ORIGIN`. `AppRun` и `pa_player`
  открывают именно эту копию. Системная libmpv больше не нужна; нужен glibc ≥ 2.39 (сборка на Ubuntu 24.04).
- **Исходники.** `lgpl-sources.sh` берёт архив `libmpv-sources` того же запуска (все исходные архивы,
  оба рецепта и патчи), проверяет, что каждый SHA-256 из `BUILDINFO.txt` вложенных сборок есть в его
  `SHA256SUMS`, и кладёт его в релиз как `kinolune-<v>-libmpv-sources.tar.xz` (LGPL-2.1 §6:
  исходники «из того же места»). Таблица компонентов — в заметках релиза и `LIBMPV-COMPONENTS.md`.
- Тексты лицензий: SPDX-тексты скачивает `fetch-licences.sh` (тег и SHA-256 закреплены), файлы
  лицензий upstream из сборки — в `licenses/libmpv/` архива лицензий. Флаттеровский `NOTICES`
  кладётся в Linux-архивы как `licenses/FLUTTER-NOTICES.txt`; в Android он доступен на экране
  лицензий приложения (`showLicensePage`).

Локальная сборка APK: собрать `tools/mpv/build.sh android-<abi>` (или `docker-build.sh`) либо скачать
CI-артефакты `libmpv-android-*`, выполнить `bundle.sh android` (см. README пакета), затем
`flutter build apk --release`.

## Шаги владельца

1. **Ключ подписи Android (один раз, хранить вне репозитория и с резервной копией — потеря ключа = пользователи не смогут обновиться):**
   ```sh
   keytool -genkeypair -v -keystore kinolune-release.jks -alias kinolune \
     -keyalg RSA -keysize 4096 -validity 10000
   base64 -w0 kinolune-release.jks > kinolune-release.jks.b64
   ```
2. **Секреты репозитория** (Settings → Secrets and variables → Actions → New repository secret):
   `ANDROID_KEYSTORE_BASE64` (содержимое `.b64`), `ANDROID_KEY_ALIAS` (`kinolune`), `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_PASSWORD`.
   Без них релиз собирается, но APK подписаны одноразовым debug-ключом CI, и заголовок релиза помечен «Android debug-signed»: такие APK нельзя обновить следующим релизом без удаления.
3. **Dry run** перед тегом: Actions → «Release app» → Run workflow → ветка `main`, версия, например `0.1.0-rc.1`. Проверить артефакт `release-<версия>` (APK ставится, AppImage запускается, `notes.md`).
   Из CLI: `gh workflow run release-app.yml -f version=0.1.0-rc.1 && gh run watch`.
4. **Релиз:** аннотированный тег — его текст становится заметками релиза:
   ```sh
   git tag -a app-v0.1.0 -m "Kinolune 0.1.0" -m "Что нового: ..." && git push origin app-v0.1.0
   ```
5. По желанию: защитить теги `app-v*` (Settings → Rules → Tag rulesets), чтобы их могли создавать только мейнтейнеры.

## Требования к приложению (R01-APP)

- Путь `clients/flutter/app`, release-сборка Android подписывается debug-ключом по умолчанию (как в шаблоне `flutter create`) — CI сам переподписывает APK; не требовать `key.properties` для сборки.
- В Linux бандле ровно один исполняемый файл в корне (раннер Flutter).
- Закоммитить `pubspec.lock` — релиз ставит зависимости с `--enforce-lockfile`.
