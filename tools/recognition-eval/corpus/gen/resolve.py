#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Resolve and cross-check external ids for the hand-curated title list.

    python3 resolve.py --aod anime-offline-database.jsonl [--cache DIR] [--out facts.json]

Sources (all open, no API key):
  * Wikidata (CC0)                      TMDB/IMDb/TVDB/TVmaze/Kinopoisk ids, labels, year, runtime
  * TVmaze API (data CC BY-SA 4.0)      episode lists (season, number, air date, runtime)
  * anime-offline-database (ODbL 1.0)   AniDB/MAL/AniList/Kitsu ids, type, episode count

The anime-offline-database file is the pinned release from
data/anime-mappings/upstream.env (check its SHA-256 before use). Raw responses
are cached in --cache and never committed; only the small set of facts the
generator needs goes to facts.json. Every work gets a list of `checks` that
says which source confirmed which field, and `problems` for anything that did
not line up; the generator refuses works with problems.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
import unicodedata
import urllib.parse
import urllib.request

import yaml

UA = "kinolune-recognition-corpus/0.1 (open-source test corpus; https://github.com/DenisHumen/project_alpha)"
WD_API = "https://www.wikidata.org/w/api.php"
TVMAZE = "https://api.tvmaze.com"

P_TMDB_MOVIE, P_TMDB_TV, P_IMDB, P_TVDB, P_TVMAZE = "P4947", "P4983", "P345", "P4835", "P8600"
P_KINOPOISK, P_ANIDB, P_MAL, P_PUB, P_START, P_DURATION = "P2603", "P5646", "P4086", "P577", "P580", "P2047"


class Fetcher:
    def __init__(self, cache: str):
        self.cache = cache
        os.makedirs(cache, exist_ok=True)
        self.last = {}

    def get(self, url: str, host_delay: float = 0.3):
        key = hashlib.sha256(url.encode()).hexdigest()[:24]
        path = os.path.join(self.cache, key + ".json")
        if os.path.exists(path):
            with open(path, encoding="utf-8") as f:
                return json.load(f)
        host = urllib.parse.urlparse(url).netloc
        wait = self.last.get(host, 0) + host_delay - time.time()
        if wait > 0:
            time.sleep(wait)
        for attempt in range(5):
            try:
                req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "application/json"})
                with urllib.request.urlopen(req, timeout=60) as r:
                    data = json.loads(r.read().decode("utf-8"))
                break
            except urllib.error.HTTPError as e:
                if e.code == 404:
                    data = None
                    break
                if e.code in (429, 500, 502, 503, 504):
                    time.sleep(2 + 3 * attempt)
                    continue
                raise
            except (urllib.error.URLError, TimeoutError):
                time.sleep(2 + 3 * attempt)
        else:
            raise RuntimeError(f"giving up on {url}")
        self.last[host] = time.time()
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f)
        return data


def norm(s: str) -> str:
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    return re.sub(r"[^0-9a-zа-яё]+", "", s.casefold())


def claim_values(entity: dict, prop: str) -> list:
    out = []
    for c in entity.get("claims", {}).get(prop, []):
        if c.get("rank") == "deprecated":
            continue
        dv = c.get("mainsnak", {}).get("datavalue")
        if not dv:
            continue
        v = dv["value"]
        out.append(v)
    return out


def claim_str(entity, prop):
    vals = [v for v in claim_values(entity, prop) if isinstance(v, str)]
    return vals[0] if vals else None


def years(entity) -> set[int]:
    ys = set()
    for p in (P_PUB, P_START):
        for v in claim_values(entity, p):
            if isinstance(v, dict) and "time" in v:
                m = re.match(r"[+-](\d{4})", v["time"])
                if m:
                    ys.add(int(m.group(1)))
    return ys


def duration_min(entity):
    for v in claim_values(entity, P_DURATION):
        if isinstance(v, dict) and "amount" in v:
            unit = v.get("unit", "")
            amount = float(v["amount"])
            if unit.endswith("Q7727"):  # minute
                return round(amount)
            if unit.endswith("Q11574"):  # second
                return round(amount / 60)
            if unit.endswith("Q25235"):  # hour
                return round(amount * 60)
    return None


