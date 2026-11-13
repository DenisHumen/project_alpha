<!-- SPDX-License-Identifier: MPL-2.0 -->
# packaging/app — сборка и релиз Flutter-приложения Kinolune

Задача R01-REL (релиз 0.1, ADR-0011, ADR-0012). Код приложения — `clients/flutter/app`, здесь только упаковка.

| Файл | Что делает |
|---|---|
| `.github/workflows/flutter.yml` | PR в `clients/flutter/**`: `dart format`, `flutter analyze`, тесты для приложения и каждого пакета; smoke-сборки Linux (с упаковкой) и Android debug. Обязательная проверка — `flutter-green` |
| `.github/workflows/release-app.yml` | Тег `app-v*` → сборка и GitHub Release; `workflow_dispatch` → dry run (артефакт `release-<версия>`, без публикации) |
| `linux/package.sh` | Linux-бандл Flutter → `kinolune-<v>-linux-x86_64.tar.gz` и `kinolune-<v>-x86_64.AppImage` (appimagetool 1.9.1 и runtime закреплены по SHA-256) |
| `android/sign.sh` | Переподпись APK ключом из секретов (apksigner, v1+v2+v3) или оставляет debug-подпись |
| `android/check-native-licences.sh` | Лицензионный гейт: FFmpeg в APK — только «LGPL 2.1/3 or later» без `--enable-gpl`/`--enable-nonfree`, libmpv — `-Dgpl=false`; иначе релиз падает |
| `android/lgpl-sources.sh` | Архив исходников LGPL-компонентов libmpv.so (см. ниже) |
| `THIRD-PARTY-NOTICES.md`, `fetch-licences.sh` | Уведомления и тексты лицензий (скачиваются из SPDX license-list-data по закреплённому тегу с проверкой SHA-256); кладутся в tar.gz, AppImage и в `kinolune-<v>-licenses.tar.gz` |

Версия Flutter закреплена в `env.FLUTTER_VERSION` обоих workflow (сейчас 3.50.0) — менять вместе.

## Артефакты релиза

`kinolune-<v>-x86_64.AppImage`, `kinolune-<v>-linux-x86_64.tar.gz`, `kinolune-<v>-android-{arm64-v8a,armeabi-v7a,x86_64,universal}.apk`, `kinolune-<v>-licenses.tar.gz`, `kinolune-<v>-android-lgpl-sources.tar`, `SHA256SUMS`. Заметки релиза: текст аннотированного тега + таблица загрузок, статус подписи, состав LGPL-компонентов и отчёт гейта.

- `versionCode` Android = номер запуска workflow (`github.run_number`), монотонно растёт; split-APK получают `+1000×ABI` (так делает Flutter). Обновляться стоит в пределах одного вида APK (split или universal).
- Версия с суффиксом (`0.1.0-rc.1`) публикуется как pre-release.

## Лицензии libmpv / FFmpeg (LGPL)

- **Linux.** media_kit на Linux линкуется с системным libmpv; в tar.gz и AppImage libmpv/FFmpeg **не входят** (пользователь ставит `libmpv2` / `mpv-libs` / `mpv`). Обязательств по LGPL-исходникам у Linux-артефактов нет. Встраивание собственной LGPL-сборки (tools/ffmpeg, PLAY-001) — задача PKG-002.
- **Android.** `media_kit_libs_android_video` кладёт в APK готовый `libmpv.so` из [libmpv-android-video-build](https://github.com/media-kit/libmpv-android-video-build) (mpv `-Dgpl=false` + FFmpeg `--disable-gpl --disable-nonfree --enable-version3` и зависимости статически). Проверено для 1.3.8 → сборка `v1.1.7`: **FFmpeg там LGPL-3.0-or-later** (из-за mbedTLS), а не 2.1, как в нашей `tools/ffmpeg`. Гейт это допускает (LGPL), GPL/nonfree — нет.
  `lgpl-sources.sh` находит тег сборки по `build.gradle` разрешённой версии пакета, скачивает репозиторий сборки на этом теге (скрипты, патчи, пины версий) и исходники mpv, FFmpeg и FriBidi на закреплённых версиях, и прикладывает архив к тому же релизу (LGPL-2.1 §6 / LGPL-3.0 §4: равный доступ к исходникам «из того же места»). Тот же подход, что `tools/ffmpeg/build.sh sources`.
- Тексты лицензий скачивает `fetch-licences.sh` (SPDX license-list-data, тег и SHA-256 закреплены) — в репозитории копий чужих лицензий нет. Флаттеровский `NOTICES` (движок и все Dart-пакеты) кладётся в Linux-архивы как `licenses/FLUTTER-NOTICES.txt`; в Android он доступен на экране лицензий приложения (`showLicensePage`). Приложению (R01-APP) стоит добавить в `LicenseRegistry` запись про libmpv/FFmpeg со ссылкой на релиз — это экран «О программе».

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
