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
# Product filename parser (META-005/META-006): parse metrics only, no work matching.
"$bin/pa-eval" run --recognizer parser --json "$out/parser.json" --md "$out/parser.md" "$corpus"/*.jsonl
"$bin/pa-eval" run --recognizer naive --tape "$out/tmdb-search.tape.jsonl" \
    --json "$out/naive.json" --md "$out/naive.md" "$corpus"/*.jsonl
"$bin/pa-eval" run --recognizer naive --tape "$out/tmdb-search.tape.jsonl" --threshold 0.8 \
    --json "$out/naive-t080.json" --md "$out/naive-t080.md" "$corpus"/*.jsonl

# META-007: candidate recall of the local title index, no provider calls.
# Needs the bundled anime-offline-database (data/anime-mappings/build.sh).
python3 "$here/catalog.py" > "$out/title-catalog.jsonl"
aod="${AOD_BUNDLE:-$root/data/anime-mappings/dist/anime-offline-ids.jsonl}"
if [ -f "$aod" ]; then
    "$bin/pa-eval" candidates --anime "$aod" --catalog "$out/title-catalog.jsonl" \
        --json "$out/candidates.json" --md "$out/candidates.md" "$corpus/train.jsonl" "$corpus/dev.jsonl"
    "$bin/pa-eval" candidates --anime "$aod" --catalog "$out/title-catalog.jsonl" \
        --json "$out/candidates-test.json" --md "$out/candidates-test.md" "$corpus/test.jsonl"
    # META-026: scorer v0 over the same index — work, franchise entry, episode.
    "$bin/pa-eval" run --recognizer scorer --anime "$aod" --catalog "$out/title-catalog.jsonl" \
        --json "$out/scorer.json" --md "$out/scorer.md" "$corpus/train.jsonl" "$corpus/dev.jsonl"
    "$bin/pa-eval" run --recognizer scorer --anime "$aod" --catalog "$out/title-catalog.jsonl" \
        --json "$out/scorer-test.json" --md "$out/scorer-test.md" "$corpus/test.jsonl"
else
    echo "skipping candidate and scorer reports: $aod not found (run data/anime-mappings/build.sh)" >&2
fi
