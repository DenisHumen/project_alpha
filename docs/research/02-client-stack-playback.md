# 02. Клиентский стек и движок воспроизведения

> Исследование для project_alpha: кроссплатформенный видеоплеер уровня Infuse и музыкальный плеер уровня Spotify для локальных и сетевых медиатек.
> Дата: 2026-11-13. Автор: исследователь №2 (клиентский стек и воспроизведение).
> Целевые платформы: Linux, macOS, Windows, iOS/iPadOS, Android, Android TV, Apple TV (tvOS).
> Команда: один разработчик и AI-агенты.

---

## 0. TL;DR

1. **Общее ядро на Rust**: сеть (SMB/NFS/WebDAV/FTP), клиенты Jellyfin/Plex/Emby, метаданные, SQLite, синхронизация, абстракция «источник байтов» для плееров. Привязки: UniFFI для Swift/Kotlin, `flutter_rust_bridge` для Dart.
2. **UI**: Flutter на Android, Android TV, iOS/iPadOS, Windows, Linux и macOS. **Отдельный нативный SwiftUI-клиент для tvOS**, который позже станет основой «Apple TV-стиля» и, возможно, Apple-версии в целом. Официальной поддержки tvOS нет ни у Flutter, ни у Compose Multiplatform: есть только сторонние форки.
3. **Экран плеера нативный на каждой платформе**, а не Flutter-текстура: иначе теряются HDR/DV, переключение частоты кадров и passthrough.
4. **Движки воспроизведения** — гибрид по образцу Infuse:
   - Android и Android TV: Media3/ExoPlayer, libmpv как запасной вариант.
   - Apple: своя связка FFmpeg-демуксер → VideoToolbox → `AVSampleBufferDisplayLayer` для HDR/DV. На первом этапе MPVKit и AVPlayer.
   - Desktop: libmpv (gpu-next/libplacebo).
5. **Лицензии**: только LGPL-сборки FFmpeg/mpv (без `--enable-gpl` и `--enable-nonfree`), динамическая линковка, публикация исходников и инструкций по перелинковке. Никаких GPL-компонентов (x264/x265, KSPlayer-GPL и т. п.) в бинарниках для App Store.
6. **Публикация**: сначала бесплатные каналы (GitHub Releases, Flathub, AUR, winget, Microsoft Store — для физлиц регистрация теперь бесплатная), затем Google Play ($25), затем Apple Developer Program ($99/год) одновременно для iOS, tvOS и macOS.

---

## 1. Кроссплатформенный UI: сравнение подходов

### 1.1 Ключевые требования

- 6 платформ, из них две «телевизионные» (Android TV и tvOS) с фокус-навигацией пультом (D-pad, Siri Remote).
- Богатая медиатека: сетки постеров, «полки», параллакс, фоновые арты, плавный скролл тысяч элементов.
- Видеоповерхность, которая не ломает HDR и Dolby Vision, а также переключение режима дисплея.
- Один разработчик: минимум отдельных UI-кодовых баз, хорошая «знакомость» стека для AI-агентов.

### 1.2 Сводная таблица

