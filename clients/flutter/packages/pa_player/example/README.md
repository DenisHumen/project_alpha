<!-- SPDX-License-Identifier: MPL-2.0 -->
# pa_player example

Приложение для ручной проверки `pa_player`. Подробности — в [README пакета](../README.md).

```bash
flutter run -d linux --dart-entrypoint-args /path/to/movie.mkv
flutter build linux && build/linux/x64/release/bundle/pa_player_example movie.mkv
build/linux/x64/release/bundle/pa_player_example a.flac b.opus c.mp3   # музыка
flutter build apk --debug   # на Android путь вводится в поле на стартовом экране
```

Переменные окружения: `PA_START_MS` (продолжить с позиции), `PA_SUBTITLE` (внешние субтитры),
`PA_EXIT_AFTER=N` (через N секунд напечатать `PA_RESULT {json}` и выйти — для headless-проверки).