def label(entity, lang):
    labels = entity.get("labels", {})
    v = labels.get(lang, {}).get("value")
    if v is None and lang == "en":
        # Wikidata moved many language-independent names to the "mul" label.
        v = labels.get("mul", {}).get("value")
    return v


def aliases(entity, lang):
    al = entity.get("aliases", {})
    langs = [lang, "mul"] if lang == "en" else [lang]
    return [a["value"] for lg in langs for a in al.get(lg, [])]


class Wikidata:
    def __init__(self, fx: Fetcher):
        self.fx = fx

    def search(self, text: str, limit=10) -> list[str]:
        q = urllib.parse.urlencode({"action": "query", "list": "search", "srsearch": text,
                                    "srlimit": limit, "format": "json"})
        data = self.fx.get(f"{WD_API}?{q}")
        return [h["title"] for h in data.get("query", {}).get("search", [])]

    def entities(self, qids: list[str]) -> dict:
        if not qids:
            return {}
        q = urllib.parse.urlencode({"action": "wbgetentities", "ids": "|".join(qids),
                                    "props": "claims|labels|aliases", "languages": "en|mul|ru|ja",
                                    "format": "json"})
        return self.fx.get(f"{WD_API}?{q}").get("entities", {})


def pick(wd: Wikidata, title: str, year: int, prop: str, qid: str | None, problems: list):
    if qid:
        ents = wd.entities([qid])
        return ents.get(qid)
    hits = wd.search(f'"{title}" haswbstatement:{prop}')
    if not hits:
        hits = wd.search(f"{title} haswbstatement:{prop}")
    ents = wd.entities(hits)
    exact, near = [], []
    for qid_ in hits:
        e = ents.get(qid_)
        if not e:
            continue
        names = [label(e, "en") or ""] + aliases(e, "en") + [label(e, "ru") or ""]
        if not any(norm(n) == norm(title) for n in names if n):
            continue
        ys = years(e)
        if year in ys:
            exact.append(e)
        elif any(abs(year - y) <= 1 for y in ys):
            near.append(e)
    cands = exact or near
    if len(cands) > 1:
        # several items with the same title and year: keep the one with the most sitelinks-free claims
        cands.sort(key=lambda e: -len(e.get("claims", {})))
        problems.append(f"ambiguous wikidata match: {[e['id'] for e in cands]}")
        return None
    if not cands:
        problems.append("no wikidata match")
        return None
    return cands[0]


def tvmaze_episodes(fx: Fetcher, tvmaze_id: int):
    eps = fx.get(f"{TVMAZE}/shows/{tvmaze_id}/episodes", host_delay=0.6) or []
    out = []
    for e in eps:
        if e.get("type") != "regular" or e.get("season") is None or e.get("number") is None:
            continue
        out.append([e["season"], e["number"], e.get("airdate") or "", e.get("runtime") or 0])
    return out


