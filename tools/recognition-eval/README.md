<!-- SPDX-License-Identifier: MPL-2.0 -->
# recognition-eval — формат корпуса распознавания

Формат тестового корпуса для оценки распознавания фильмов, сериалов и аниме (задача META-001, исследование [03 §6](../../docs/research/03-metadata-recognition.md)). Здесь лежат спецификация, инструменты и корпус v0 ([`corpus/`](corpus/README.md), META-002).

| Файл | Что это |
|---|---|
| [`GUIDE.md`](GUIDE.md) | гайд по разметке: что такое кейс, роли файлов, схемы нумерации, правила спорных случаев |
| [`schema/corpus-record.v1.schema.json`](schema/corpus-record.v1.schema.json) | JSON Schema (draft 2020-12) одной строки корпуса |
| [`schema/taxonomy.v1.json`](schema/taxonomy.v1.json) | таксономия трудных случаев A1–A17 |
| [`examples/examples.jsonl`](examples/examples.jsonl) | 54 собственных примера (MPL-2.0), каждая категория встречается ≥ 3 раз |
| `src/` | крейт `pa-recognition-eval`: библиотека валидации и CLI `corpus-validate` |
| [`corpus/`](corpus/README.md) | корпус v0: 2 035 групп файлов, генератор и сверка ID (META-002) |
| [`baselines/v0/`](baselines/v0/README.md) | отчёты бейзлайнов `pa-eval` по корпусу v0 |

## Формат в двух словах

JSONL, одна строка — один **кейс** (дерево файлов + разметка):

```text
{ schema_version, id, categories: ["A1","A6"], split, source{kind,license},
  input:  { files: [ {path, size, duration_ms, audio_langs, sub_langs, oshash, ed2k, crc32, embedded_tags, video} ] },
  labels: { works: [ {ref, kind, anime_type, title, year, ids{tmdb,anidb,tvdb,imdb,...}} ],
            files: [ {path, unit, role, work, episodes[{type, numbering{anidb,tmdb,tvdb_aired,tvdb_dvd,tvdb_absolute,tmdb_group}, air_date}],
                      part, extra_type, edition, parse{title, year, season, episodes, absolute, release_group, crc32, ...}} ] },
  review: { status: draft|verified|disputed, annotators, double_labelled } }
```

Подробности и правила — в [GUIDE.md](GUIDE.md).

## Проверка

```bash
cd tools/recognition-eval
cargo run --bin corpus-validate -- examples/examples.jsonl
cargo run --bin corpus-validate -- --min-per-category 50 corpus/v0/*.jsonl   # корпус v0 (META-002)
cargo test
```

Валидатор проверяет JSON Schema, а также то, что схема выразить не может: каждый входной файл покрыт ровно одной меткой, ссылки на работы корректны, у аниме размечены две схемы нумерации, части пронумерованы 1..N, пути не содержат личных данных, каждая категория действительно содержит свой трудный случай. Предупреждения (недостающие ID и схемы) допустимы в `draft` и становятся ошибками в `verified` и с `--strict`. Код выхода: 0 — ок, 1 — ошибки, 2 — неверные аргументы или ввод-вывод.

Та же схема проверяется любым валидатором JSON Schema 2020-12 (например, Python-пакетом `jsonschema`), если нужна проверка без Rust.

## Примеры

`examples/examples.jsonl` — иллюстрации формата, написанные вручную, `status: draft`. **Внешние ID и номера эпизодов в них не сверены с провайдерами** и не являются эталоном: часть ID оставлена пустой намеренно (валидатор выдаёт предупреждения). Перед переносом в корпус каждый пример проверяется по §9 гайда.

Тест-кейсы guessit (LGPL), Sonarr (GPL), Jellyfin (GPL) сюда не копируются.

## Где живёт корпус

По [ADR-0011](../../docs/adr/0011-maintainer-decisions-release-0-1.md) п. 8 корпус пока лежит в этом репозитории ([`corpus/`](corpus/README.md)) и содержит только собственные данные проекта (MPL-2.0). Отдельный eval-репозиторий понадобится, когда появятся данные с несовместимыми лицензиями (тест-кейсы чужих парсеров, дампы библиотек волонтёров) и закрытый test-сплит.
