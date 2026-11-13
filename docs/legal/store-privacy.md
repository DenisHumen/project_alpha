# Анкеты приватности сторов: App Store и Google Play (LEGAL-007)

> Черновик ответов к [политике конфиденциальности](privacy-policy.md) · 2026-11-13. Заполняет владелец в App Store Connect и Play Console (PKG-задачи сторов). **Не юридическая консультация.** Пометка **[проверить]** — решение, которое владельцу нужно подтвердить при заполнении: формулировки правил сторов меняются.
>
> Речь о **store-сборках** (S §6): загрузки Twitch выключены, донат-ссылок нет. Если включатся «чаевые» через IAP (DEC-003), платежи обрабатывает стор, и в анкеты это не попадает.

## 0. Исходные факты

| Факт | Источник |
|---|---|
| У проекта нет серверов; данные о пользователях разработчикам не приходят | S §1, §5 |
| SDK аналитики, рекламы и крэш-репортинга нет; телеметрия только по opt-in | ROADMAP §0.3 п. 10 |
| Crash-отчёты сохраняются локально только по opt-in и отправляются вручную пользователем (через issue на GitHub) | CORE-008, UI-017 |
| Секреты лежат в Keychain и Android Keystore | CORE-007 |
| Приложение само (своим кодом, не через SDK) обращается к TMDB, AniList, Shikimori, OpenSubtitles, MusicBrainz, LRCLIB, Twitch и др.; Trakt и AcoustID — только по opt-in | privacy-policy §4 |
| Синхронизация — LAN, папка пользователя или сервер пользователя; iroh шифрует соединение от устройства до сервера | S §2 |

## 1. App Store — App Privacy («nutrition label»)

