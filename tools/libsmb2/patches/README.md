<!-- SPDX-License-Identifier: MPL-2.0 -->

# Патчи к исходникам libsmb2

Патч кладётся в `patches/libsmb2/NNNN-short-description.patch` (формат `git format-patch`,
`-p1` от корня исходников). `build.sh` применяет их по алфавиту после проверки SHA-256 архива.

Все патчи публичны (условие LGPL, см. `docs/research/02-client-stack-playback.md` §3.1),
перечисляются в `BUILDINFO.txt` артефакта и входят в архив `build.sh sources`.
Каждый патч начинается с описания: зачем он нужен и отправлен ли в upstream.

Сейчас патчей нет.
