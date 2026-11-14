# pa-frb

Фасад ядра для Dart на [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge) 2.13 (ADR-0001, ADR-0013). Dart-пакет — [`clients/flutter/packages/pa_core`](../../../clients/flutter/packages/pa_core); Kotlin и Swift используют [`pa-ffi`](../pa-ffi) (UniFFI).

- API — модули `src/api/<domain>.rs`: `version` (`core_version`), `events` (тестовый Stream), `recognition` (`parse_filename`, META-005), `sources` (`CoreSources`: менеджер источников, NET-011).
- `src/frb_generated.rs` генерируется `just core frb` и коммитится; руками не править.
- `rust-toolchain.toml` рядом с крейтом нужен build hook'у Native Assets: тот же `channel`, что в корне, плюс список целей.

```bash
cargo test -p pa-frb
just core frb-check
```
