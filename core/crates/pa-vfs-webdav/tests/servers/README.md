# Контракт-тесты WebDAV против реальных серверов

Общий контракт-тест-сьют `pa_vfs::contract` (ADR-0013) против четырёх серверов в Docker:

| Сервис | Образ | Путь | Авторизация |
|---|---|---|---|
| nginx + `dav-ext` | `alpine:3.22` + `nginx-mod-http-dav-ext` | `/dav/` | Basic |
| Apache `mod_dav` | `httpd:2.4-alpine` | `/dav/` | Digest (MD5, `qop=auth`) |
| `rclone serve webdav` | `rclone/rclone:1.71` | `/` | Basic |
| Nextcloud | `nextcloud:31-apache` (SQLite) | `/remote.php/dav/files/<user>/` | Basic |

Запуск (нужны Docker с плагином compose и cargo):

```bash
core/crates/pa-vfs-webdav/tests/servers/run.sh            # все серверы
core/crates/pa-vfs-webdav/tests/servers/run.sh -- nginx   # один
```

Скрипт кладёт фикстуру в `/media/контракт` (рядом с посторонним файлом), генерирует файлы паролей во временном каталоге, поднимает контейнеры (порты 18081–18084 только на `127.0.0.1`), копирует фикстуру в Nextcloud и выполняет `occ files:scan`, затем запускает `cargo test -p pa-vfs-webdav --features server-tests --test contract_servers` и удаляет контейнеры. Логин и пароль — одноразовые для этих контейнеров.

Для каждого сервера проходят все кейсы контракта и проверка, что анонимный доступ отклоняется (значит, тесты прошли через авторизацию). Без фичи `server-tests` эти тесты не компилируются; в обычном `cargo test` контракт гоняется против встроенного тестового сервера (`tests/contract_mock.rs`). Подключение к CI — задача CI-012.
