#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build an offline `tmdb/search` tape for the pa-eval baselines.

    pa-eval run --misses misses.jsonl ../v0/*.jsonl      # requests the recognizer makes
    python3 tape.py misses.jsonl > ../../baselines/v0/tmdb-search.tape.jsonl

This is NOT a recording of TMDB (the project has no key yet, DEC-008). It is a
stand-in search over the titles this corpus knows (facts.json + titles.yaml),
built so the baseline exercises identification metrics, including silent
errors on homonyms:

  * a candidate matches when the pa-eval-normalized query equals one of its
    names (English/original title, Wikidata en/ru labels, transliteration,
    the release names from titles.yaml);
  * with a `year` parameter only candidates of that year are returned (TMDB
    filters by year the same way);
  * otherwise candidates are ordered newest first, a crude stand-in for
    TMDB's popularity order;
  * results carry TMDB/IMDb ids only, as a TMDB search would.

Requests without a match get an empty result list, so the tape has no misses.
"""

from __future__ import annotations

import json
import os
import sys
import unicodedata

import yaml

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from generate import clean, dotted, translit, undot  # noqa: E402


def normalize(s: str) -> str:
    # Mirrors pa_eval::recognizer::normalize (alphanumerics, lowercased).
    return "".join(c.lower() for c in s if c.isalnum())


def variants(name):
    if not name:
        return set()
    out = {name, clean(name), undot(dotted(name))}
    if any("а" <= c.lower() <= "я" or c.lower() == "ё" for c in name):
        out |= {translit(clean(name)), undot(dotted(translit(clean(name))))}
    return {normalize(unicodedata.normalize("NFC", v)) for v in out if v}


def index(facts, titles):
    idx = []
    for sec, kind in (("movies", "movie"), ("shows", "show"), ("daily", "show")):
        for r in facts[sec]:
            if not r.get("tmdb") or r.get("problems"):
                continue
            ids = {"tmdb": r["tmdb"]} | ({"imdb": r["imdb"]} if r.get("imdb") else {})
            names = set()
            for n in (r["title"], r.get("label_en"), r.get("label_ru")):
                names |= variants(n)
            idx.append({"kind": kind, "title": r["title"], "year": r["year"], "ids": ids, "names": names})
    anime = facts["anime"]
    for fr in titles["anime"]:
        tm = None
        for e in fr["entries"]:
            f = anime.get(str(e["aid"]), {})
            if f.get("tmdb", "").startswith("tv:"):
                tm = tm or f["tmdb"]
        if not tm:
            continue
        first = anime.get(str(fr["entries"][0]["aid"]), {})
        names = set()
        for e in fr["entries"]:
            for n in list(fr["names"].values()) + list(e.get("names", {}).values()):
                names |= variants(n)
        idx.append({"kind": "show", "title": first.get("title", fr["key"]), "year": first.get("year"),
                    "ids": {"tmdb": tm}, "names": names})
    for m in titles["anime_movies"]:
        f = anime.get(str(m["aid"]), {})
        if not f.get("tmdb", "").startswith("movie:"):
            continue
        names = set()
        for n in list(m["names"].values()) + [f.get("title")]:
            names |= variants(n)
        idx.append({"kind": "movie", "title": f["title"], "year": f.get("year"),
                    "ids": {"tmdb": f["tmdb"]} | ({"imdb": f["imdb"]} if f.get("imdb") else {}), "names": names})
    return idx


def main():
    misses = sys.argv[1]
    facts = json.load(open(os.path.join(HERE, "facts.json"), encoding="utf-8"))
    titles = yaml.safe_load(open(os.path.join(HERE, "titles.yaml"), encoding="utf-8"))
    idx = index(facts, titles)
    lines = []
    for line in open(misses, encoding="utf-8"):
        if not line.strip():
            continue
        req = json.loads(line)
        if req["provider"] != "tmdb" or req["endpoint"] != "search":
            continue
        q = normalize(req["params"].get("query", ""))
        year = req["params"].get("year")
        hits = [w for w in idx if q and q in w["names"]]
        if year:
            hits = [w for w in hits if str(w["year"]) == year]
        hits.sort(key=lambda w: (-(w["year"] or 0), w["ids"]["tmdb"]))
        seen, results = set(), []
        for w in hits:
            if w["ids"]["tmdb"] in seen:
                continue
            seen.add(w["ids"]["tmdb"])
            results.append({"kind": w["kind"], "title": w["title"], "year": w["year"], "ids": w["ids"]})
        lines.append((json.dumps(req, ensure_ascii=False, separators=(",", ":")),
                      {"request": req, "response": {"results": results[:10]}}))
    lines.sort(key=lambda kv: kv[0])
    for _, obj in lines:
        sys.stdout.write(json.dumps(obj, ensure_ascii=False, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
