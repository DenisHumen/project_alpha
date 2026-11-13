#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Regenerate corpus v0 and the pa-eval baseline reports (META-002).
# Offline: uses facts.json, no network. Run from anywhere:
#   tools/recognition-eval/corpus/gen/baselines.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
corpus="$here/../v0"
out="$here/../../baselines/v0"
root="$(cd "$here/../../../.." && pwd)"
mkdir -p "$out"

python3 "$here/generate.py" --out-dir "$corpus"
cargo build --quiet --release --manifest-path "$root/Cargo.toml" -p pa-eval -p pa-recognition-eval
bin="${CARGO_TARGET_DIR:-$root/target}/release"

"$bin/corpus-validate" --quiet --min-per-category 50 "$corpus"/*.jsonl

misses="$(mktemp)"
trap 'rm -f "$misses"' EXIT
"$bin/pa-eval" run --recognizer naive --misses "$misses" --json /dev/null "$corpus"/*.jsonl
python3 "$here/tape.py" "$misses" > "$out/tmdb-search.tape.jsonl"

"$bin/pa-eval" run --recognizer abstain --json "$out/abstain.json" --md "$out/abstain.md" "$corpus"/*.jsonl
"$bin/pa-eval" run --recognizer naive --tape "$out/tmdb-search.tape.jsonl" \
    --json "$out/naive.json" --md "$out/naive.md" "$corpus"/*.jsonl
"$bin/pa-eval" run --recognizer naive --tape "$out/tmdb-search.tape.jsonl" --threshold 0.8 \
    --json "$out/naive-t080.json" --md "$out/naive-t080.md" "$corpus"/*.jsonl
