<!-- SPDX-License-Identifier: MPL-2.0 -->
# pa_player

Плеер релиза Kinolune 0.1 ([контракт](../../../../docs/dev/release-0.1-contract.md), [ADR-0012](../../../../docs/adr/0012-release-0-1-player-media-kit.md)):
полноэкранный видеоплеер с OSD в духе tvOS и музыкальный плеер в духе Spotify. Движок — [media_kit](https://github.com/media-kit/media-kit) (libmpv) на Linux и Android.

## API

```dart
void main() {
  WidgetsFlutterBinding.ensureInitialized();
  ensurePaPlayerInitialized();          // MediaKit.ensureInitialized()
  runApp(const App());
}

Navigator.push(context, MaterialPageRoute(builder: (_) => PlayerScreen(
  request: PlayRequest(uri: path, title: 'Фильм', startPositionMs: savedMs, subtitleUri: srt),
  onProgress: (posMs, durMs) => library.saveProgress(...), // каждые 5 с, на паузе, перемотке, выходе
  onFinished: () => ...,
)));

final music = MusicPlayerController();          // один на приложение
music.play(tracks, index: 3);                   // List<PlayRequest> (+ artist/album/artworkUri)
NowPlayingBar(controller: music)                // мини-плеер внизу; тап открывает NowPlayingScreen
NowPlayingScreen(controller: music)
```

`PlayRequest` расширен необязательными полями `artist`, `album`, `artworkUri`, `durationMs` (для музыки);
поля контракта не менялись. Движок спрятан за интерфейсом `PlayerEngine`; `PlayerScreen` и
`MusicPlayerController` принимают `engineFactory`, а `package:pa_player/testing.dart` даёт `FakePlayerEngine` —
тесты приложения не требуют libmpv.

## Видеоплеер

- OSD: название, scrubber с текущим временем, «осталось» и «закончится в ЧЧ:ММ», ±10 с, play/pause, дорожки аудио,
  субтитры (встроенные, «Выкл.», загрузка внешнего файла через встроенный файловый браузер — работает с пультом,
  без системного file picker), скорость (0,5–2×), Info (кодеки, разрешение, частота кадров, формат звука).
  Автоскрытие через 4 с во время воспроизведения; на паузе и при открытой панели OSD не прячется.
- Клавиатура: Space — пауза, ←/→ — ±10 с, ↑/↓ — показать OSD и перейти между scrubber и кнопками, F — полный экран,
  I — Info, M — без звука, Esc — закрыть панель / выйти из полноэкранного режима / выйти.
- Пульт Android TV: Select — пауза (на scrubber) или нажатие кнопки, ←/→ — перемотка (на scrubber) или переход
  между кнопками, Back — закрыть панель, затем выйти; медиаклавиши Play/Pause/FF/Rewind.
- Мышь и касание: движение мыши или тап показывают OSD, двойной тап по краям — ±10 с, по центру — полный экран.
- Resume: `startPositionMs` передаётся в mpv как `start` файла. Внешние субтитры из `subtitleUri` подключаются после
  открытия файла; соседние `.srt/.ass` подхватываются автоматически (`sub-auto=fuzzy`).

## Музыка

Вся очередь передаётся в libmpv одним плейлистом с `gapless-audio=yes` и `prefetch-playlist=yes` — переходы между
треками без пауз (следующий файл открывается заранее). Shuffle (текущий трек остаётся первым, остальные
перемешиваются), repeat off/all/one, «назад» после 3 с перематывает трек в начало. Форматы — всё, что декодирует
FFmpeg сборки: MP3, FLAC, ALAC, Opus, AAC, WAV/PCM, Vorbis, WavPack, APE, DSD. Обложка — `artworkUri`, иначе
сгенерированная заглушка; на широких экранах (десктоп, ТВ) обложка слева, очередь справа.

## Пример

`example/` — приложение для ручной проверки, см. [example/README.md](example/README.md).

## Лицензии (проверено для 0.1)

Dart-пакеты media_kit, media_kit_video, media_kit_libs_linux, media_kit_libs_android_video — MIT.
Транзитивно: wakelock_plus, package_info_plus (BSD-3-Clause), dbus, uuid, image, http, synchronized,
safe_local_storage, uri_parser (MIT/BSD). Всё совместимо с MPL-2.0. Нативные библиотеки:

| Платформа | Что подключаем | Откуда libmpv | Лицензия |
|---|---|---|---|
| Linux | `media_kit_libs_linux` | **системный** `libmpv.so.2` (динамически, `dlopen`), в бандл не кладётся. Пакет скачивает только mimalloc (MIT) | LGPL-2.1+ при сборке mpv с `-Dgpl=false`; дистрибутивные пакеты обычно собраны с GPL — это выбор пользователя/дистрибутива, наш бинарь GPL-кода не содержит. Для AppImage/Flatpak (R01-REL) кладём libmpv из PLAY-002 (`-Dgpl=false` + LGPL-FFmpeg PLAY-001) |
| Android | `media_kit_libs_android_video` 1.3.8 | `libmpv-android-video-build` v1.1.7, flavour **`default`** (`default-<abi>.jar`, MD5 проверяется в Gradle) | FFmpeg 6.0 `--disable-gpl --disable-nonfree --enable-version3` (LGPL-3.0+), mpv `-Dgpl=false` (LGPL-2.1+), libass (ISC), freetype (FTL), fribidi (LGPL-2.1+), harfbuzz (MIT), mbedTLS (Apache-2.0), dav1d (BSD-2), libxml2 (MIT). Итог — **LGPL-3.0+**, `libmpv.so` — отдельная динамическая библиотека в APK |

Чего избегаем:

- **`media_kit_libs_video` / `media_kit_libs_audio`** (зонтичные пакеты) — тянут сборки для Windows/macOS/iOS, которые мы
  не проверяли; подключаем только платформенные пакеты.
- Flavour **`encoders-gpl`** в `libmpv-android-video-build` (libx264, `--enable-gpl`) — GPL, не используем.
  Flavour `full` в 0.1 тоже не нужен.
- `media_kit_libs_android_audio` не нужен: video-вариант включает те же аудиодекодеры.

Обязательства LGPL для APK: в «О программе» приложения (R01-APP) — уведомление о libmpv/FFmpeg, их лицензии и ссылка
на исходники (`media-kit/libmpv-android-video-build` тега v1.1.7 и upstream mpv/FFmpeg); `libmpv.so` можно заменить
(динамическая библиотека). Патентно-обременённые декодеры (HEVC, AAC, AC-3) присутствуют в FFmpeg — вопрос DEC-005,
энкодеров нет.

## Тесты

```bash
flutter analyze && flutter test   # без libmpv: FakePlayerEngine
```
