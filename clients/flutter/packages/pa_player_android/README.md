<!-- SPDX-License-Identifier: MPL-2.0 -->

# pa_player_android

Flutter-плагин плеера Android (PLAY-AND-002, ADR-0008 вариант B): видео играет `Media3Engine` ([clients/android/player](../../../android/player/README.md)) в отдельной `PlayerActivity` на `SurfaceView`, а OSD — второй Flutter-движок, прозрачный `FlutterFragment` поверх видео.

## В приложении

```dart
await PaPlayerAndroid.prewarmOsd();             // при открытии карточки: OSD-движок стартует заранее
final last = await PaPlayerAndroid.open([PlayerItem('file:///…/movie.mkv')], startPositionMs: resumeMs);
// last — EngineState в JSON после закрытия плеера (position_ms, completed…), для «Продолжить просмотр»
```

`nativeOsd: true` — запасной вариант A: контроллер Media3 `PlayerView` вместо Flutter-OSD.

## OSD-entrypoint

```dart
@pragma('vm:entry-point')
void osdMain() {
  WidgetsFlutterBinding.ensureInitialized();
  final osd = PlayerOsdBridge();
  runApp(MyOsd(osd: osd));   // osd.state, osd.updates; osd.seek(…), osd.setAudioTrack(…), osd.close()
}
```

`PlayerOsdBridge` — команды контракта `PlayerEngine` по `MethodChannel('pa_player_android/osd')` (отказ — `PlatformException` с кодом вида `EngineError`: `invalid_state`, `invalid_argument`, `unsupported`, `closed`…) и события по `EventChannel('pa_player_android/events')`: каждое событие — JSON ядра (`{"event":"position","position_ms":…}`) вместе с состоянием после него. Первым приходит `{"event":"snapshot"}` с текущим состоянием. Прогретый OSD подписывается раньше, чем появляется Activity, поэтому Activity вызывает у него `attached`, и мост подписывается заново.

## Сборка

Android-часть берёт исходники движка из `clients/android/player/src/main/kotlin` и ядро (UniFFI-привязки, `libpa_ffi.so`) из `paGenDir` — свойство Gradle приложения (`android/gradle.properties`), по умолчанию `<репозиторий>/target/android` после `tools/bindings/build-aar.sh`. Приложение с этим плагином не должно отдельно подключать AAR `pa-core-android` (те же классы).

В приложение Kinolune плагин пока не подключён: экран плеера 0.1 остаётся на media_kit (ADR-0012), переход — вместе с OSD (UI-013).

## Проверено

- `flutter analyze`, `flutter test` (каналы, порядок событий, переподписка прогретого OSD).
- Временное приложение на эмуляторе Android 14: прогретый OSD, `open` → `Loaded`, позиции, `stats` (h264/aac), `seek(8000)` из OSD, `close` из OSD → `open` вернул состояние с позицией. Вариант A (`nativeOsd`) и реальные устройства (HDR, AFR, время старта) не проверялись.
