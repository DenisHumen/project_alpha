<!-- SPDX-License-Identifier: MPL-2.0 -->

# media_kit_libs_android_video (локальная замена, PLAY-013)

Замена пакета [`media_kit_libs_android_video`](https://pub.dev/packages/media_kit_libs_android_video) 1.3.8
для приложения Kinolune. Подключается в `clients/flutter/app/pubspec.yaml` через `dependency_overrides`.

Зачем: upstream при сборке скачивает jar-файлы с готовым `libmpv.so`, в который статически вшит
FFmpeg под **LGPL v3**. Мы кладём в APK свою сборку из [`tools/mpv`](../../../../tools/mpv/README.md):
mpv `-Dgpl=false` и FFmpeg **LGPL v2.1+** отдельными `.so`, с открытым рецептом и исходниками.

Что осталось от upstream (MIT, © Hitesh Kumar Saini, см. `LICENSE`): Java-обвязка без изменений —
`MediaKitLibsAndroidVideoPlugin` (регистрация плагина, `System.loadLibrary("mpv")`) и
`MediaKitAndroidHelper` (JNI-хелпер, его `libmediakitandroidhelper.so` тоже собирает `tools/mpv`),
`settings.gradle`, `AndroidManifest.xml`. Наш — только `android/build.gradle`: задачи
`downloadDependencies` и зависимости от jar больше нет.

## Наполнение jniLibs

`android/src/main/jniLibs/` не хранится в git. Проще всего — одной командой для обеих библиотек
(libmpv и libsmb2, CI-артефакты или локальная сборка, см. `clients/flutter/AGENTS.md`):

```bash
clients/flutter/app/tool/native_libs.sh fetch   # или build
```

Вручную, перед сборкой APK:

```bash
tools/mpv/bundle.sh android \
  clients/flutter/packages/media_kit_libs_android_video/android/src/main/jniLibs \
  <libmpv-android-arm64-v8a> <libmpv-android-armeabi-v7a> <libmpv-android-x86_64>
```

Аргументы — каталоги `tools/mpv/out/android-<abi>` (локальная сборка `tools/mpv/build.sh` или
`docker-build.sh`) или распакованные CI-артефакты `libmpv-android-<abi>` (`.github/workflows/libmpv.yml`).
`bundle.sh` сначала прогоняет `check-android` (SONAME без версий, зависимости, выравнивание 16 КБ).

В каждом `jniLibs/<abi>/` — ровно набор `libmpv.so`, `libavcodec.so`, `libavfilter.so`,
`libavformat.so`, `libavutil.so`, `libswresample.so`, `libswscale.so`, `libmediakitandroidhelper.so`
(плюс `libavdevice.so`, которую ставит сборка FFmpeg; libmpv её не использует).

Рядом лежит `libsmb2.so` — LGPL-клиент SMB из [`tools/libsmb2`](../../../../tools/libsmb2/README.md)
(LIB-019): Rust-ядро загружает его в рантайме для SMB-источников (ADR-0033). Кладётся так же:

```bash
tools/libsmb2/bundle.sh android \
  clients/flutter/packages/media_kit_libs_android_video/android/src/main/jniLibs \
  <libsmb2-android-arm64-v8a> <libsmb2-android-armeabi-v7a> <libsmb2-android-x86_64>
```

- **release**: сборка падает, если ни одного ABI нет или набор (вместе с `libsmb2.so`) неполный.
  Обойти (например, для сборки без плеера): `flutter build apk --release -Pkinolune.allowMissingLibmpv=true`.
- **debug**: только предупреждение — APK соберётся, но без `libmpv.so` приложение закроется при
  старте, без `libsmb2.so` не откроются SMB-источники.

Релиз (`.github/workflows/release-app.yml`) собирает libmpv через `libmpv.yml` и libsmb2 через
`libsmb2.yml`, наполняет jniLibs и проверяет готовые APK гейтом
`packaging/app/android/check-native-licences.sh`: в APK должен быть только наш набор, FFmpeg —
`LGPL version 2.1 or later` отдельными библиотеками, libmpv — `-Dgpl=false`, libsmb2 — только
отдельной `libsmb2.so` в каждом ABI.

## Обновление upstream

При обновлении media_kit сравнить Java-файлы и манифест с новой версией пакета на pub.dev
и перенести изменения; `build.gradle` — только `compileSdkVersion` и версию AGP.
