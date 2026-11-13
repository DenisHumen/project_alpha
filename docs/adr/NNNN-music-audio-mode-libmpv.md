<!-- SPDX-License-Identifier: MPL-2.0 -->

# ADR-NNNN: Музыка v1 — libmpv в аудиорежиме, ReplayGain из тегов файла, проверка gapless по сэмплам

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** мейнтейнер (решения делегированы владельцем, ADR-0011)
- **Связано:** `MUS-002`, `MUS-001`, `MUS-003`, `MUS-008`, `MUS-016`, `PLAY-002`; [02 §5.2](../research/02-client-stack-playback.md) (вариант A); ADR-0012, ADR-0017, ADR-0020

## Контекст и проблема

В релизе 0.1 музыка уже играет через `pa_player` (media_kit, libmpv): очередь целиком уходит в mpv плейлистом, `gapless-audio` и `prefetch-playlist` включены. Нет нормализации громкости и нет доказательства, что стыки треков действительно без пауз. MUS-001 сохраняет ReplayGain из тегов в `music_tracks.rg_*`. Свой аудиодвижок (вариант B) — отдельная большая задача MUS-008; до него нужен рабочий v1 на libmpv для десктопа и Android.

## Факторы решения

- Один код для Linux, Windows, macOS и Android: в 0.1 везде один движок (`MediaKitEngine`).
- Gapless без переинициализации аудиовыхода между треками; проверка — по сэмплам, а не на слух.
- ReplayGain track и album, pre-amp, защита от клиппинга, R128 у Opus.
- Контракт движка (ADR-0017) не должен зависеть от mpv: Media3 и свой движок получат те же настройки.

## Решение

1. **`AudioProfile`** (`pa_player`, `lib/src/engine/audio_profile.dart`) — настройки аудиорежима без привязки к движку: `gapless`, `replayGain` (`off`, `track`, `album`, `auto`), `preampDb`, `preventClipping`, `fallbackDb`, `exclusive`. `PlayerEngine.setAudioProfile` — новая команда контракта; `MediaKitEngine` переводит профиль в свойства mpv:

   | Поле | Свойство mpv |
   |---|---|
   | `gapless` | `gapless-audio=yes`, `prefetch-playlist=yes` |
   | `replayGain` | `replaygain=no\|track\|album` (`auto` — см. п. 3) |
   | `preampDb` | `replaygain-preamp` (−15…15) |
   | `preventClipping` | `replaygain-clip=no` (в mpv 0.42 `yes` *разрешает* клиппинг) |
   | `fallbackDb` | `replaygain-fallback` — для файлов без тегов |
   | `exclusive` | `audio-exclusive` (WASAPI exclusive на Windows) |

   Все свойства mpv меняются на лету (`UPDATE_VOL`), поэтому смена режима действует на играющий трек.
2. **Значения ReplayGain берёт mpv из тегов файла** (`REPLAYGAIN_*`, у Opus — `R128_*` с поправкой к уровню ReplayGain), то есть те же теги, что MUS-001 пишет в `music_tracks.rg_*`. Передавать значения из базы в mpv нельзя без фильтра громкости на каждый трек, а перестройка фильтров на стыке ломает gapless. База нужна для UI и для своего движка (MUS-008).
3. **`auto`** (по умолчанию) — album gain, пока альбом играет по порядку (все треки очереди с одним ключом альбома, без shuffle), иначе track gain. Ключ альбома — `PlayRequest.albumId` (`music_albums.id` из MUS-001), без него — название альбома. `MusicPlayerController` пересчитывает режим при новой очереди и при переключении shuffle и отправляет профиль до старта первого трека.
4. **Проверка gapless по сэмплам** (`pa_player/test/gapless_libmpv_test.dart`): тест режет непрерывный сигнал на треки длиной не кратной кадрам кодеков, проигрывает их одной очередью через настоящую libmpv с выводом в файл (`ao=pcm`) и сравнивает результат с исходным сигналом: длина, смещение каждого трека (0 сэмплов), провалы в тишину; для WAV и FLAC — совпадение бит в бит. Контрольный прогон с `gapless=false` обязан провалить проверку. Тест требует libmpv и запускается вручную: `PA_LIBMPV_TESTS=1`, `LIBMPV_LIBRARY_PATH`; с `PA_GAPLESS_FFMPEG` дополнительно кодирует альбом в FLAC, MP3, AAC (M4A), Opus и проверяет ReplayGain по тегам. В CI пакета (`flutter test` без libmpv) он пропускается.
5. **Apple (MPVKit)** выделен в задачу `MUS-016`: интеграции MPVKit ещё нет (PLAY-APL-001 не закрыта), собрать и проверить Swift-код без macOS нельзя. Flutter-приложение на macOS использует тот же `MediaKitEngine` и тот же профиль.

### Последствия

- Хорошо: gapless и ReplayGain на всех платформах 0.1 одним кодом; проверка стыков воспроизводима и ловит регресс (контрольный прогон).
- Плохо: **AAC в MP4** (FFmpeg-энкодер, edit list) на libmpv 0.42 с FFmpeg 9.0 теряет 1024 сэмпла в начале каждого трека — priming пропускается дважды; случай помечен в тесте как известная проблема. MP3 (LAME header), Opus, FLAC, WAV проходят. Нет кроссфейда и bit-perfect — это MUS-008 и MUS-009.
- Требует работы: настройки громкости в UI (MUS-004/MUS-005), сохранение профиля вместе с очередью (MUS-003), AAC-стыки и iTunSMPB в своём движке (MUS-008), Apple (MUS-016).

### Как проверяем соблюдение

- Unit-тесты `pa_player/test/music_test.dart`: свойства mpv профиля, семантика `replaygain-clip`, выбор `auto`, отправка профиля до старта очереди.
- `gapless_libmpv_test.dart` на LGPL-сборке libmpv из `tools/mpv` перед изменениями аудиопути (`PA_LIBMPV_TESTS=1`).

## Ссылки

- [mpv: опции `--gapless-audio`, `--replaygain*`](https://mpv.io/manual/stable/#options-replaygain)
- [ReplayGain 2.0](https://wiki.hydrogenaud.io/index.php?title=ReplayGain_2.0_specification), [RFC 7845 §5.2.1 (R128 в Opus)](https://www.rfc-editor.org/rfc/rfc7845#section-5.2.1)