def resolve_video(wd, fx, item, kind, problems, daily=False):
    prop = P_TMDB_MOVIE if kind == "movie" else P_TMDB_TV
    e = pick(wd, item["t"], item["y"], prop, item.get("wd"), problems)
    if e is None:
        return None
    ys = years(e)
    year = item["y"] if item["y"] in ys or not ys else min(ys, key=lambda y: (abs(y - item["y"]), y))
    if abs(year - item["y"]) > 1:
        problems.append(f"year {item['y']} not in wikidata {sorted(ys)}")
    rec = {"qid": e["id"], "title": item["t"], "year": year, "name_year": item["y"],
           "label_en": label(e, "en"), "label_ru": item.get("ru") or label(e, "ru"),
           "checks": []}
    tmdb = claim_str(e, prop)
    if tmdb:
        rec["tmdb"] = f"{'movie' if kind == 'movie' else 'tv'}:{tmdb}"
        rec["checks"].append("tmdb:wikidata")
    else:
        problems.append("wikidata item has no tmdb id")
    imdb = claim_str(e, P_IMDB)
    if imdb:
        rec["imdb"] = imdb
    kp = claim_str(e, P_KINOPOISK)
    if kp and kp.isdigit():
        rec["kinopoisk"] = int(kp)
    if kind == "movie":
        d = duration_min(e)
        if d:
            rec["runtime_min"] = d
        return rec
    tvdb = claim_str(e, P_TVDB)
    if tvdb and tvdb.isdigit():
        rec["tvdb"] = f"series:{tvdb}"
    tvmaze = claim_str(e, P_TVMAZE)
    show = None
    if tvmaze and tvmaze.isdigit():
        show = fx.get(f"{TVMAZE}/shows/{tvmaze}", host_delay=0.6)
    elif imdb:
        show = fx.get(f"{TVMAZE}/lookup/shows?imdb={imdb}", host_delay=0.6)
    if not show:
        problems.append("no tvmaze show")
        return rec
    rec["tvmaze"] = show["id"]
    ext = show.get("externals") or {}
    if imdb and ext.get("imdb") and ext["imdb"] != imdb:
        problems.append(f"imdb mismatch wikidata {imdb} vs tvmaze {ext['imdb']}")
    else:
        rec["checks"].append("imdb:wikidata+tvmaze" if ext.get("imdb") else "imdb:wikidata")
    if ext.get("thetvdb"):
        tv = f"series:{ext['thetvdb']}"
        if rec.get("tvdb") and rec["tvdb"] != tv:
            problems.append(f"tvdb mismatch wikidata {rec['tvdb']} vs tvmaze {tv}")
        rec["tvdb"] = tv
    prem = (show.get("premiered") or "")[:4]
    if prem.isdigit() and abs(int(prem) - item["y"]) > 1:
        if daily:
            # TVmaze splits long-running talk shows by host; the air dates are
            # still episodes of the same TMDB series.
            rec["note"] = f"tvmaze show {show['id']} premiered {prem}"
        else:
            problems.append(f"tvmaze premiered {prem} vs {item['y']}")
    rec["runtime_min"] = show.get("averageRuntime") or show.get("runtime")
    rec["episodes"] = tvmaze_episodes(fx, show["id"])
    rec["checks"].append("episodes:tvmaze")
    return rec


def aod_index(path):
    idx = {}
    with open(path, encoding="utf-8") as f:
        next(f)
        for line in f:
            r = json.loads(line)
            ids = {}
            for s in r["sources"]:
                for k, p in (("anidb", "anidb.net/anime/"), ("anilist", "anilist.co/anime/"),
                             ("mal", "myanimelist.net/anime/"), ("kitsu", "kitsu.app/anime/")):
                    if p in s:
                        ids[k] = int(s.split(p)[1])
            if "anidb" in ids:
                idx[ids["anidb"]] = (r, ids)
    return idx


AOD_TYPE = {"TV": "tv", "OVA": "ova", "ONA": "ona", "MOVIE": "movie", "SPECIAL": "special"}


