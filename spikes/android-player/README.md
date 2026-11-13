# Спайк PLAY-AND-001: нативный плеер Media3 во Flutter-приложении (Android / Android TV) без потери HDR и AFR

> Задача: [`docs/ROADMAP.md`](../../docs/ROADMAP.md) → PLAY-AND-001. Решение — [ADR-0008](../../docs/adr/0008-android-player-embedding.md).
> Исследование: [02 §1.3, §2.2, §2.3, §2.7, §2.8](../../docs/research/02-client-stack-playback.md).
>
> Это **спайк**: код одноразовый, в продукт не переносится как есть. Рабочая реализация — PLAY-AND-002
> (`clients/flutter/packages/pa_player_android/`).

## Что сравниваем

Одно Flutter-приложение (`android_player_spike`), один и тот же ExoPlayer (`SpikePlayer.kt`: одинаковые рендереры,
AFR и метрики), четыре способа прикрепить видео к UI:

| | Вариант | Видео-поверхность | OSD | Ожидание по HDR/DV | AFR |
|---|---|---|---|---|---|
| **A** | `activity_native_osd` — отдельная `PlayerActivity` | `SurfaceView` в собственном окне | нативный (Media3 `PlayerView`) | ✅ эталон: видео — отдельный слой SurfaceFlinger, HWC отдаёт его на HDMI как есть | `preferredDisplayModeId` / `Surface.setFrameRate` |
| **B** | `activity_flutter_osd` — та же `PlayerActivity` | `SurfaceView` | Flutter: прозрачный `FlutterFragment` (RenderMode.texture), второй движок из `FlutterEngineGroup`, Dart-entrypoint `osdMain` | ✅ должно совпадать с A: OSD — обычный SDR-слой окна над видео, ровно как нативный контроллер | как в A |
| **C** | `platform_view_hc` — PlatformView, Hybrid Composition (`PlatformViewLink` + `initExpensiveAndroidView`) | `SurfaceView` внутри `FlutterActivity` | Flutter в том же дереве виджетов | ⚠️ вероятно, но не гарантировано: Flutter-слои над видео становятся `FlutterImageView`, HWC может уйти в GPU-композицию | как в A |
| **D** | `platform_view_tlhc` — `AndroidView` (Texture Layer Hybrid Composition) | `TextureView` → GL-текстура Flutter | Flutter | ❌ по построению: видео проходит через SDR-композитор Flutter; DV/HDR-режим HDMI не включится. Контрольный «негативный» вариант | только `preferredDisplayModeId` (у текстуры нет `Surface.setFrameRate`) |

AFR реализован тремя способами (переключатель на главном экране или `--es afr …`):

- `mode` — `WindowManager.LayoutParams.preferredDisplayModeId` с выбором режима в `AfrModeSelector` (как Kodi):
  разрешение не меняем, частота = частоте контента или наименьшему кратному, 23.976 ≠ 24.000, если текущий режим
  уже кратен — не переключаемся. Старт: `prepare()` на паузе → известна частота → переключение → ожидание
  `DisplayListener.onDisplayChanged` (тайм-аут 6 с) → опциональная пауза `settle_ms` → `play()`.
- `surface` — `Surface.setFrameRate(fps, FIXED_SOURCE, CHANGE_FRAME_RATE_ALWAYS)` (API 31+). Решение о переключении
  принимает система с учётом пользовательской настройки «Match content frame rate» (Android 12+).
- `off` — только встроенная стратегия Media3 `ONLY_IF_SEAMLESS` (меняет частоту лишь там, где это бесшовно).

Если контейнер не сообщает частоту (MKV без `DefaultDuration`, часть TS — `Format.frameRate == -1`), плеер
~1 с играет, оценивает частоту по временным меткам кадров (`FrameRateEstimator`), ставит паузу, переключает режим
и продолжает.

## Структура

