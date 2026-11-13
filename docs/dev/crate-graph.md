# Граф крейтов workspace

> **Файл генерируется.** Блок между маркерами пересобирает `just docs::crate-graph`
> (скрипт [`scripts/crate_graph.py`](scripts/crate_graph.py) читает `cargo metadata`).
> CI ([`docs.yml`](../../.github/workflows/docs.yml)) запускает `just docs::crate-graph-check` и падает, если граф устарел.
> Руками блок не правят: добавили крейт или зависимость между крейтами — перегенерируйте и закоммитьте.

Показаны только связи между крейтами workspace: сплошная стрелка — обычная зависимость, пунктир — `dev` или `build`. Внешние крейты только подсчитаны в таблице. Целевой (запланированный) состав ядра — в [architecture.md §4](architecture.md#4-ядро-целевой-состав-крейтов).

<!-- BEGIN GENERATED: crate-graph -->

```mermaid
flowchart LR
  subgraph g0["core/crates — ядро"]
    pa_anime_mappings["pa-anime-mappings"]
    pa_ffi["pa-ffi"]
    pa_frb["pa-frb"]
    pa_model["pa-model"]
    pa_recognition["pa-recognition"]
    pa_runtime["pa-runtime"]
    pa_vfs["pa-vfs"]
  end
  subgraph g1["tools — инструменты"]
    pa_eval["pa-eval"]
    pa_recognition_eval["pa-recognition-eval"]
  end
  pa_eval --> pa_recognition_eval
  pa_ffi --> pa_runtime
  pa_frb --> pa_ffi
  pa_frb --> pa_recognition
```

| Крейт | Каталог | Внешних зависимостей | Описание |
|---|---|---|---|
| `pa-anime-mappings` | `core/crates/pa-anime-mappings` | 4 | Anime cross-ID and episode mappings: bundled ODbL data plus runtime-downloaded upstream lists (META-008) |
| `pa-eval` | `tools/crates/pa-eval` | 2 | Offline eval runner for the recognition corpus: metrics, reports, regression check (META-003) |
| `pa-ffi` | `core/crates/pa-ffi` | 3 | UniFFI facade of the project_alpha core: one API for Kotlin and Swift |
| `pa-frb` | `core/crates/pa-frb` | 1 | flutter_rust_bridge facade of the project_alpha core for the Dart package pa_core |
| `pa-model` | `core/crates/pa-model` | 1 | Domain types of the project_alpha core: media entities, sync keys, playback state |
| `pa-recognition` | `core/crates/pa-recognition` | 1 | Media recognition core (sans-IO): filename parser with N-best hypotheses |
| `pa-recognition-eval` | `tools/recognition-eval` | 2 | Recognition test-corpus format: JSON Schema, taxonomy and validator (META-001) |
| `pa-runtime` | `core/crates/pa-runtime` | 6 | Core runtime of project_alpha: async runtime, lifecycle, data directories, settings store |
| `pa-vfs` | `core/crates/pa-vfs` | 0 | Virtual file system of media sources: Vfs and ByteSource traits, local backend, backend contract tests |

<!-- END GENERATED: crate-graph -->