def resolve_anime(wd, aod, aid, problems):
    if aid not in aod:
        problems.append("aid not in anime-offline-database")
        return None
    r, ids = aod[aid]
    rec = {"anidb": aid, "title": r["title"], "year": r["animeSeason"].get("year"),
           "anime_type": AOD_TYPE.get(r["type"]), "episodes": r["episodes"],
           "checks": ["anidb+mal+anilist+kitsu:anime-offline-database"]}
    for k in ("mal", "anilist", "kitsu"):
        if k in ids:
            rec[k] = ids[k]
    dur = r.get("duration") or {}
    if dur.get("unit") == "SECONDS":
        rec["episode_seconds"] = dur.get("value")
    if rec["anime_type"] is None:
        problems.append(f"unknown type {r['type']}")
    hits = wd.search(f"haswbstatement:{P_ANIDB}={aid}", limit=3)
    if not hits and "mal" in ids:
        hits = wd.search(f"haswbstatement:{P_MAL}={ids['mal']}", limit=3)
    if len(hits) == 1:
        e = wd.entities(hits)[hits[0]]
        rec["qid"] = e["id"]
        want = P_TMDB_MOVIE if rec["anime_type"] == "movie" else P_TMDB_TV
        tmdb = claim_str(e, want)
        if tmdb:
            rec["tmdb"] = f"{'movie' if want == P_TMDB_MOVIE else 'tv'}:{tmdb}"
            rec["checks"].append("tmdb:wikidata")
        imdb = claim_str(e, P_IMDB)
        if imdb:
            rec["imdb"] = imdb
        wd_anidb = claim_str(e, P_ANIDB)
        if wd_anidb and wd_anidb != str(aid):
            # The item describes the franchise or a different AniDB entry: ids are not ours.
            rec.pop("tmdb", None)
            rec.pop("imdb", None)
            rec["checks"] = rec["checks"][:1]
            rec["note"] = f"wikidata item {e['id']} is AniDB {wd_anidb}, ids not taken"
        rec["label_ja"] = label(e, "ja")
    elif len(hits) > 1:
        rec["note"] = f"several wikidata items for aid {aid}: {hits}"
    if rec["anime_type"] != "movie" and hits:
        # Season items carry the AniDB id, the TMDB TV id sits on the series
        # item ("part of the series" P179 / "part of" P361). GUIDE §3: the
        # TMDB id of a season-like AniDB entry is the common series.
        series = set()
        for e in wd.entities(hits).values():
            parents = [v["id"] for p in ("P179", "P361") for v in claim_values(e, p)
                       if isinstance(v, dict) and v.get("id")]
            for pe in wd.entities(parents[:5]).values():
                t = claim_str(pe, P_TMDB_TV)
                if t:
                    series.add(f"tv:{t}")
        if len(series) == 1:
            rec["tmdb_series"] = series.pop()
            rec["checks"].append("tmdb_series:wikidata(P179/P361)")
        elif series:
            rec["note"] = (rec.get("note", "") + f" several parent series {sorted(series)}").strip()
    return rec


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--titles", default=os.path.join(here, "titles.yaml"))
    ap.add_argument("--aod", required=True)
    ap.add_argument("--cache", default=os.path.join(here, ".cache"))
    ap.add_argument("--out", default=os.path.join(here, "facts.json"))
    args = ap.parse_args()

    titles = yaml.safe_load(open(args.titles, encoding="utf-8"))
    fx = Fetcher(args.cache)
    wd = Wikidata(fx)
    aod = aod_index(args.aod)
    facts = {"sources": {
        "wikidata": "https://www.wikidata.org (CC0-1.0)",
        "tvmaze": "https://www.tvmaze.com/api (CC BY-SA 4.0)",
        "anime-offline-database": "https://github.com/cedya77/anime-offline-database 2026-45 (ODbL-1.0), continuation of manami-project",
    }, "movies": [], "shows": [], "daily": [], "anime": {}, "problems": []}

    for section in ("movies", "shows", "daily"):
        kind = "movie" if section == "movies" else "show"
        for item in titles[section]:
            problems: list[str] = []
            rec = resolve_video(wd, fx, item, kind, problems, daily=section == "daily")
            key = f"{section}:{item['t']} ({item['y']})"
            if rec is None:
                facts["problems"].append({"key": key, "problems": problems})
                print(f"FAIL {key}: {problems}", file=sys.stderr)
                continue
            rec["tags"] = item.get("tags", [])
            if problems:
                rec["problems"] = problems
                print(f"WARN {key}: {problems}", file=sys.stderr)
            facts[section].append(rec)

    aids = []
    for fr in titles["anime"]:
        aids += [e["aid"] for e in fr["entries"]]
    aids += [m["aid"] for m in titles["anime_movies"]]
    for aid in aids:
        problems = []
        rec = resolve_anime(wd, aod, aid, problems)
        if rec is None or problems:
            facts["problems"].append({"key": f"anidb:{aid}", "problems": problems})
            print(f"FAIL anidb {aid}: {problems}", file=sys.stderr)
            if rec is None:
                continue
            rec["problems"] = problems
        facts["anime"][str(aid)] = rec

    # TVmaze for anime franchises is not used: its numbering is its own scheme.
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(facts, f, ensure_ascii=False, indent=1, sort_keys=True)
        f.write("\n")
    n = sum(len(facts[s]) for s in ("movies", "shows", "daily"))
    print(f"resolved {n} movies/shows, {len(facts['anime'])} anime entries, "
          f"{len(facts['problems'])} failures", file=sys.stderr)


if __name__ == "__main__":
    main()