**Определения Apple** ([App privacy details](https://developer.apple.com/app-store/app-privacy-details/)):
- *collect* — передача данных с устройства так, что **вы и/или ваши third-party partners** получают к ним доступ дольше, чем нужно для обработки запроса в реальном времени;
- *third-party partners* — аналитика, рекламные сети, сторонние SDK и **другие внешние поставщики, чей код вы добавили** в приложение.

**Рекомендуемый ответ:** «**Data Not Collected**» (данные не собираются).

Обоснование:
- Разработчик никаких данных не получает: серверов нет.
- SDK третьих сторон, то есть «third-party partners» в смысле Apple, в приложении нет. TMDB, Twitch и другие сервисы вызываются нашим собственным кодом по запросу пользователя, их код в приложение не встроен.
- Crash-отчёты пользователь отправляет сам, вручную, через GitHub, выбирая это каждый раз. Сбор не регулярный и не автоматический.
- Данные, которые собирает Apple (например, «Share with App Developers»), разработчик не декларирует: «You are not responsible for disclosing data collected by Apple». Если владелец начнёт **использовать** крэш-логи из App Store Connect/Xcode Organizer, вопрос стоит перепроверить **[проверить]**.

**Консервативный вариант** — если ревьюер Apple считает провайдеров метаданных «партнёрами» **[проверить]**:

| Категория → тип | Связано с пользователем | Трекинг | Цель |
|---|---|---|---|
| Search History → Search History | Нет | Нет | App Functionality |

Остальные поля: **Tracking — No** (App Tracking Transparency не нужен), **Privacy Policy URL** — ссылка на опубликованную `privacy-policy` (сайт или GitHub), **Account deletion** — не применимо (аккаунтов нет).

Дополнительно для App Store:
- **Privacy manifest** (`PrivacyInfo.xcprivacy`): `NSPrivacyTracking = false`, `NSPrivacyTrackingDomains` пусто, `NSPrivacyCollectedDataTypes` пусто (или как в консервативном варианте). Причины использования **Required Reason API** (UserDefaults, file timestamp, disk space, system boot time) указать по факту кода, включая зависимости (Flutter-плагины, MPVKit). Это задача сборки iOS/tvOS/macOS **[проверить при первой сборке]**.
- Строки разрешений: `NSLocalNetworkUsageDescription` и `NSBonjourServices` (mDNS, SMB, наш сервер) — объяснить, что приложение ищет медиасерверы и устройства в домашней сети.

## 2. Google Play — Data safety

**Определения Google** ([Data safety](https://support.google.com/googleplay/android-developer/answer/10787469)):
- *collect* — передача данных **с устройства пользователя** (без оговорки «разработчику»);
- *share* — передача собранных данных третьей стороне. Не считается sharing: передача сервис-провайдеру, по действию пользователя (user-initiated) и т. п.;
- исключения из collection: обработка только на устройстве и сквозное шифрование (end-to-end).

Определение у Google шире, чем у Apple: запросы к TMDB и другим сервисам формально «передача с устройства». Поэтому для Play **рекомендуется консервативный ответ**.

### 2.1. Рекомендуемые ответы

| Вопрос | Ответ |
|---|---|
| Does your app collect or share any of the required user data types? | **Yes** (консервативно; см. 2.3) |
| Is all of the user data collected by your app encrypted in transit? | **Yes** — HTTPS/TLS. **[проверить]**: все ли провайдеры принимают только HTTPS; SMB/NFS в LAN идут к собственным устройствам пользователя, это не «collection» |
| Do you provide a way for users to request that their data is deleted? | Аккаунтов нет, данные лежат на устройстве: удаление через очистку данных приложения или его удаление. В форме выбрать вариант, который позволяет указать, что данные не хранятся разработчиком **[проверить формулировку в консоли]** |
| Privacy policy URL | Опубликованная `privacy-policy` |
| Committed to Play Families Policy | Нет (приложение не для детей) |

### 2.2. Типы данных (консервативно)

| Тип данных (Play) | Collected | Shared | Ephemeral | Обязательно / по выбору | Цель | Откуда |
|---|---|---|---|---|---|---|
| App activity → **In-app search history** | Yes | No (user-initiated / сервис-провайдер) | Yes **[проверить]** | Required (без поиска нет распознавания) | App functionality | Запросы к TMDB, AniList, Shikimori и др. |
| App activity → **Other actions** | Yes | No (user-initiated) | No | Optional | App functionality | Trakt: история просмотров в аккаунт пользователя, только если он подключил Trakt |
| Messages → **Other in-app messages** | Yes | No (user-initiated) | Yes | Optional | App functionality | Сообщения в чат Twitch, которые отправляет пользователь |
| Personal info → **User IDs** | Yes | No (user-initiated) | Yes | Optional | App functionality, Account management | Логин или токен пользователя в Twitch, Trakt, OpenSubtitles, AniDB — передаются только самому сервису при входе |
| Audio → **Other audio files** **[проверить]** | Yes | No | Yes | Optional | App functionality | Акустический отпечаток для AcoustID — только при opt-in. Если фича в store-сборке выключена, строку убрать |

**Не декларируем** (и почему):
- *App info and performance → Crash logs / Diagnostics*: приложение само ничего не отправляет. Пользователь вручную прикладывает файл к issue на GitHub — это его действие вне приложения.
- *Location*: геолокация не запрашивается. IP-адрес виден сервисам, но не используется для определения местоположения.
- *Device or other IDs*: рекламный ID и Android ID не используются.
- *Financial info*: IAP-«чаевые» обрабатывает Google Play Billing, приложение платёжных данных не видит.
- Синхронизация через iroh: шифрование от устройства до сервера (исключение end-to-end). LAN и папки пользователя — собственная инфраструктура пользователя.

### 2.3. Альтернатива «No data collected»

Её применяют многие open source-клиенты: разработчик данных не получает, а запросы к API идут по прямому действию пользователя. Риск в том, что формально под определение Google такие запросы попадают, и при проверке Google может потребовать исправить форму. Консервативный вариант ничем не ограничивает публикацию, поэтому рекомендуем его. Окончательно выбирает владелец **[проверить]**.

## 3. Чек-лист перед отправкой в сторы

- [ ] Имя из DEC-004 подставлено в политику вместо «project_alpha».
- [ ] Политика опубликована по стабильному URL (сайт SITE или GitHub), не в PDF.
- [ ] Добавлен контактный email для вопросов приватности (нужен Google Play).
- [ ] Ссылка на политику есть в экране «О программе» (UI-016) и в карточках сторов.
- [ ] Список сервисов в §4 политики совпадает с фактическим кодом store-сборки: каждый новый сетевой провайдер сначала добавляется в политику.
- [ ] `PrivacyInfo.xcprivacy` собран и проверен (Xcode → Privacy Report).

---

## English summary

Draft answers for the App Store "App Privacy" label and the Google Play Data safety form for **store builds** (not legal advice; items marked **[проверить]** need owner confirmation at submission time).

- **App Store:** recommended **"Data Not Collected"**. The developer receives no data; there are no third-party SDKs ("third-party partners" in Apple's definition means code you add to the app); crash reports are exported manually by the user; data collected by Apple itself is not the developer's to disclose. Conservative alternative: *Search History — not linked to user, no tracking, App Functionality*. Tracking: No. Also ship a `PrivacyInfo.xcprivacy` with no tracking and correct Required Reason API entries.
- **Google Play:** Google defines "collect" as any transmission off the device, so the recommended conservative answer declares: In-app search history (metadata lookups), Other actions (Trakt, opt-in), Other in-app messages (Twitch chat), User IDs (sign-in to Twitch/Trakt/OpenSubtitles/AniDB), and possibly the AcoustID fingerprint (opt-in). All are for App functionality, none are shared (user-initiated), and all are encrypted in transit. Crash logs, location, device IDs and financial info are not declared. "No data collected" is a possible but riskier alternative.