```
lib/main.dart                  главный экран (URL, варианты, AFR, tunneling, settle) + entrypoint osdMain (вариант B)
lib/platform_view_player.dart  варианты C и D
lib/osd.dart                   общий Flutter-OSD: ±10 с, пауза, панель статистики (опрос `stats` раз в секунду)
android/app/src/main/kotlin/dev/projectalpha/spike/androidplayer/
  MainActivity.kt              FlutterActivity: канал pa_spike/main, фабрика PlatformView
  PlayerActivity.kt            варианты A и B
  PlayerPlatformView.kt        варианты C и D
  SpikePlayer.kt               ExoPlayer + старт с AFR + метрики
  AfrController.kt             переключение режима дисплея и Surface.setFrameRate
  AfrModeSelector.kt           выбор режима и оценка частоты (чистый Kotlin, покрыт JUnit)
  DeviceCapabilities.kt        HDR-типы дисплея, DV/HEVC/AV1-декодеры, passthrough-кодировки
  SpikeLog.kt                  метрики в logcat (тег PaSpike, по JSON на строку)
android/app/src/test/…         JUnit: AfrModeSelectorTest
test/widget_test.dart          widget-тесты главного экрана и OSD
tools/make-test-media.sh       генерация синтетических тестовых роликов (SDR/HDR10/HLG, 6 частот)
```

## Сборка

Нужны Flutter (stable, проверено на 3.50.0), Android SDK (platform 36, build-tools 36) и **JDK 17–21**
(у JRE без `javac` сборка падает с `does not provide the required capabilities: [JAVA_COMPILER]`).

```bash
flutter config --jdk-dir <путь-к-JDK-21>    # если системная Java — только JRE или не 17–21
cd spikes/android-player
flutter pub get
flutter analyze && flutter test              # Dart: анализ и widget-тесты
(cd android && ./gradlew :app:testDebugUnitTest)   # Kotlin: JUnit
flutter build apk --debug                    # build/app/outputs/flutter-apk/app-debug.apk
adb install -r build/app/outputs/flutter-apk/app-debug.apk
```

Приложение появляется и в лаунчере телефона, и в Android TV / Google TV (`LEANBACK_LAUNCHER`, баннер-заглушка).

Запуск вариантов A/B без Flutter-экрана (удобно для скриптов):

```bash
adb shell am start -n dev.projectalpha.spike.androidplayer/.PlayerActivity \
  --es url http://192.168.1.10:8000/hdr10-hevc-1080p-23.976.mkv \
  --es variant activity_flutter_osd --es afr mode --ez tunneling false --el settle_ms 0
adb logcat -s PaSpike
```

## Метрики (logcat, тег `PaSpike`)

Каждая строка — `METRIC {json}`; `t` — `elapsedRealtime` в мс.

| event | Что значит |
|---|---|
| `device` | модель, SDK, режимы дисплея, HDR-типы дисплея (`display_hdr_types`), декодеры DV/HEVC/AV1 с профилями, passthrough-кодировки |
| `open` | старт варианта (вариант, AFR, tunneling, URL) |
| `video_track` | выбранная видеодорожка: кодек, `codecs` (`dvhe.08.06` и т. п.), fps, `color_transfer` (6 = PQ, 7 = HLG) |
| `fps_unknown` / `fps_estimated` | контейнер без частоты → оценка по меткам кадров |
| `afr_request` / `afr_done` / `afr_skip` | запрошенный режим, `switch_ms` (время до `onDisplayChanged`), итог `switched`/`timeout` |
| `afr_surface` / `afr_surface_unsupported` | вызов `Surface.setFrameRate` (вариант `surface`) |
| `decoder` | имя декодера (`c2.*.dolby-vision.*`, `OMX.*.hevc.*` …) и время инициализации |
| `first_frame` / `playing` | время от `open` до первого кадра и до старта воспроизведения |
| `osd_first_frame` | вариант B: время от `onCreate` до первого кадра Flutter-OSD |
| `dropped_frames` | пропущенные кадры (порции и сумма) |
| `release` | итоговая статистика при выходе |

## Что удалось проверить в этой среде

Физического Android TV у агента не было. Проверено:

| Проверка | Результат |
|---|---|
| `flutter analyze` | без замечаний |
| `flutter test` (2 widget-теста) | проходят |
| `./gradlew :app:testDebugUnitTest` (JUnit `AfrModeSelectorTest`) | все тесты проходят |
| `flutter build apk --debug` | APK собирается (Flutter 3.50.0, AGP 9.3.1, Gradle 9.5.0, Media3 1.11.1, JDK 21) |
| Эмулятор Android TV 14 (x86, образ `android-tv`, swiftshader, без окна) | см. ниже |

Эмулятор (числа **не характеризуют реальное железо**: ARM-код Flutter исполняется через трансляцию в x86, графика —
программная, видео — софтверный `c2.goldfish.*`):

- все четыре варианта собираются и воспроизводят SDR H.264 (MP4/MKV); Flutter-OSD (B, C, D) управляется с D-pad;
- `first_frame` от `open()`: A — 0,8–2,2 с, B — 13–14,6 с (старт второго Flutter-движка блокирует главный поток),
  C — 6,1 с, D — 2,2 с; пропуски кадров на одном ролике: C — 110, D — 1 за ~25 с;