| Подход | Linux | Win | macOS | iOS/iPadOS | Android | Android TV | tvOS | Фокус/TV-навигация | Встраивание HDR-видео | Вердикт |
|---|---|---|---|---|---|---|---|---|---|---|
| **Flutter** | ✅ стабильно | ✅ | ✅ | ✅ | ✅ | ✅ (Focus/Shortcuts, D-pad) | ⚠️ только сторонний embedder [flutter-tvos](https://pub.dev/packages/flutter_tvos) | Хорошая на Android TV, на tvOS через сторонний embedder | ⚠️ Texture-путь в Flutter SDR; для HDR нужен нативный view или отдельный нативный экран | **Рекомендуется для 6 из 7 таргетов** |
| **KMP + Compose Multiplatform** | ✅ (JVM desktop) | ✅ | ✅ | ✅ (стабилен с CMP 1.8) | ✅ | ✅ лучший в классе ([Compose for TV](https://developer.android.com/training/tv/playback/compose)) | ⚠️ только неофициальный форк/плагин [`dev.sajidali.compose-tvos`](https://plugins.gradle.org/plugin/dev.sajidali.compose-tvos) | Отличная на Android TV | ⚠️ На desktop видео через SwingPanel/heavyweight — неудобно | Сильная альтернатива, особенно если Android TV — приоритет №1 |
| **SwiftUI (Apple) + X для остальных** | — | — | ✅ | ✅ | — | — | ✅ нативно, эталонный focus engine | Эталон на tvOS | ✅ полностью нативно | **SwiftUI — для tvOS обязательно** |
| **React Native (+ react-native-tvos)** | ❌ нет официального | ✅ (react-native-windows) | ✅ (react-native-macos) | ✅ | ✅ | ✅ | ✅ [react-native-tvos](https://github.com/react-native-tvos/react-native-tvos) (0.86.x) | Очень хорошая: нативный focus engine | ✅ нативные view (react-native-video и др.) | Хорош для TV, но нет Linux и три «out-of-tree» платформы |
| **Tauri / Electron** | ✅ | ✅ | ✅ | Tauri 2: ✅ (без TV) | Tauri 2: ✅ | ❌ | ❌ | Только веб-фокус | ❌ WebView не умеет MKV/HDR/passthrough, нужен внешний mpv-оконный хак | Не подходит |
| **Qt/QML** | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ можно, но вручную | ❌ | Ручная | ✅ (QtMultimedia/встраивание mpv) | Лицензия LGPLv3/коммерческая, нет tvOS, мало AI-знаний. Не рекомендуется |
| **Avalonia** | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ❌ | Ручная | ⚠️ | Нет tvOS |
| **.NET MAUI** | ❌ | ✅ | ✅ (Catalyst) | ✅ | ✅ | ⚠️ | ❌ | Ручная | ⚠️ | Нет Linux и tvOS. Не подходит |

Источники по статусу tvOS:
- Flutter: официальной поддержки Apple TV нет, есть сторонний embedder [fluttertv.dev / flutter_tvos](https://pub.dev/packages/flutter_tvos) с набором плагинов ([fluttertv/plugins](https://github.com/fluttertv/plugins)). Официально Flutter приходит на LG webOS ([LG](https://webostv.developer.lge.com/news/flutter-for-webos-tv-is-now-available-for-developers)) и Samsung Tizen ([Samsung](https://developer.samsung.com/smarttv/develop/native/flutter.html)), но не на tvOS.
- Compose Multiplatform: JetBrains не публикует tvOS-артефакты UI-модулей. Есть энтузиастский форк и Gradle-плагин ([proandroiddev](https://proandroiddev.com/my-journey-making-compose-multiplatform-work-on-tvos-and-what-i-learned-28744910489e), [klibs](https://klibs.io/project/sajidalidev/compose-tvos)).
- React Native: core работает только на New Architecture начиная с 0.82 ([reactnative.dev](https://reactnative.dev/blog/2025/10/08/react-native-0.82.md)), версии react-native-tvos идут вслед за core ([README 0.86](https://unpkg.com/react-native-tvos@0.86.0-2/README.md)).

### 1.3 Разбор кандидатов

**Flutter.**
- Плюсы:
  - Единственный зрелый фреймворк, который официально и стабильно покрывает все три десктопа (включая Linux) и обе мобильные ОС.
  - Отличный рендеринг сеток и анимаций, Impeller на iOS/Android.
  - Огромная база кода в обучающих данных моделей, то есть AI-агенты пишут на Dart/Flutter уверенно.
  - Android TV реализуем через `Focus`, `FocusTraversalGroup` и `Shortcuts`. Есть готовые пакеты TV-навигации, но «фокусную» дисциплину придётся выстроить самим.
- Минусы:
  - tvOS только через сторонний embedder. Ставить на него весь Apple TV-продукт рискованно: зависимость от одного мейнтейнера и отставание от версий Flutter.
  - Видео через `Texture` композится в SDR-поверхность Flutter. HDR10/DV, AFR и HDMI-режимы на этом пути не работают. Решение: плеер — отдельный нативный экран (Activity / UIViewController / нативное окно) или PlatformView с `SurfaceView`.
  - Нативный «look» Apple (Liquid Glass и т. п.) в Flutter только эмулируется.

**KMP + Compose Multiplatform.**
- Плюсы:
  - Лучшая TV-история на Android (Compose for TV, `tv-material`) и нативная интеграция с Media3.
  - Compose на iOS стабилен с мая 2025.
  - Kotlin/Native-ядро нормально интегрируется со Swift.
- Минусы:
  - Desktop на JVM (вес, старт, встраивание видео).
  - tvOS только через неофициальный форк.
  - Linux-десктоп заметно менее отполирован, чем у Flutter.
  - Для «музыкального плеера с крутым UI» на 3 десктопах Flutter практичнее.
- KMP имеет смысл как ядро, если не Rust (см. §1.4).

**Нативный SwiftUI для Apple.** Для tvOS это единственный путь без риска: focus engine, `TVTopShelf`, `AVDisplayCriteria`, параллакс-постеры, системные паттерны. Для одного разработчика держать SwiftUI на всех четырёх Apple-платформах и параллельно второй UI — дорого. Поэтому SwiftUI только для tvOS на старте, с опцией позже расширить его на iOS и macOS.

**React Native.** Технически хорош для TV (react-native-tvos активно поддерживается и даёт настоящий нативный фокус tvOS), но:
- нет Linux;
- Windows и macOS — отдельные проекты Microsoft со своими релизными циклами;
- три out-of-tree платформы для одного человека — лишняя связка зависимостей.

Рассматривать только если бы Linux не был в списке.

**Tauri/Electron.** WebView не умеет воспроизводить MKV/HEVC/TrueHD/PGS, HDR и passthrough. Пришлось бы держать mpv в отдельном окне поверх WebView. Для медиаплеера премиум-класса это тупик. Tauri 2 поддерживает iOS/Android, но не TV.

**Qt/QML, Avalonia, MAUI.** Нет tvOS. У Qt дорогая коммерческая лицензия либо LGPLv3 со сложностями для App Store. У MAUI нет Linux. Avalonia хороша на desktop, но TV и медиа-экосистема слабые.

### 1.4 Стратегия общего ядра

| Критерий | **Rust core** | KMP core | C++ core |
|---|---|---|---|
| Привязки к Swift | UniFFI (Mozilla) — зрелые | Kotlin/Native → ObjC/Swift export | ручной C API |
| Привязки к Kotlin (Android) | UniFFI | нативно | JNI |
| Привязки к Dart/Flutter | `flutter_rust_bridge` — зрелые | нет (только через C) | `ffigen` |
| Сетевые протоколы | crates + FFI к libsmb2/libnfs | только FFI к C-библиотекам или JVM-библиотеки (не для iOS) | нативно |
| Работа с FFmpeg | `ffmpeg-next`/`rsmpeg`, кросс-компиляция через cargo | неудобно | нативно |
| Нейтральность к UI | полная | тянет в Compose | полная |
| Безопасность памяти и знание AI-агентами | высокие | высокие | ниже |

**Решение: Rust-ядро.** Оно одинаково хорошо обслуживает Flutter (FRB), SwiftUI (UniFFI → Swift Package / XCFramework) и нативный Android-код плеера (UniFFI → Kotlin).

Состав ядра:

- `net`: SMB2/3, NFSv3/v4, WebDAV, FTP/SFTP, HTTP; discovery (mDNS/Bonjour, WS-Discovery, NetBIOS).
- `servers`: клиенты Jellyfin/Emby (OpenAPI), Plex, будущий собственный сервер.
- `library`: сканер, парсинг имён файлов, TMDB/TVDB/MusicBrainz (если не на сервере), SQLite (rusqlite/sqlx), FTS5-поиск, прогресс просмотра, синхронизация.
- `io`: **абстракция `ByteSource` (read/seek/size)** с адаптерами:
  - FFmpeg `AVIOContext`;
  - mpv `mpv_stream_cb_add_ro`;
  - Media3 `DataSource` (через JNI/UniFFI);
  - loopback HTTP range-сервер как универсальный fallback для AVPlayer/ExoPlayer.
- `audio` (опционально, этап 2): декодирование через FFmpeg и гапless-движок для музыки (см. §5).

---

## 2. Движки воспроизведения

### 2.1 Что использует Infuse

Infuse (Firecore) построен на **собственном движке поверх FFmpeg**:
- демуксинг и декодирование «всего» в софте;
- аппаратный декод HEVC/H.264/AV1 через VideoToolbox там, где он доступен;
- вывод через системные слои Apple, чтобы получить нативный HDR10/Dolby Vision и переключение режимов Apple TV;
- собственный рендер субтитров (ASS, PGS).

Ограничения, которые видны по форуму Firecore:
- DV Profile 7 FEL нативно не воспроизводится: показывается как HDR10 или требует конвертации в P8.1 ([тред P7 FEL](https://community.firecore.com/t/technical-feasibility-native-p7-fel-playback-via-in-app-dual-layer-composition/60611), [большой тред P7/P8](https://community.firecore.com/t/dolby-vision-profile-7-8-support-ts-mkv-files/19713)).
- На Apple TV нет bitstream-passthrough TrueHD и DTS-HD. Infuse декодирует их сам в многоканальный LPCM, теряя объектную часть Atmos и DTS:X. Atmos доходит до ресивера только в E-AC-3 JOC ([тред](https://community.firecore.com/t/dolby-atmos-and-dtsx-not-supported-in-infuse/19820), [tvOS 11.3](https://community.firecore.com/t/passthrough-not-available-in-tvos-11-3-solution-use-auto-instead/16025)).

Вывод: **конкурентный уровень на Apple = гибридный движок** (FFmpeg-демукс + аппаратный декод + системный вывод), а не «чистый» mpv/VLC.

### 2.2 Сравнение движков

| Движок | Платформы | Форматы | HDR10 / HDR10+ / DV | Passthrough | AFR / HDMI-режим | Субтитры | Лицензия | Комментарий |
|---|---|---|---|---|---|---|---|---|
| **libmpv** (mpv 0.41, gpu-next по умолчанию — [релиз](https://linuxiac.com/mpv-0-41-media-player-switches-to-gpu-next-renderer/)) | Linux, Win, macOS, Android, iOS/tvOS (через MPVKit) | всё, что умеет FFmpeg | Тон-маппинг через libplacebo (включая DV P5 reshaping через libdovi). HDR-вывод: Win (d3d11), Linux (Wayland color-management), macOS частично. **На Apple TV настоящего DV нет** ([Emby форум](https://emby.media/community/topic/149237-whats-the-current-state-of-the-mvp-vs-native-player-in-the-appletv-client/)) | Desktop: да (`audio-spdif=ac3,eac3,dts,dts-hd,truehd`), Android: ограниченно, tvOS: нет | Desktop/Android — вручную, tvOS — вручную через `AVDisplayManager` | Лучшие: libass (ASS/SSA), PGS, VobSub, DVB | LGPLv2.1+ при `-Dgpl=false` | Эталон для desktop |
| **MPVKit** ([mpvkit/MPVKit](https://github.com/mpvkit/MPVKit)) | iOS, iPadOS, tvOS, macOS, visionOS | как mpv | тон-маппинг в SDR/PQ, без DV-пайплайна Apple | LPCM многоканал | вручную | как mpv | LGPLv3 (сборки `-GPL` — GPLv3) | Быстрый старт на Apple. Реальные приложения (Nuvio, VortX) используют его как fallback, а DV отдают в AVPlayer ([Nuvio](https://newreleases.io/project/github/bobsupra/NuvioTVOS/release/tvos-beta-3.1), [VortX](https://newreleases.io/project/github/VortXTV/VortX/release/v0.3.11-b163)) |
| **libVLC / VLCKit 4** | все 7, включая tvOS | всё | HDR → тон-маппинг, без DV | desktop/Android — да | частично | хорошие (libass) | LGPLv2.1+ | Зрелый, тяжёлый, менее гибкий рендер, чем mpv. Запасной кандидат |
| **media_kit** (Flutter-обёртка libmpv) | Win, Linux, macOS, iOS, Android | как mpv | Texture-путь Flutter → только SDR | нет | нет | как mpv | MIT + LGPL-библиотеки | Апстрим малоактивен (последний апдейт ~12.2025, [depscope](https://depscope.dev/pkg/pub/media_kit)). Альтернативы: [mpv_kit](https://pub.dev/documentation/mpv_kit/latest/), [mpv_audio_kit](https://pub.dev/packages/mpv_audio_kit/versions/0.4.2/changelog). Подходит для музыки и превью, не для HDR-плеера |
| **Media3 / ExoPlayer** | Android, Android TV, Google TV | MP4, MKV, TS, WebM, HLS, DASH; аудио-кодеки через FFmpeg extension | **HDR10, HDR10+, HLG, DV (P5/P8 на устройствах с DV-декодером, P7 на части устройств, например Shield)** — нативно через MediaCodec | **Да**: AC-3/E-AC-3/JOC, TrueHD, DTS, DTS-HD через `AudioTrack` (зависит от устройства и HDMI) | `Surface.setFrameRate` / `setVideoChangeFrameRateStrategy`; смена разрешения через `preferredDisplayModeId` | SRT, SSA (упрощённо), WebVTT, TTML, PGS, VobSub, DVB; полноценный ASS — через libass-оверлей | Apache 2.0 (+ LGPL FFmpeg ext) | **Основной движок для Android и Android TV** |
| **AVPlayer / AVFoundation** | iOS, tvOS, macOS | MP4/MOV/HLS (не MKV), кодеки Apple | **Полный нативный HDR10, HLG, DV P5/P8.1** (P7 нет); HDR10+ на новых устройствах | Только E-AC-3 (включая Atmos JOC) и многоканальный LPCM; TrueHD/DTS — нет | Автоматически (Match Content) | Только WebVTT/CEA-608/tx3g → ASS/PGS требуют своего оверлея | системный | Использовать для HLS/fMP4 от сервера (Jellyfin remux) |
| **Своя связка FFmpeg → VideoToolbox → `AVSampleBufferDisplayLayer` + `AVSampleBufferAudioRenderer`** («путь Infuse») | iOS, tvOS, macOS | всё (MKV и пр.), софт-декод для экзотики | **Нативный HDR10 / DV P5 / P8.1** (CMFormatDescription с `dvcC`/`dvvC`), P7 → BL+RPU как P8.1 (без FEL) | LPCM 7.1 + E-AC-3 JOC (проверить на `AVSampleBufferAudioRenderer`) | `AVDisplayCriteria(refreshRate:formatDescription:)` (tvOS 17+) | свой рендер: libass для ASS, FFmpeg-битмапы для PGS/VobSub | LGPL (FFmpeg) | **Целевой движок для Apple**, 2–4 месяца работы |

### 2.3 HDR и Dolby Vision по платформам

**Профили DV (кратко):**
- **P5**: одиночный слой IPTPQc2, без HDR10-совместимости. На не-DV дисплее без RPU-reshaping цвета «уходят в зелёно-фиолетовые».
- **P7**: двухслойный (BL + EL MEL/FEL), Blu-ray UHD.
- **P8.1**: HDR10-совместимая база и RPU. Самый «дружелюбный» профиль.
- **P8.4**: HLG-база (iPhone-видео).

| Платформа | HDR10 | HDR10+ | DV P5 | DV P8.1 | DV P7 | Тон-маппинг в SDR |
|---|---|---|---|---|---|---|
| **tvOS** | нативно | только на поддерживающих TV | нативно через AVFoundation/AVSBDL | нативно (MP4/HLS сразу; MKV — через свой демукс → `CMSampleBuffer` с DV-конфигом) | ❌ нет нативно → играть BL+RPU как P8.1 (MEL почти без потерь, FEL с потерей) | системный (при SDR-выходе) |
| **iOS/iPadOS** | нативно (EDR) | частично | нативно | нативно | ❌ (как tvOS) | системный |
| **macOS** | EDR на XDR/HDR-дисплеях через AVSBDL | частично | через AVFoundation | через AVFoundation | ❌ | mpv/libplacebo или системный |
| **Android TV** | MediaCodec | при поддержке SoC/TV | на DV-устройствах (Shield, Chromecast GTV, многие TV) | там же | частично: Shield Pro (MEL/FEL с оговорками), большинство — только BL | при SDR-выходе — устройство или свой GL-шейдер |
| **Android (телефоны)** | MediaCodec | Samsung/Pixel | на DV-телефонах | там же | ❌ | — |
| **Windows** | mpv d3d11 + `target-colorspace-hint`; Windows HDR mode | метаданные | **только тон-маппинг** libplacebo (RPU reshaping + libdovi) | тон-маппинг/HDR10-база | BL+RPU, тон-маппинг | libplacebo — лучший в классе |
| **Linux** | mpv gpu-next на Wayland с color-management (KDE Plasma 6, GNOME 48+) | метаданные | тон-маппинг libplacebo | — | — | libplacebo |

Практика:
- Desktop и все не-DV пути: **libplacebo** (через mpv gpu-next) с RPU через libdovi. Это самый качественный открытый тон-маппинг ([libplacebo](https://www-test.videolan.org/projects/libplacebo/)).
- Android и Android TV: доверять MediaCodec и Media3, на HDMI выдавать «как есть».
- Apple: только через системный слой. Любой Metal/OpenGL-рендер (mpv) означает отсутствие DV-режима на TV.
- **Конвертация P7 → P8.1 на лету** (отбросить EL, переписать RPU через логику `dovi_tool`/libdovi) — уже реализуемая фича, которой просят пользователи Infuse ([тред](https://community.firecore.com/t/support-dolby-vision-p7-automatic-conversion-to-p8/54099?page=2)). Конкурентное преимущество.

### 2.4 Аудио passthrough

| Платформа | AC-3 / E-AC-3 | E-AC-3 + Atmos (JOC) | TrueHD (+Atmos) | DTS core | DTS-HD MA / DTS:X | Многоканальный LPCM |
|---|---|---|---|---|---|---|
| **tvOS** | декод → LPCM или DD+ через AVFoundation | ✅ только через нативный путь Apple | ❌ bitstream; декод FFmpeg → LPCM 7.1, Atmos теряется | ❌ (декод → LPCM) | ❌ (декод XLL → LPCM 7.1, DTS:X-объекты теряются) | ✅ 7.1 |
| **iOS** | то же, в основном стерео/AirPlay | ✅ (AirPlay/наушники spatial) | декод | декод | декод | ограниченно |
| **macOS** | CoreAudio exclusive (hog) + S/PDIF: AC-3/DTS | ❌ | ❌ | ✅ (S/PDIF/HDMI) | ❌ | ✅ |
| **Android TV** | ✅ `AudioTrack` ENCODING_AC3/E_AC3 | ✅ ENCODING_E_AC3_JOC | ✅ ENCODING_DOLBY_TRUEHD (Shield и часть боксов) | ✅ | ✅ ENCODING_DTS_HD (зависит от устройства) | ✅ |
| **Windows** | ✅ WASAPI exclusive + IEC 61937 (mpv `audio-spdif`) | ✅ | ✅ (HDMI) | ✅ | ✅ | ✅ |
| **Linux** | ✅ ALSA `hdmi:`/`iec958:` (mpv `audio-spdif`), PipeWire passthrough | ✅ | ✅ | ✅ | ✅ | ✅ |

Вывод: на Apple TV — **только качественный внутренний декод** (FFmpeg TrueHD/DTS/DTS-HD XLL → LPCM 7.1) плюс нативный E-AC-3 JOC. На Android TV и desktop — полноценный bitstream.

### 2.5 Аппаратное декодирование

| Платформа | API | Через что |
|---|---|---|
| Apple | VideoToolbox (H.264, HEVC Main10, ProRes, AV1 на A17 Pro/M3+, VP9 на части) | FFmpeg `hwaccel videotoolbox` или прямой `VTDecompressionSession` |
| Android | MediaCodec | Media3; mpv `hwdec=mediacodec` |
| Windows | D3D11VA / DXVA2, NVDEC | mpv `hwdec=d3d11va` / `auto-safe` |
| Linux | VA-API, VDPAU, NVDEC, Vulkan Video | mpv `hwdec=vaapi`/`vulkan` |

Софт-фолбэки: dav1d (AV1, BSD), встроенные декодеры FFmpeg (VC-1, MPEG-2, MPEG-4 ASP/DivX, VP9 и др.).

### 2.6 Субтитры

| Формат | mpv/libVLC | Media3 | AVPlayer | Рекомендация |
|---|---|---|---|---|
| SRT/WebVTT | ✅ | ✅ | WebVTT ✅ | — |
| **ASS/SSA** (стили, караоке, шрифты из MKV-attachments) | ✅ libass | упрощённо | ❌ | **libass в Rust-ядре или в нативном модуле** → RGBA-оверлей поверх видео на Android/Apple |
| **PGS** (Blu-ray) | ✅ | ✅ | ❌ | FFmpeg `pgssub` → битмап-оверлей |
| **VobSub** (DVD) | ✅ | ✅ | ❌ | FFmpeg `dvdsub` |
| DVB-SUB, CEA-608/708 | ✅ | ✅ | 608 ✅ | — |

Важно:
- PGS и ASS при HDR-выходе нужно рисовать в PQ-пространстве с ограничением яркости (≈200–300 нит), иначе субтитры «режут глаза».
- Нужны поиск и загрузка внешних субтитров (OpenSubtitles API), синхронизация по задержке и выбор шрифтов.

### 2.7 Переключение частоты кадров и динамического диапазона (AFR / Match Content)

- **tvOS**:
  - AVPlayer делает это сам при включённом «Match Content» в настройках пользователя.
  - Свой движок должен выставить `AVDisplayManager.preferredDisplayCriteria = AVDisplayCriteria(refreshRate:formatDescription:)` (публичный init с tvOS 17), дождаться `isDisplayModeSwitchInProgress == false` и только потом начать вывод.
  - mpv на tvOS: то же, вручную.
- **Android TV**:
  - Частота: `Surface.setFrameRate(fps, FRAME_RATE_COMPATIBILITY_FIXED_SOURCE, CHANGE_FRAME_RATE_ALWAYS)`, в Media3 — `setVideoChangeFrameRateStrategy`.
  - Разрешение и refresh: `WindowManager.LayoutParams.preferredDisplayModeId` по `Display.getSupportedModes()`.
  - HDR-режим переключает сам MediaCodec/HWC по метаданным потока.
  - Делать собственный переключатель «разрешение + частота» как опцию: так делает Kodi.
- **Windows/Linux**: mpv `--display-fps-override` и скрипты смены режима (Win: `ChangeDisplaySettingsEx`, Linux: xrandr/wlr-output-management). Низкий приоритет.

### 2.8 Рекомендуемая архитектура плеера

```
            ┌──────────────────── Rust core ─────────────────────┐
            │ ByteSource (SMB/NFS/WebDAV/FTP/HTTP)  · Library DB │
            │ Session/Progress · Track selection policy          │
            └──────┬───────────────┬───────────────┬─────────────┘
                   │ UniFFI/FRB    │               │
     ┌─────────────▼───┐   ┌───────▼────────┐  ┌───▼─────────────────────┐
     │ Desktop (W/L/M) │   │ Android/ATV    │  │ Apple (iOS/tvOS/macOS)  │
     │ libmpv gpu-next │   │ Media3 (основн)│  │ Этап 1: MPVKit + AVPlayer│
     │ native window   │   │ + libass overlay│ │ Этап 2: FFmpeg demux →   │
     │ (HDR/AFR)       │   │ libmpv fallback │  │ VT → AVSBDL/AVSBAR (DV)  │
     └─────────────────┘   └────────────────┘  │ + MPVKit fallback        │
                                               └──────────────────────────┘
```

Логика выбора движка («Auto»):
- Android: Media3 может → Media3, иначе libmpv.
- Apple: AVFoundation-совместимо, включая remux-HLS от Jellyfin → нативный путь; иначе свой движок; если и он не может → MPVKit.

То же самое делают современные tvOS-клиенты: DV отдают нативному пути, остальное — mpv ([Nuvio](https://newreleases.io/project/github/bobsupra/NuvioTVOS/release/tvos-beta-3.3.1)).

Единый контракт плеера (`PlayerEngine` в ядре и платформенных слоях): `open(source, startPos)`, треки, `seek`, скорость, события, статистика. Это даёт единый UI-оверлей управления (Flutter или SwiftUI) поверх нативной поверхности.

**Встраивание в Flutter**:
- Android: отдельная `Activity` с `SurfaceView` (или PlatformView в Hybrid Composition).
- iOS: `UIViewController` с AVSBDL, презентуемый поверх Flutter.
- Desktop: libmpv render API в Flutter-текстуру для SDR-v1; для HDR — нативное дочернее окно (HWND / X11 / Wayland subsurface) с `vo=gpu-next`.
- Оверлей управления можно делать нативно (проще для TV-фокуса) или Flutter-слоем поверх.

---

## 3. Лицензионные ловушки

### 3.1 GPL/LGPL и App Store

| Компонент | Лицензия по умолчанию | Ловушка | Что делать |
|---|---|---|---|
| FFmpeg | LGPLv2.1+; с `--enable-gpl` → GPL; с `--enable-nonfree` → нераспространяемо | x264, x265, libxvid, frei0r, rubberband, vidstab — GPL; libfdk-aac — nonfree | Только LGPL-сборка, декодеры встроенные + dav1d (BSD) |
| mpv/libmpv | GPLv2+ по умолчанию; **LGPLv2.1+ при `-Dgpl=false`** | GPL-only части отключаются этим флагом | Собирать с `-Dgpl=false` |
| MPVKit | LGPLv3; бинарники с суффиксом `-GPL` — GPLv3 ([README](https://github.com/mpvkit/MPVKit)) | легко взять GPL-вариант | Брать не-GPL сборку или собирать самим |
| libplacebo | LGPLv2.1+ | — | ok |
| libass | ISC | — | ok |
| libdovi (dovi_tool) | MIT | — | ok |
| libVLC/VLCKit | LGPLv2.1+ (VLC-приложение — GPL) | модули VLC частично GPL | только libVLC-сборки |
| libsmb2 | LGPLv2.1 ([репо](https://github.com/sahlberg/libsmb2)) | статическая линковка в iOS | динамический framework |
| libnfs | LGPLv2.1 (библиотека) | то же | то же |
| KSPlayer (Swift-плеер на FFmpeg) | GPL / коммерческая (проверить актуальную) | GPL в App Store | не использовать без коммерческой лицензии |
| Qt | LGPLv3 / коммерческая | LGPLv3 + iOS | не используем |

**Почему GPL несовместим с App Store.** Условия Apple Media Services/EULA накладывают ограничения (число устройств, запрет реверса), которые трактуются как «дополнительные ограничения», запрещённые GPL. Прецедент: VLC удалили из App Store в 2011 году, вернули после перелицензирования libVLC под LGPL.

**LGPL в App Store** на практике используется массово (Infuse, VLC, MPVKit-приложения). Условия соблюдения:

1. **Динамическая линковка**: FFmpeg, mpv, libsmb2, libnfs — отдельные `.framework`/`.dylib`/`.so`/`.dll`. На iOS/tvOS это embedded dynamic frameworks в XCFramework.
2. Публикация **точного исходного кода** использованных LGPL-библиотек и всех патчей, а также скриптов сборки (публичный репозиторий `project_alpha-thirdparty`).
3. **Возможность перелинковки**: для LGPLv2.1 достаточно динамических библиотек плюс инструкции. Для LGPLv3 (MPVKit) есть спорный пункт об «Installation Information» для User Products. Минимизировать риск: предпочитать LGPLv2.1-версии (собрать mpv/FFmpeg самим, не брать LGPLv3-зависимости) и/или предоставлять объектные файлы приложения по запросу.
4. Экран «Лицензии и благодарности» в приложении, тексты лицензий в бандле.
5. Не модифицировать LGPL-код «закрыто»: все патчи открыты.

**Рецепт LGPL-сборки FFmpeg** (схема):
```
./configure --disable-gpl --disable-nonfree --disable-version3* \
  --enable-shared --disable-static --disable-programs --disable-doc \
  --enable-libdav1d --enable-libass(для субтитров) \
  --enable-videotoolbox / --enable-mediacodec / --enable-d3d11va / --enable-vaapi \
  --disable-encoders (кроме нужных: например, aac/ac3 для перекодирования аудио)
# *--enable-version3 (LGPLv3) допустим, но лучше оставаться на v2.1
```
CI-проверка: `ffmpeg -L`/`avutil_license()` должна возвращать «LGPL version 2.1 or later». Сборка падает, если в конфигурации всплывает `--enable-gpl`.

### 3.2 Dolby, DTS и патенты

- **Декодеры FFmpeg** (ac3, eac3, truehd/mlp, dca включая DTS-HD MA XLL) открыты, но **патентная лицензия на кодек не входит в лицензию кода**.
  - Патенты на AC-3 истекли (≈2017).
  - E-AC-3 и TrueHD — частично.
  - DTS-HD — неоднородно по юрисдикциям.
- **Atmos-объекты (JOC/TrueHD-Atmos) и DTS:X** декодировать в объектный рендер открытыми средствами нельзя. Их можно только пропустить bitstream'ом или декодировать «ядро».
- **Логотипы Dolby/DTS/Dolby Vision** использовать в UI и маркетинге нельзя без лицензии (Dolby Vision — программа для партнёров). Писать нейтрально: «DV P8.1», «HDR (PQ)».
- **Как снижают риск игроки рынка**: опираются на системные декодеры (Apple/Android платят роялти за устройство), а собственные декодеры используют как fallback. Infuse исторически включал DTS/TrueHD в Pro-подписку и временами менял поведение passthrough ([тред](https://community.firecore.com/t/dts-hd-passthrough-not-working/12883)).
- **Рекомендация**:
  - E-AC-3 и AC-3 на Apple и Android по возможности декодировать системно (AudioToolbox/MediaCodec), что покрыто лицензией платформы.
  - TrueHD и DTS-HD — FFmpeg-декод как fallback, passthrough там, где можно.
  - Перед коммерческим запуском — консультация юриста по патентам E-AC-3/DTS в целевых странах.
  - HEVC: аппаратный декодер платформы покрыт лицензиями производителя. Софт-декод HEVC через FFmpeg — серая зона (пулы Access Advance/Via LA); держать его как fallback.

---

## 4. Сетевые клиентские библиотеки

| Протокол | Библиотека | Язык/лицензия | Платформы | Примечания |
|---|---|---|---|---|
| **SMB2/3** | **[libsmb2](https://github.com/sahlberg/libsmb2)** | C, LGPLv2.1 | все, включая iOS/tvOS ([AMSMB2](https://github.com/amosavian/AMSMB2) — Swift-обёртка) | SMB3 encryption через `smb3_set_seal` с встроенным NTLMSSP или KRB5. Async API, высокий throughput. Используется в Kodi, VLC, Infuse-подобных приложениях. **Основной выбор** |
| SMB2/3 | `smb` (smb-rs, Rust) | Rust, MIT | все | pure Rust, SMB 3.1.1, шифрование, подпись. Моложе libsmb2 — кандидат на замену, если покажет зрелость (избавляет от LGPL) |
| SMB | libdsm | C, LGPL | — | только SMB1 — не использовать |
| **NFS v3/v4** | **[libnfs](https://github.com/sahlberg/libnfs)** | C, LGPLv2.1 (lib) | все, включая iOS (используется VLC iOS) | userspace-клиент, не требует mount (критично для iOS/tvOS/Android без root) |
| **WebDAV** | `reqwest` + `quick-xml` (свои PROPFIND) / `reqwest_dav` | Rust, MIT/Apache | все | Range-запросы для стриминга, Basic/Digest auth |
| **FTP/FTPS** | `suppaftp` | Rust, MIT/Apache | все | REST для seek |
| **SFTP** | `russh` + `russh-sftp` | Rust, Apache | все | — |
| **HTTP/Jellyfin/Emby/Plex** | `reqwest` + сгенерированные клиенты (Jellyfin OpenAPI) | Rust | все | Direct Play/Direct Stream/Transcode (HLS) |
| **Discovery** | `mdns-sd` (Bonjour), WS-Discovery (свой), NetBIOS NS (свой) | Rust | все | на iOS — `NWBrowser` для Bonjour |

Особенности iOS/tvOS-песочницы:
- Обычные TCP-сокеты (445 SMB, 2049 NFS) из приложения разрешены, mount не нужен и невозможен. Библиотеки userspace — правильный выбор.
- **Local Network Privacy** (iOS 14+/tvOS): нужен `NSLocalNetworkUsageDescription`, `NSBonjourServices` (`_smb._tcp`, `_nfs._tcp`, `_webdav._tcp`, `_jellyfin…`). Для широковещательного/мультикаст-discovery (NetBIOS, WS-Discovery) нужен entitlement `com.apple.developer.networking.multicast`, который запрашивается у Apple вручную.
- **tvOS не гарантирует постоянное локальное хранилище**: всё, кроме небольшого `UserDefaults`, — кеш, который система может очистить. БД медиатеки на tvOS должна восстанавливаться: синхронизация через свой сервер или CloudKit, быстрое пересканирование, хранение только кеша.
- Фоновые загрузки и офлайн на iOS: `URLSession` background работает только для HTTP(S). Для SMB/NFS «скачать для офлайна» возможно только пока приложение активно или в рамках `BGProcessingTask`.
- Android: сеть свободна. Для Android 13+ — `NEARBY_WIFI_DEVICES` не нужен для обычных сокетов. Multicast: `WifiManager.MulticastLock`.

**Стриминг в плеер**:
- Ядро отдаёт `ByteSource` с read-ahead-буфером (настраиваемый, 32–256 МБ для 4K-ремуксов по SMB через Wi-Fi) и параллельным prefetch.
- Адаптеры: в FFmpeg — через `AVIOContext`, в mpv — через `stream_cb`, в Media3 — через кастомный `DataSource`.
- Для AVPlayer и как универсальный fallback — loopback HTTP-сервер на `127.0.0.1` с Range-поддержкой.

---

## 5. Музыка

### 5.1 Декодирование

| Формат | FFmpeg | Symphonia (pure Rust) | Системно Apple | Системно Android |
|---|---|---|---|---|
| MP3 | ✅ | ✅ | ✅ | ✅ |
| AAC/HE-AAC | ✅ (встроенный декодер) | AAC-LC | ✅ | ✅ |
| FLAC (до 32/384) | ✅ | ✅ | ✅ | ✅ |
| ALAC | ✅ | ✅ | ✅ | ✅ (Android 5+) |
| Opus | ✅ (+libopus BSD) | ⚠️ не полностью | ✅ (iOS 17+ в CAF/Ogg ограниченно) | ✅ |
| Vorbis | ✅ | ✅ | ❌ | ✅ |
| WAV/AIFF | ✅ | ✅ | ✅ | ✅ |
| WavPack, APE, TAK, MPC | ✅ | частично | ❌ | ❌ |
| **DSD (DSF/DFF)** | ✅ декодер в PCM (`dsd_lsbf` и др.) | ❌ | ❌ | ❌ |

**Решение**: один путь декодирования — **FFmpeg (LGPL) в Rust-ядре** (`rsmpeg`/`ffmpeg-next`) на всех платформах. Он даёт одинаковое поведение: gapless-метаданные (iTunSMPB, LAME header, encoder delay/padding), теги, обложки.

Для DSD:
- DSD → PCM (176.4 кГц) по умолчанию;
- **DoP (DSD over PCM)** на desktop для совместимых DAC в эксклюзивном режиме (этап 3);
- native DSD — только ASIO на Windows (нишево, позже).

### 5.2 Движок вывода и gapless

Вариант A (быстрый старт): **libmpv в аудио-режиме** (`--gapless-audio=yes`, `--prefetch-playlist`, `--replaygain=track|album`, `--audio-exclusive` на Windows). Есть Flutter-обёртка [mpv_audio_kit](https://pub.dev/packages/mpv_audio_kit/versions/0.4.2/changelog) с актуальным libmpv. Минусы: gapless «почти» идеален, кроссфейда нет, тонкий контроль слабый.

Вариант B (целевой): **свой аудио-движок в Rust**:
- декод FFmpeg → ресемплер (`rubato`, по необходимости) → кольцевой буфер → платформенный вывод;
- выход: `cpal` для shared-режима плюс собственные бэкенды для эксклюзивного режима: WASAPI exclusive (Win), CoreAudio hog mode + смена nominal sample rate (macOS), ALSA `hw:` direct (Linux, мимо PipeWire или через PipeWire с `node.force-rate`), AAudio/Oboe (Android).
- Возможности: истинный gapless (следующий трек декодируется заранее, стык сэмпл-в-сэмпл с учётом delay/padding), кроссфейд, ReplayGain track/album с pre-amp и prevent clipping, а также поддержка тегов R128 для Opus.
- Мобильные платформы Apple: декод в ядре → `AVAudioEngine`/`AVAudioSourceNode`, либо `AudioQueue` в формате источника.

**Bit-perfect на desktop**:

| ОС | Механизм | Особенности |
|---|---|---|
| Windows | WASAPI Exclusive (event-driven), опционально ASIO | смена частоты дискретизации под трек, отключение системного микшера |
| macOS | CoreAudio: hog mode + `kAudioDevicePropertyNominalSampleRate` + целочисленный формат потока | как Audirvana/Roon |
| Linux | ALSA `hw:` напрямую, либо PipeWire с разрешённым набором частот (`default.clock.allowed-rates`) | — |
| Android | Android 14+: `AudioMixerAttributes` + `MIXER_BEHAVIOR_BIT_PERFECT` для USB-DAC | работает только с USB-аудио |
| iOS | нет настоящего эксклюзива; `AVAudioSession.setPreferredSampleRate` с USB DAC | — |

### 5.3 Фон, медиасессии, CarPlay и Android Auto

- **Android**:
  - `MediaSessionService` / `MediaLibraryService` из Media3 — обязательно для фонового воспроизведения, уведомления, Bluetooth-кнопок, **Android Auto** (browse-дерево через `MediaLibrarySession`) и Wear OS.
  - Foreground service типа `mediaPlayback`.
  - Для Auto достаточно media-шаблона, без своего UI. Требуется ревью Google Play для Auto.
  - Свой Rust-аудиодвижок оборачивается в Media3 `SimpleBasePlayer`. Так сохраняется вся системная интеграция.
- **iOS/iPadOS**:
  - `AVAudioSession` category `.playback`, background mode `audio`;
  - `MPNowPlayingInfoCenter` и `MPRemoteCommandCenter`.
  - **CarPlay Audio** требует entitlement `com.apple.developer.carplay-audio`, который выдаётся по заявке Apple. Используются шаблоны `CPListTemplate`/`CPNowPlayingTemplate`.
- **macOS**: Now Playing через MPNowPlayingInfoCenter, медиаклавиши.
- **Windows**: SMTC (`SystemMediaTransportControls`).
- **Linux**: MPRIS2 (D-Bus).
- **Flutter**: `audio_service` закрывает часть этого, но для Android Auto и CarPlay надёжнее нативный слой (Kotlin/Swift) поверх Rust-движка. Flutter-UI только управляет им.

---

## 6. Каналы публикации: бесплатные и платные

### 6.1 Таблица каналов

| Канал | Платформа | Стоимость | Ограничения и заметки |
|---|---|---|---|
| **GitHub Releases** | все desktop + Android APK | бесплатно | AppImage/deb/rpm, MSI/EXE, DMG, APK. Без подписи: SmartScreen (Win) и Gatekeeper (macOS) пугают пользователей. На macOS Sequoia+ обход Gatekeeper усложнён |
| **Flathub** | Linux | бесплатно | Проприетарные приложения допустимы (extra-data или собственные бинарники). **Встроенных платежей Flathub до сих пор нет**: работа с Stripe шла с 2023 года, запуска не подтверждено ([itsfoss](https://itsfoss.com/news/flathub-paid-apps/), [discourse](https://discourse.flathub.org/t/when-are-paid-applications-supported/7757)). Монетизация через свой лицензионный ключ или аккаунт внутри приложения. Верификация издателя через GitHub/домен |
| **AUR** | Arch | бесплатно | PKGBUILD (бинарный `-bin` или из исходников), поддержка сообществом. Без модерации — нужен свой мейнтейнер |
| **Snap Store** | Ubuntu и др. | бесплатно | Работает, но в Linux-аудитории медиаплееров менее популярен; strict confinement осложняет доступ к сети и устройствам. Низкий приоритет |
| **F-Droid** | Android | бесплатно | **Только полностью FOSS, сборка из исходников на серверах F-Droid**, без проприетарных SDK (Firebase, Play Billing, проприетарные кодеки). Платные фичи через ключ формально не запрещены, но приложение получит anti-features и вызовет недовольство. Сам F-Droid переживает удар от **Android developer verification** (принудительно с 30.09.2026 в BR/ID/SG/TH, глобально в 2027 — [F-Droid open letter](https://f-droid.org/ru/2026/02/24/open-letter-opposing-developer-verification.html), [обзор](https://pinggy.io/blog/f_droid_2_0_android_developer_verification/)). Канал только для «free/open» редакции |
| **winget** | Windows | бесплатно | PR манифеста в `winget-pkgs`. Неподписанный инсталлятор допустим, но ловит SmartScreen |
| **Microsoft Store** | Windows | **бесплатно для физлиц** (с 09.2025, [Windows Blog](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/)); для компаний тоже убирают $99 ([Neowin](https://www.neowin.net/amp/microsoft-makes-company-developer-accounts-free-for-the-microsoft-store/)) | Win32/MSIX. **Store сам подписывает MSIX** — бесплатный способ избежать покупки сертификата. Для Win32-приложений можно использовать собственную коммерцию (0% комиссии Store при своей платёжке); при Store-коммерции — 15% (приложения) / 12% (игры) |
| **Homebrew cask** | macOS | бесплатно | **С 01.09.2026 неподписанные/ненотаризованные casks удаляются из официального tap** ([Homebrew 5.0](https://workbrew.com/blog/homebrew-5-0-0)). Без Apple Developer Program канал фактически закрыт |
| **Google Play** | Android, Android TV, Google TV | **$25 единоразово** | Новые личные аккаунты обязаны пройти закрытое тестирование (≥12 тестеров, 14 дней) перед продакшеном. Комиссия 15% на первый $1M/год. Отдельное ревью для TV и Auto. Аккаунт Play также закрывает требование developer verification |
| **Apple Developer Program** | iOS, iPadOS, tvOS, macOS (MAS и notarization) | **$99/год** | Обязателен для любой дистрибуции iOS/tvOS и для нотаризации macOS. Small Business Program — 15%. CarPlay и multicast entitlements — по заявке |
| **Подпись Windows: Azure Artifact Signing** (бывший Trusted Signing) | Windows вне Store | **$9.99/мес** (Basic, 5000 подписей) ([pricing](https://azure.microsoft.com/pricing/details/artifact-signing/)) | **Физлица — только США и Канада**; организации — США, Канада, ЕС, UK ([quickstart](https://learn.microsoft.com/azure/artifact-signing/quickstart)). Для разработчика из другой страны путь — юрлицо в ЕС/UK/US или альтернатива |
| Альтернативы подписи Windows | — | SignPath Foundation — бесплатно для OSS-проектов; Certum Open Source Code Signing — недорого (≈десятки €/год, проверить); классические OV/EV — $200–600/год | OV после 2023 требует HSM/токен |

### 6.2 Рекомендованный порядок выхода

**Этап 0 — альфа и бета, $0.**
- Linux: GitHub Releases (AppImage), Flathub, AUR.
- Windows: GitHub Releases (неподписанный, с инструкцией), winget, **Microsoft Store** (бесплатно и с бесплатной подписью).
- Android: APK на GitHub Releases, Obtainium, по желанию F-Droid для FOSS-редакции.
- Цель — обкатать ядро, сетевой слой, libmpv-движок и медиатеку на desktop/Android, где HDR- и лицензионных сложностей меньше всего.

**Этап 1 — Google Play, $25.**
- Android и Android TV: Media3-движок, TV-фокус, passthrough.
- Сразу запустить закрытое тестирование: 14 дней обязательны.

**Этап 2 — Apple, $99/год.**
- Покупать **одновременно с готовностью хотя бы двух Apple-таргетов**, чтобы год подписки не простаивал: macOS (нотаризация, Homebrew cask, DMG) и iOS/iPadOS через TestFlight, затем tvOS.
- Параллельно заявки на CarPlay audio и multicast entitlement: они идут неделями.

**Этап 3 — подпись Windows вне Store.**
- Если Store-канала недостаточно: Artifact Signing (при наличии юрлица в ЕС/UK) или SignPath/Certum.

Модель монетизации (вне моей зоны, но с техническими следствиями):
- одна кодовая база с флагами «Pro», открытое ядро (LGPL-совместимо) и закрытый UI или наоборот;
- на Apple только IAP (StoreKit 2);
- на Google — Play Billing (кроме F-Droid-сборки);
- на desktop — собственные лицензионные ключи (Paddle/Lemon Squeezy).

---

## 7. Итоговый рекомендуемый стек

| Слой | Выбор | Альтернатива / запасной вариант |
|---|---|---|
| Общее ядро | **Rust** (tokio, rusqlite, reqwest, serde) | KMP |
| Привязки | **UniFFI** (Swift, Kotlin), **flutter_rust_bridge** (Dart) | C ABI + ffigen |
| UI: Android, Android TV, iOS/iPadOS, Windows, Linux, macOS | **Flutter** (Material 3 + своя дизайн-система, TV-фокус-слой) | Compose Multiplatform |
| UI: tvOS | **SwiftUI** (нативный, отдельное приложение; позже можно расширить на iOS/macOS) | flutter-tvos (сторонний, не рекомендуется) |
| Плеер Android/ATV | **Media3/ExoPlayer** + FFmpeg audio ext + libass-оверлей; fallback **libmpv** | libVLC |
| Плеер Apple | Этап 1: **MPVKit (LGPL) + AVPlayer** для совместимых потоков. Этап 2: **свой FFmpeg → VideoToolbox → AVSampleBufferDisplayLayer** (DV P5/P8.1, P7→P8.1, AVDisplayCriteria) | VLCKit 4 |
| Плеер desktop | **libmpv** (gpu-next/libplacebo, `-Dgpl=false`), нативное окно для HDR | libVLC |
| Субтитры | libass + FFmpeg bitmap-декодеры, PQ-aware композиция | — |
| Сеть | **libsmb2**, **libnfs** (LGPL, динамически), Rust: WebDAV/FTP/SFTP/HTTP | smb-rs (pure Rust) |
| Музыка | Этап 1: libmpv audio (gapless, ReplayGain). Этап 2: свой Rust-движок (FFmpeg-декод, WASAPI-excl/CoreAudio hog/ALSA hw, DoP) | — |
| Системная интеграция аудио | Media3 MediaLibraryService (Android Auto), MPNowPlaying + CarPlay, SMTC, MPRIS | audio_service (Flutter) |
| FFmpeg | собственная **LGPLv2.1** сборка: shared, dav1d, libass, hwaccel; CI-проверка лицензии | — |
| Каналы | GitHub/Flathub/AUR/winget/MS Store → Google Play → Apple | — |

**Главные риски и как их снять**

1. *HDR и DV на Apple требуют своего движка.* Это 2–4 месяца работы. Снимается поэтапно: MPVKit плюс AVPlayer с HLS-remux от сервера на первом этапе.
2. *Flutter-видео в SDR.* Плеер с первого дня делать нативным экраном, а не `Texture`-виджетом.
3. *tvOS — второй UI.* Ограничить tvOS-приложение «10-футовой» медиатекой и плеером, вся логика в Rust-ядре. SwiftUI на tvOS компактен.
4. *Лицензии.* LGPL-only сборки, динамическая линковка, публичный thirdparty-репозиторий, юр-проверка патентов E-AC-3/DTS/HEVC перед монетизацией.
5. *Хранилище на tvOS очищаемо.* Синхронизация медиатеки через собственный сервер или CloudKit.
6. *Android developer verification (2027 глобально).* Регистрироваться в Play Console заранее; F-Droid-канал считать нестабильным.

---

## 8. Источники

- Infuse: https://firecore.com/infuse ; форум Firecore о DV P7/P8: https://community.firecore.com/t/dolby-vision-profile-7-8-support-ts-mkv-files/19713 ; P7 FEL: https://community.firecore.com/t/technical-feasibility-native-p7-fel-playback-via-in-app-dual-layer-composition/60611 ; Atmos/DTS:X: https://community.firecore.com/t/dolby-atmos-and-dtsx-not-supported-in-infuse/19820 ; passthrough tvOS 11.3: https://community.firecore.com/t/passthrough-not-available-in-tvos-11-3-solution-use-auto-instead/16025
- Flutter tvOS (сторонний): https://pub.dev/packages/flutter_tvos , https://github.com/fluttertv/plugins ; Flutter на webOS: https://webostv.developer.lge.com/news/flutter-for-webos-tv-is-now-available-for-developers ; Tizen: https://developer.samsung.com/smarttv/develop/native/flutter.html
- Compose Multiplatform на tvOS (неофициально): https://proandroiddev.com/my-journey-making-compose-multiplatform-work-on-tvos-and-what-i-learned-28744910489e , https://plugins.gradle.org/plugin/dev.sajidali.compose-tvos ; Compose for TV: https://developer.android.com/training/tv/playback/compose
- React Native 0.82 (New Architecture only): https://reactnative.dev/blog/2025/10/08/react-native-0.82.md ; react-native-tvos: https://github.com/react-native-tvos/react-native-tvos , https://unpkg.com/react-native-tvos@0.86.0-2/README.md
- mpv 0.41 / gpu-next: https://linuxiac.com/mpv-0-41-media-player-switches-to-gpu-next-renderer/ , https://sources.debian.org/src/mpv/0.41.0-2/RELEASE_NOTES ; libplacebo: https://www-test.videolan.org/projects/libplacebo/
- MPVKit: https://github.com/mpvkit/MPVKit ; mpv vs нативный плеер на Apple TV (Emby): https://emby.media/community/topic/149237-whats-the-current-state-of-the-mvp-vs-native-player-in-the-appletv-client/ ; гибридные tvOS-клиенты: https://newreleases.io/project/github/bobsupra/NuvioTVOS/release/tvos-beta-3.1 , https://newreleases.io/project/github/VortXTV/VortX/release/v0.3.11-b163
- media_kit и альтернативы: https://depscope.dev/pkg/pub/media_kit , https://pub.dev/documentation/mpv_kit/latest/ , https://pub.dev/packages/mpv_audio_kit/versions/0.4.2/changelog
- libsmb2: https://github.com/sahlberg/libsmb2 ; AMSMB2: https://github.com/amosavian/AMSMB2 ; libnfs: https://github.com/sahlberg/libnfs
- Microsoft Store бесплатно для физлиц: https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/ ; для компаний: https://www.neowin.net/amp/microsoft-makes-company-developer-accounts-free-for-the-microsoft-store/
- Azure Artifact Signing: https://azure.microsoft.com/pricing/details/artifact-signing/ , https://learn.microsoft.com/azure/artifact-signing/quickstart , https://learn.microsoft.com/azure/artifact-signing/faq
- Homebrew 5.0 и неподписанные casks: https://workbrew.com/blog/homebrew-5-0-0
- Flathub и платежи: https://itsfoss.com/news/flathub-paid-apps/ , https://discourse.flathub.org/t/when-are-paid-applications-supported/7757
- Android developer verification / F-Droid: https://f-droid.org/ru/2026/02/24/open-letter-opposing-developer-verification.html , https://pinggy.io/blog/f_droid_2_0_android_developer_verification/
- FFmpeg лицензирование: https://ffmpeg.org/legal.html ; mpv лицензия: https://github.com/mpv-player/mpv/blob/master/Copyright
- Media3: https://developer.android.com/media/media3/exoplayer ; Android frame rate API: https://developer.android.com/media/optimize/performance/frame-rate ; AVDisplayCriteria: https://developer.apple.com/documentation/avkit/avdisplaycriteria
- Android bit-perfect USB (Android 14): https://source.android.com/docs/core/audio/preferred-mixer-attr
- CarPlay entitlement: https://developer.apple.com/carplay/ ; Android Auto media: https://developer.android.com/training/cars/media

> Примечание о достоверности: часть утверждений о поведении Infuse и гибридных tvOS-клиентов основана на пользовательских форумах и release notes, а не на официальной документации. Поведение `AVSampleBufferAudioRenderer` с E-AC-3 JOC и точные условия патентных пулов для E-AC-3/DTS нужно проверить прототипом и у юриста до финальных решений.
