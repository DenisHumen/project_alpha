<!-- SPDX-License-Identifier: MPL-2.0 -->

# Обнаружение в LAN против Samba с Avahi (NET-020)

`Dockerfile.samba-avahi` — Samba с гостевой шарой `media` и Avahi, который анонсирует её как `_smb._tcp` с именем экземпляра `NET-020 Samba`. Контейнер живёт в отдельной docker-сети: хост подключён к её мосту, поэтому mDNS-демон ядра на хосте (все интерфейсы, порт 5353) видит анонс, а в настоящую LAN ничего не уходит.

```bash
docker build -t pa-net020-samba:test -f core/crates/pa-discovery/tests/servers/Dockerfile.samba-avahi .
docker network create net020
docker run -d --name net020-samba --network net020 pa-net020-samba:test

# Rust: CoreDiscovery (pa-frb) находит шару и строит smb://<IP>/
PA_LAN_SMB_INSTANCE='NET-020 Samba' cargo test -p pa-frb -- --ignored lan_samba --nocapture

# Уборка (томов нет)
docker rm -f net020-samba && docker network rm net020 && docker rmi pa-net020-samba:test
```

Эмулятор Android (сеть QEMU user-mode, SLIRP) не получает multicast из docker-сети хоста, поэтому на эмуляторе эта шара не видна; приложение целиком проверяется Linux-сборкой под Xvfb на той же машине.