- HEVC Main10 (HDR10) эмулятор не декодирует (`c2.goldfish.hevc.decoder`), поэтому HDR здесь не проверить вообще;
- у эмулятора один режим дисплея (1920x1080@60) — AFR доходит до `afr_skip`/`afr_surface`, но не переключает;
- частота кадров: MKV → `-1` (оценка по меткам дала верные 29.97), MP4 25 fps → Media3 сообщает 24.900398
  (исправлено «прилипанием» к стандартной частоте);
- на снимках экрана эмулятора (swiftshader без окна) Flutter-главный экран чёрный, хотя работает (ввод
  обрабатывается); экраны с PlatformView и вариант B снимаются нормально — артефакт эмулятора.

Подробная таблица — в [ADR-0008, «Замеры»](../../docs/adr/0008-android-player-embedding.md#замеры).

## Что НЕ проверено (нужен реальный TV)

- Включается ли на HDMI режим **HDR10 / HLG / Dolby Vision** в каждом из вариантов A–D (главный вопрос спайка).
- Не мешает ли Flutter-OSD (вариант B) и Flutter-слои (вариант C) HDR-слою видео на конкретных SoC (Tegra X1+,
  Amlogic S905X4/S905X5M, MediaTek MT8696), нет ли «серого/выцветшего» UI поверх HDR и наоборот.
- Реальное время переключения режима (`afr_done.switch_ms`) и поведение TV во время ресинхронизации HDMI.
- Работа `Surface.setFrameRate(…, CHANGE_FRAME_RATE_ALWAYS)` на Google TV с разными значениями системной
  настройки «Match content frame rate» (Seamless only / Always / Never).
- DV Profile 7 (FEL/MEL) на Shield, DV P5/P8.1 на Chromecast with Google TV / Google TV Streamer, tunneling.
- Производительность варианта C (Hybrid Composition до Android 10 сливает потоки platform и raster).
- Passthrough-аудио в этот спайк не входит (PLAY-AND-004), но `device.audio_passthrough` логируется.

## Ручной тест-план (для владельца или контрибьютора с железом)

### Оборудование

| Устройство | Почему | Android |
|---|---|---|
| NVIDIA SHIELD TV / TV Pro (2019) | эталон Android TV: DV P5/P8, P7 FEL/MEL, AFR через `preferredDisplayModeId` | 11 |
| Chromecast with Google TV 4K **или** Google TV Streamer | массовый Google TV с DV, Android 12+ (`Surface.setFrameRate`, «Match content frame rate») | 12 / 14 |
| Любая приставка на Amlogic S905X4 (опционально) | частые ошибки HDR-композиции и AFR у Amlogic | 11+ |
| Телевизор с HDR10 + Dolby Vision, HDMI 2.0+, с индикацией режима (инфо-баннер / сервисное меню) | видеть фактический сигнал | — |
| Опционально: HDMI-анализатор или AVR с отображением входного сигнала | точные Hz/HDR без «на глаз» | — |

На приставке: **отключить** системный «Match content frame rate» для проверки `mode` и включить (Always / Seamless only)
для проверки `surface`; разрешение вывода 4K 60 Hz; формат «HDR/DV — по контенту» (на Shield: *Match content
dynamic range*; на Google TV: *Dynamic range format = по умолчанию/Match*).

### Тестовые файлы

1. Синтетические: `tools/make-test-media.sh test-media 60` → SDR H.264 и HDR10 HEVC Main10 на 23.976/24/25/29.97/50/59.94
   и HLG 50; раздать по LAN: `python3 -m http.server 8000 --directory test-media`.
2. Реальные HDR10, HDR10+, DV P5, P8.1 и P7 (MKV и MP4) — собственные рипы/демо, которые есть у тестировщика.
   Публичные демо-ролики перечислены, например, на странице Kodi wiki «Samples» **[проверить лицензии перед
   использованием; в репозиторий не добавлять]**.
3. DV без исходников: из HDR10-ролика из п. 1 сделать P8.1 через `dovi_tool generate` + `inject-rpu`
   (MIT) и смуксить `mkvmerge` **[проверить: синтетический RPU включает DV-режим, но картинку не оценивает]**.
4. MKV без `DefaultDuration` (проверка оценки частоты): например, ролик по умолчанию из приложения
   (публичный тест-медиа Media3, 29.97 fps, H.264).

### Шаги (для каждого устройства × вариант A, B, C, D)

1. `adb logcat -c && adb logcat -s PaSpike > <device>-<variant>.log`, на главном экране нажать **Device info** —
   сохранить `device` (HDR-типы дисплея, DV-декодеры, режимы).
2. HDR10 23.976 MKV, AFR = `mode`. Наблюдать: TV показывает **HDR10** и **23.976/24p**; `afr_done.result=switched`,
   записать `switch_ms`, время чёрного экрана по секундомеру/видео; `first_frame.since_open_ms`.
3. Тот же файл, AFR = `surface` при системной настройке Always и Seamless only. Записать, переключился ли режим.
4. DV P8.1 MP4 и MKV: TV показывает **Dolby Vision**; в `decoder` — DV-декодер (или HEVC-fallback на не-DV
   устройствах — тогда должен быть HDR10). P5: только DV, цвета без зелёно-фиолетового сдвига. P7 на Shield.
5. HLG 50 и SDR 25/50/59.94: правильная частота, SDR-контент не включает HDR-режим.
6. В каждом варианте открыть OSD/статистику и подержать на экране 30 с: **HDR-режим TV не должен выключаться**,
   UI не должен «выцветать» или становиться пересвеченным, видео не должно мигать.
7. Перемотка ±10 с ×10, пауза/продолжение, выход назад: режим дисплея возвращается к исходному
   (после выхода из варианта C/D — сразу, т. к. окно FlutterActivity остаётся).
8. 10 минут воспроизведения 4K HDR10 23.976: `release.dropped_frames`; для B и C дополнительно включить
   `adb shell dumpsys gfxinfo dev.projectalpha.spike.androidplayer` до/после.
9. Повторить шаг 2 с `tunneling=true` (Shield, Chromecast): режимы и пропуски кадров.
10. Пульт: навигация D-pad по OSD в вариантах B и C (фокус виден, OK нажимает, Back выходит).

### Таблица результатов (заполнить и приложить к ADR-0008)

| Устройство | Вариант | Файл | HDR-режим на TV | Частота на TV | `switch_ms` | `first_frame` мс | Пропуски / 10 мин | OSD не ломает HDR | Заметки |
|---|---|---|---|---|---|---|---|---|---|
| | | | | | | | | | |

## Выводы спайка

Сводно — в [ADR-0008](../../docs/adr/0008-android-player-embedding.md). Коротко:

1. **Рекомендуемый вариант — B**: отдельная нативная `PlayerActivity` с `SurfaceView` (Media3), OSD на Flutter
   в прозрачном `FlutterFragment` над видео, второй движок — из `FlutterEngineGroup`. С точки зрения SurfaceFlinger/HWC
   это та же композиция, что и у нативного контроллера Media3 (SDR-слой UI над HDR-слоем видео), поэтому HDR/DV и
   AFR должны вести себя как в эталонном варианте A, а UI OSD остаётся общим с остальными платформами.
   Запасной путь — вариант A (нативный OSD), если на каком-то SoC Flutter-слой над видео ломает HDR.
2. **D (TLHC/Texture) отвергнут**: видео идёт через SDR-композитор Flutter, `Surface.setFrameRate` недоступен.
3. **C (Hybrid Composition) не выбран**: технически возможен, но зависит от того, как HWC справится со
   слоями `FlutterImageView` над HDR-видео, хуже по производительности UI и сложнее по фокусу/жизненному циклу;
   при этом не даёт ничего, чего нет у B.
4. **AFR**: `preferredDisplayModeId` (API 23+, обязателен для Android TV 9–11 / Shield) и `Surface.setFrameRate
   CHANGE_FRAME_RATE_ALWAYS` (API 31+, уважает системную настройку) — оба оставить, по умолчанию `surface` на API 31+
   и `mode` ниже; проверить на железе.
5. Частота кадров из контейнера бывает неизвестна (`-1`) даже у обычных MKV — оценка по меткам кадров обязательна.
6. Наблюдения про Flutter 3.50: `FlutterLoader` нужно инициализировать до `findAppBundlePath()` (создать
   `FlutterEngineGroup` раньше), иначе `PlayerActivity`, запущенная не из Flutter, падает с NPE; Flutter
   предупреждает, что в release-сборках внешние intent'ы в `FlutterActivity` будут игнорироваться — плееру это
   не мешает (`PlayerActivity` не `FlutterActivity`), но deep-link'и в приложение надо будет оформлять отдельно.
