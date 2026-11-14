# pa-remote-config

Удалённый конфиг без серверов (CORE-014, ADR-0004, ADR-NNNN): `config/remote-config.json` из публичного репо, подпись minisign встроенным ключом ([`minisign-verify`](https://crates.io/crates/minisign-verify)), ETag, проверенный кэш на диске, встроенная копия (`builtin/remote-config.json`), anti-rollback по `serial`, типизированные флаги и kill-switch'и, значения для Twitch (TWI-004). Фасад для Dart — `CoreRemoteConfig` в `pa-frb`.

Пока `PRODUCTION_PUBLIC_KEY` — заглушка, клиент работает только на встроенной копии. Как владельцу создать ключ и подписать конфиг — [docs/dev/remote-config.md](../../../docs/dev/remote-config.md).

```bash
cargo test -p pa-remote-config
just core remote-config-verify            # проверить config/**/*.json как приложение
just core remote-config-release-check     # падает, пока ключ — заглушка
```
