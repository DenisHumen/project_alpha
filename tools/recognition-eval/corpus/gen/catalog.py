#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Offline stand-in for the TMDB title cache (META-007 candidate eval).

Writes one `TitleRecord` (pa_recognition::title) per movie, series and daily
show of facts.json: ids, kind, year and the titles a cached TMDB entry would
carry (original/English title and the ru-RU translation). Anime is not
written here: it comes from the bundled anime-offline-database.

Like tape.py this is NOT a TMDB recording (the project has no TMDB key yet,
DEC-008); it only holds the corpus works, so retrieval among movies and
shows is easier than against a real, larger cache. Re-record from the TMDB
client (pa_metadata::tmdb::titles::title_record) once a key exists.

Usage: catalog.py [--facts facts.json] > title-catalog.jsonl
"""

import argparse
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def records(facts):
    for section, kind in (("movies", "movie"), ("shows", "series"), ("daily", "series")):
        for w in facts[section]:
            ids = {}
            for ns in ("tmdb", "tvdb", "imdb", "kinopoisk", "tvmaze"):
                if w.get(ns) is not None:
                    ids[ns] = str(w[ns])
            if not ids:
                continue
            titles = []
            for t in (w.get("title"), w.get("label_en"), w.get("label_ru")):
                if t and t not in titles:
                    titles.append(t)
            rec = {"ids": ids, "kind": kind}
            if w.get("year"):
                rec["year"] = w["year"]
            rec["titles"] = titles
            yield rec


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--facts", default=str(HERE / "facts.json"))
    args = ap.parse_args()
    facts = json.loads(pathlib.Path(args.facts).read_text(encoding="utf-8"))
    out = sorted(records(facts), key=lambda r: (r["kind"], r["ids"].get("tmdb", ""), r["titles"][0]))
    seen = set()
    for rec in out:
        line = json.dumps(rec, ensure_ascii=False, sort_keys=False)
        if line not in seen:
            seen.add(line)
            sys.stdout.write(line + "\n")


if __name__ == "__main__":
    main()
