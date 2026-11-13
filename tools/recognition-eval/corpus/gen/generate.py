#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Generate recognition corpus v0 (META-002) from titles.yaml + facts.json.

    python3 generate.py [--out-dir ../v0] [--seed 2]

Offline and deterministic: the same inputs give byte-identical JSONL. The
generator only *varies* how files are named (release groups, separators,
quality tags, folder layouts); every label comes from facts.json (ids checked
against open sources by resolve.py) or from the maintainer rules recorded in
titles.yaml. The expected parse is built from the very tokens the generator
wrote into the path, so it is correct by construction.

Status policy (see ../README.md):
  verified  every work is non-episodic (movie, anime movie) or a daily show
            identified by air date, all ids come from resolve.py checks, and
            corpus-validate --strict passes;
  draft     the record carries episode positions in provider schemes that
            could not be checked against the provider API (TMDB/TVDB need a
            key): TMDB positions of shows are taken from TVmaze aired order,
            anime TMDB/TVDB positions follow the rules in titles.yaml.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import unicodedata
from collections import Counter, defaultdict

import yaml

HERE = os.path.dirname(os.path.abspath(__file__))
ANNOTATOR = "pa-meta002"
LICENSE = "MPL-2.0"

# ---------------------------------------------------------------- vocab

FANSUB = ["SubsPlease", "Erai-raws", "HorribleSubs", "Judas", "ASW", "EMBER", "Commie", "GJM",
          "Kametsu", "Coalgirls", "Doki", "FFF", "UTW", "Underwater", "Beatrice-Raws", "Moozzi2",
          "VCB-Studio", "Reinforce", "ANE", "LoliHouse", "Kaleido-subs", "Okay-Subs", "neoDESU",
          "Anime Time", "DameDesuYo", "Cleo", "Arid", "Vodes", "MTBB", "sam"]
SCENE = ["SPARKS", "AMIABLE", "GECKOS", "DRONES", "ROVERS", "CiNEFiLE", "BLOW", "FGT", "NTb",
         "FLUX", "CMRG", "EVO", "RARBG", "YTS", "iNTERNAL", "DiMENSION", "KILLERS", "LOL", "SVA",
         "AVS", "MiNX", "ION10", "TEPES", "NOGRP", "playWEB", "GalaxyRG", "SiGMA", "HONE",
         "W4NK3R", "DEFLATE"]
XVID_GROUPS = ["aXXo", "DiAMOND", "FXG", "MAXSPEED", "NeDiVx", "KLAXXON", "VoMiT", "ViSiON", "PUKKA",
               "SAPHiRE", "DoNE", "iMBT"]
RU_DUB_SHOW = ["LostFilm", "NewStudio", "AlexFilm", "Кубик в Кубе", "HDRezka Studio", "Jaskier",
               "Пифагор", "TVShows", "BaibaKo", "Novamedia", "Amedia", "Кураж-Бамбей"]
RU_DUB_MOVIE = ["Дубляж", "MVO", "AVO", "Гоблин", "Пучков", "Гаврилов", "Володарский", "Сербин",
                "iTunes", "Line"]
RU_DUB_ANIME = ["AniLibria", "AniDUB", "SHIZA Project", "AnimeVost", "Dream Cast", "JAM CLUB",
                "StudioBand", "Persona99", "Anything Group"]
PERSONAL = ["VID_{d:%Y%m%d}_{t}.mp4", "IMG_{n}.MOV", "Screen Recording {d:%Y-%m-%d} at {h}.{m}.{s}.mov",
            "PXL_{d:%Y%m%d}_{t}{ms}.mp4", "video_{d:%Y-%m-%d}_{h}-{m}-{s}.mp4", "GOPR{n}.MP4",
            "WhatsApp Video {d:%Y-%m-%d} at {h}.{m}.{s}.mp4", "DSC_{n}.AVI", "MVI_{n}.MP4",
            "Запись экрана {d:%Y-%m-%d} в {h}.{m}.{s}.mov", "Новогодний концерт {d:%Y}.mp4",
            "Выпускной {d:%Y}.avi", "Дача {d:%Y}.mp4", "Отпуск {d:%Y}.mp4"]

RES = {"2160p": (3840, 2160, 18), "1080p": (1920, 1080, 8), "720p": (1280, 720, 4),
       "576p": (720, 576, 2), "480p": (720, 480, 1.5)}
ANIME_BITRATE = {"2160p": 8, "1080p": 3, "720p": 1.2, "576p": 0.9, "480p": 0.7}
CONTAINER = {"mkv": "matroska", "mp4": "mp4", "m4v": "mp4", "avi": "avi", "m2ts": "mpegts",
             "ts": "mpegts", "vob": "mpeg", "mov": "mov", "webm": "webm", "wmv": "asf"}

TRANSLIT = dict(zip("абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
                    ["a", "b", "v", "g", "d", "e", "e", "zh", "z", "i", "y", "k", "l", "m", "n", "o",
                     "p", "r", "s", "t", "u", "f", "kh", "ts", "ch", "sh", "shch", "", "y", "", "e",
                     "yu", "ya"]))


def translit(s: str) -> str:
    out = []
    for c in s:
        lo = c.lower()
        if lo in TRANSLIT:
            t = TRANSLIT[lo]
            out.append(t.capitalize() if c != lo and t else t)
        else:
            out.append(c)
    return "".join(out)


def clean(t: str) -> str:
    """Title as a releaser would write it in a filename (no reserved characters)."""
    t = t.replace("«", "").replace("»", "").replace('"', "").replace("“", "").replace("”", "")
    t = t.replace(": ", " - ").replace(":", " ").replace("/", " ").replace("?", "").replace("*", "")
    t = t.replace("\\", " ").replace("|", " ").replace("<", "").replace(">", "")
    return re.sub(r"\s+", " ", t).strip(" .-")


def dotted(t: str) -> str:
    t = clean(t).replace(" - ", " ")
    t = re.sub(r"[()\[\],'’!&]", "", t)
    return re.sub(r"[\s]+", ".", t).strip(".")


def undot(t: str) -> str:
    return t.replace(".", " ")


def slug(s: str) -> str:
    s = translit(s)
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")
    return s[:40].strip("-") or "x"


def has_digit(s):
    return any(c.isdigit() for c in s)


def crc(rng):
    return "%08X" % rng.getrandbits(32)


# ---------------------------------------------------------------- record builder

class Rec:
    def __init__(self, rng, group):
        self.rng = rng
        self.group = group  # split key
        self.files = []
        self.labels = []
        self.works = []
        self.work_keys = {}
        self.cats = []
        self.notes = []
        self.unchecked = False  # numbering not checked against a provider API
        self.extra_flags = set()

    def work(self, key, w):
        if key not in self.work_keys:
            ref = f"w{len(self.works) + 1}"
            self.work_keys[key] = ref
            self.works.append(dict(ref=ref, **w))
        return self.work_keys[key]

    def add(self, path, label, *, dur=None, res=None, codec=None, alangs=None, slangs=None, anime=False,
            size=None, add_input=True):
        if add_input:
            f = {"path": path}
            ext = path.rsplit(".", 1)[-1].lower() if "." in path.rsplit("/", 1)[-1] else ""
            if dur is not None:
                f["duration_ms"] = int(dur * 1000)
                if size is None and res:
                    mbps = (ANIME_BITRATE if anime else {k: v[2] for k, v in RES.items()})[res]
                    size = int(dur * mbps * 1_000_000 / 8 * self.rng.uniform(0.85, 1.15))
            if size is not None:
                f["size"] = size
            if ext in CONTAINER:
                f["container"] = CONTAINER[ext]
            if res:
                w, h, _ = RES[res]
                f["video"] = {"width": w, "height": h}
                if codec:
                    f["video"]["codec"] = codec
            if alangs:
                f["audio_langs"] = alangs
            if slangs:
                f["sub_langs"] = slangs
            self.files.append(f)
        lab = {"path": path}
        lab.update(label)
        self.labels.append(lab)
        return lab

    def side(self, path, size=None):
        self.files.append({"path": path, "size": size or self.rng.randint(800, 90000)})
        self.labels.append({"path": path, "role": "sidecar"})


def movie_ids(m):
    ids = {}
    for k in ("tmdb", "imdb", "kinopoisk"):
        if m.get(k):
            ids[k] = m[k]
    return ids


def movie_work(m):
    return {"kind": "movie", "title": m["title"], "year": m["work_year"], "ids": movie_ids(m)}


def show_work(s):
    ids = {k: s[k] for k in ("tmdb", "tvdb", "imdb", "tvmaze") if s.get(k)}
    return {"kind": "show", "title": s["title"], "year": s["work_year"], "ids": ids}


# ---------------------------------------------------------------- data model

def own_tv(fact):
    """TMDB TV series of an AniDB entry: its own Wikidata id or its parent series (P179/P361)."""
    if fact.get("tmdb", "").startswith("tv:"):
        return fact["tmdb"]
    return fact.get("tmdb_series")


class Data:
    def __init__(self, titles, facts):
        self.movies = [m for m in facts["movies"] if not m.get("problems") and m.get("tmdb")]
        self.shows = [s for s in facts["shows"] if not s.get("problems") and s.get("tmdb") and s.get("episodes")]
        self.daily = [s for s in facts["daily"] if not s.get("problems") and s.get("tmdb") and s.get("episodes")]
        for m in self.movies + self.shows + self.daily:
            # `year` is what releasers write (titles.yaml); `work_year` is the
            # premiere year from Wikidata (they differ by one for some films).
            m["work_year"] = m["year"]
            m["year"] = m.get("name_year", m["year"])
        self.anime_facts = facts["anime"]
        self.franchises = []
        for fr in titles["anime"]:
            entries = []
            ok = True
            for e in fr["entries"]:
                f = self.anime_facts.get(str(e["aid"]))
                if not f or f.get("problems") or not f.get("episodes"):
                    ok = False
                    break
                entries.append({**e, "fact": f, "names": {**fr["names"], **e.get("names", {})}})
            if not ok:
                continue
            fr = dict(fr)
            fr["entries"] = entries
            # franchise-level TMDB series id: GUIDE §3 (TMDB id points to the common series)
            fr["tmdb"] = own_tv(entries[0]["fact"])
            for x in entries:
                own = own_tv(x["fact"])
                if x.get("tmdb_season") and (not fr["tmdb"] or (own and own != fr["tmdb"])):
                    # TMDB positions only make sense inside the franchise's TMDB series.
                    x.pop("tmdb_season", None)
                    x.pop("tmdb_offset", None)
            cum = 0
            for x in entries:
                x["abs_offset"] = cum
                cum += x["fact"]["episodes"]
            self.franchises.append(fr)
        self.fr_by_key = {f["key"]: f for f in self.franchises}
        self.anime_movies = []
        for m in titles["anime_movies"]:
            f = self.anime_facts.get(str(m["aid"]))
            if f and not f.get("problems") and f.get("anime_type") == "movie":
                self.anime_movies.append({**m, "fact": f})

    # ---- anime helpers
    def anime_work(self, fr, entry):
        f = entry["fact"]
        ids = {"anidb": f["anidb"]}
        for k in ("anilist", "mal", "kitsu"):
            if f.get(k):
                ids[k] = f[k]
        tm = own_tv(f) or (fr.get("tmdb") if fr and (entry.get("tmdb_season") or fr.get("tmdb_shared")) else None)
        if tm:
            ids["tmdb"] = tm
        if f.get("imdb") and own_tv(f) and f.get("tmdb") == tm:
            ids["imdb"] = f["imdb"]
        return {"kind": "anime", "anime_type": f["anime_type"], "title": f["title"], "year": f["year"],
                "ids": ids}

    def anime_movie_work(self, m):
        f = m["fact"]
        ids = {"anidb": f["anidb"]}
        for k in ("anilist", "mal", "kitsu"):
            if f.get(k):
                ids[k] = f[k]
        if f.get("tmdb", "").startswith("movie:"):
            ids["tmdb"] = f["tmdb"]
            if f.get("imdb"):
                ids["imdb"] = f["imdb"]
        return {"kind": "anime", "anime_type": "movie", "title": f["title"], "year": f["year"], "ids": ids}

    def anime_episode(self, fr, entry, n, work_has_tmdb):
        """Episode n of an AniDB entry in all schemes we can state."""
        num = {"anidb": {"type": "regular", "number": n}}
        if entry.get("tmdb_season") and work_has_tmdb:
            num["tmdb"] = {"season": entry["tmdb_season"], "episode": entry.get("tmdb_offset", 0) + n}
        if fr.get("abs"):
            num["tvdb_absolute"] = entry["abs_offset"] + n
        return {"type": "regular", "numbering": num}


# ---------------------------------------------------------------- helpers for names

def pick_res(rng, old=False, anime=False):
    if old:
        return rng.choice(["480p", "576p", "720p"])
    return rng.choice(["1080p", "1080p", "1080p", "720p", "2160p"] if not anime else ["1080p", "1080p", "720p"])


def movie_source(rng, res):
    if res == "2160p":
        return rng.choice(["UHD.BluRay", "WEB-DL", "BluRay.REMUX"])
    if res in ("480p", "576p"):
        return rng.choice(["DVDRip", "DVD5", "TVRip", "SATRip"])
    return rng.choice(["BluRay", "BDRip", "WEB-DL", "WEBRip", "HDTV"])


def codec_tag(rng, res, old=False):
    if old:
        return rng.choice(["XviD", "DivX", "x264"]), None
    t = rng.choice(["x264", "x265", "H.264", "HEVC", "AVC"])
    return t, {"x264": "h264", "H.264": "h264", "AVC": "h264", "x265": "hevc", "HEVC": "hevc"}[t]


def runtime_s(m, rng):
    rt = m.get("runtime_min")
    if not rt:
        return None
    return rt * 60 + rng.randint(-40, 40)


def movie_main_name(rng, m, style, res=None, group=None, ext=None):
    """Return (name, parse) for a movie file in a given naming style."""
    t = m["title"]
    y = m["year"]
    res = res or pick_res(rng, "old" in m["tags"])
    src = movie_source(rng, res)
    ctag, _ = codec_tag(rng, res, res in ("480p", "576p"))
    parse = {}
    if style == "scene":
        group = group or rng.choice(SCENE if res not in ("480p", "576p") else XVID_GROUPS)
        ext = ext or ("avi" if ctag in ("XviD", "DivX") else "mkv")
        name = f"{dotted(t)}.{y}.{res}.{src}.{ctag}-{group}.{ext}"
        parse = {"title": undot(dotted(t)), "year": y, "resolution": res, "source": src, "release_group": group}
    elif style == "plex":
        name = f"{clean(t)} ({y}).{ext or 'mkv'}"
        parse = {"title": clean(t), "year": y}
    elif style == "plain":
        name = f"{clean(t)} {y} {res} {src}.{ext or 'mkv'}"
        parse = {"title": clean(t), "year": y, "resolution": res, "source": src}
    elif style == "brackets":
        name = f"{clean(t)} [{y}] [{res}] [{src}].{ext or 'mkv'}"
        parse = {"title": clean(t), "year": y, "resolution": res, "source": src}
    elif style == "noyear":
        name = f"{clean(t)}.{ext or 'mkv'}"
        parse = {"title": clean(t)}
    elif style == "noyear-scene":
        group = group or rng.choice(SCENE)
        name = f"{dotted(t)}.{res}.{src}.{ctag}-{group}.{ext or 'mkv'}"
        parse = {"title": undot(dotted(t)), "resolution": res, "source": src, "release_group": group}
    else:
        raise ValueError(style)
    return name, parse, res, ctag


def langs_for_movie(m, ru_release=False):
    if "ru" in m["tags"]:
        return ["rus"], []
    return (["rus", "eng"] if ru_release else ["eng"]), (["rus"] if ru_release else [])


# ---------------------------------------------------------------- templates

class Gen:
    def __init__(self, data: Data, seed: int):
        self.d = data
        self.seed = seed

    def rng_for(self, cat, i):
        h = hashlib.sha256(f"{self.seed}:{cat}:{i}".encode()).digest()
        return random.Random(int.from_bytes(h[:8], "big"))

    # ===== movies =====
    def m_movie_simple(self, rng, rec, m, style, root="Movies", folder=True, ru=False):
        name, parse, res, ctag = movie_main_name(rng, m, style)
        dur = runtime_s(m, rng)
        ref = rec.work(m["qid"], movie_work(m))
        if folder:
            fold = name.rsplit(".", 1)[0] if style == "scene" else f"{clean(m['title'])} ({m['year']})"
            path = f"{root}/{fold}/{name}"
        else:
            path = f"{root}/{name}"
        al, sl = langs_for_movie(m, ru)
        codec = {"XviD": "mpeg4", "DivX": "mpeg4"}.get(ctag) or codec_tag(rng, res)[1]
        rec.add(path, {"role": "main", "work": ref, "parse": parse}, dur=dur, res=res, codec=codec, alangs=al,
                slangs=sl)
        return path, ref

    def tpl_movie_a6(self, rng, rec):
        m = rng.choice(self.d.movies)
        rec.group = m["qid"]
        path, ref = self.m_movie_simple(rng, rec, m, "scene", root=rng.choice(["Movies", "Downloads", "Фильмы"]))
        folder = path.rsplit("/", 1)[0]
        if rng.random() < 0.5:
            rec.side(f"{folder}/{folder.rsplit('/', 1)[1].lower()}.nfo")
        return m["title"]

    def tpl_movie_a14(self, rng, rec):
        pool = [m for m in self.d.movies if "digits" in m["tags"] and has_digit(m["title"])]
        m = rng.choice(pool)
        rec.group = m["qid"]
        style = rng.choice(["scene", "scene", "plex", "plain", "noyear", "brackets"])
        if style == "noyear" and not m.get("runtime_min"):
            style = "plex"
        self.m_movie_simple(rng, rec, m, style, root=rng.choice(["Movies", "Films", "Кино"]))
        if style == "noyear":
            rec.cats.append("A8")
        return m["title"]

    def tpl_movie_a8(self, rng, rec):
        groups = defaultdict(list)
        for m in self.d.movies:
            for t in m["tags"]:
                if t.startswith("remake:") and m.get("runtime_min"):
                    groups[t].append(m)
        groups = {k: v for k, v in groups.items() if len(v) >= 2}
        key = rng.choice(sorted(groups))
        m = rng.choice(groups[key])
        rec.group = key
        variant = rng.random()
        if variant < 0.55:
            style = rng.choice(["noyear", "noyear-scene"])
            self.m_movie_simple(rng, rec, m, style, root=rng.choice(["Movies", "Video", "Кино"]),
                                folder=rng.random() < 0.5)
            rec.notes.append(f"No year in the name; homonyms {sorted({x['year'] for x in groups[key]})} differ in runtime.")
        elif variant < 0.8:
            # two films of the same name in one folder, only one has the year
            ms = rng.sample(groups[key], 2)
            root = f"Movies/{clean(ms[0]['title'])}"
            for i, x in enumerate(ms):
                style = "noyear" if i == 0 else "plex"
                name, parse, res, ctag = movie_main_name(rng, x, style)
                ref = rec.work(x["qid"], movie_work(x))
                rec.add(f"{root}/{name}", {"role": "main", "work": ref, "parse": parse}, dur=runtime_s(x, rng),
                        res=res, codec=codec_tag(rng, res)[1], alangs=["eng"] if "ru" not in x["tags"] else ["rus"])
            rec.notes.append("Same title, different films: the year-less file is the other one.")
        else:
            # Russian title only (remakes share it)
            ru = m.get("label_ru")
            if not ru:
                return None
            res = pick_res(rng, "old" in m["tags"])
            if rng.random() < 0.5:
                title = clean(ru)
                name = f"{title} {res}.mkv"
            else:
                tl = dotted(translit(clean(ru)))
                title = undot(tl)
                name = f"{tl}.{res}.mkv"
            ref = rec.work(m["qid"], movie_work(m))
            rec.add(f"Кино/{name}", {"role": "main", "work": ref, "parse": {"title": title, "resolution": res}},
                    dur=runtime_s(m, rng), res=res, codec="h264", alangs=["rus"])
            rec.cats.append("A7")
        return m["title"]

    def tpl_movie_a7(self, rng, rec):
        """Russian releases: Soviet/Russian films and foreign films with Russian names."""
        ru_pool = [m for m in self.d.movies if "ru" in m["tags"] and m.get("label_ru")]
        any_pool = [m for m in self.d.movies if m.get("label_ru")]
        m = rng.choice(ru_pool if rng.random() < 0.6 else any_pool)
        rec.group = m["qid"]
        ru = clean(m["label_ru"])
        y = m["year"]
        res = pick_res(rng, "old" in m["tags"])
        src = movie_source(rng, res)
        is_ru = "ru" in m["tags"]
        variant = rng.randrange(6)
        dub = rng.choice(RU_DUB_MOVIE)
        if variant == 0:
            name = f"{ru} ({y}) {src} {res}.mkv"
            parse = {"title": ru, "year": y, "source": src, "resolution": res}
        elif variant == 1:
            tl = dotted(translit(ru))
            name = f"{tl}.{y}.{src}.{res}.mkv"
            parse = {"title": undot(tl), "year": y, "source": src, "resolution": res}
        elif variant == 2 and not is_ru and clean(m["title"]) != ru:
            name = f"{ru} ∕ {clean(m['title'])} ({y}) {src} {res}.mkv"
            parse = {"title": ru, "alt_titles": [clean(m["title"])], "year": y, "source": src, "resolution": res}
            rec.cats.append("A5")
        elif variant == 3:
            name = f"{ru}.{y}.{src}.{res}.{dub}.mkv".replace(" ", ".")
            parse = {"title": undot(ru.replace(" ", ".")), "year": y, "source": src, "resolution": res,
                     "dub_studio": dub}
            if " " in dub:
                return None
        elif variant == 4 and not is_ru and clean(m["title"]) != ru:
            name = f"{clean(m['title'])} ({ru}) {y} {res}.mkv"
            parse = {"title": clean(m["title"]), "alt_titles": [ru], "year": y, "resolution": res}
            rec.cats.append("A5")
        else:
            name = f"{ru} {y} {res}.avi" if "old" in m["tags"] else f"{ru} ({y}).mkv"
            parse = {"title": ru, "year": y} | ({"resolution": res} if name.endswith(".avi") else {})
        root = rng.choice(["Фильмы", "Кино", "Movies", "Видео/Фильмы"])
        path = f"{root}/{name}" if rng.random() < 0.5 else f"{root}/{ru} ({y})/{name}"
        ref = rec.work(m["qid"], movie_work(m))
        rec.add(path, {"role": "main", "work": ref, "parse": parse}, dur=runtime_s(m, rng), res=res,
                codec="mpeg4" if name.endswith(".avi") else "h264",
                alangs=["rus"] if is_ru else ["rus", "eng"], slangs=[] if is_ru else ["rus", "eng"])
        if rng.random() < 0.3:
            rec.side(path.rsplit(".", 1)[0] + ".srt")
        return m["title"]

    def tpl_movie_a5(self, rng, rec):
        """Same work under different names in folder and file (Russian/English, original/translated)."""
        pool = [m for m in self.d.movies if m.get("label_ru") and clean(m["label_ru"]) != clean(m["title"])]
        m = rng.choice(pool)
        rec.group = m["qid"]
        ru, en, y = clean(m["label_ru"]), clean(m["title"]), m["year"]
        res = pick_res(rng, "old" in m["tags"])
        v = rng.randrange(3)
        ref = rec.work(m["qid"], movie_work(m))
        if v == 0:
            folder, name = f"Фильмы/{ru} ({y})", f"{dotted(en)}.{y}.{res}.mkv"
            parse = {"title": undot(dotted(en)), "alt_titles": [ru], "year": y, "resolution": res}
        elif v == 1:
            folder, name = f"Movies/{en} ({y})", f"{ru} {y} {res}.mkv"
            parse = {"title": ru, "alt_titles": [en], "year": y, "resolution": res}
        else:
            folder, name = "Movies", f"{ru} | {en} ({y}) {res}.mkv"
            parse = {"title": ru, "alt_titles": [en], "year": y, "resolution": res}
        rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": parse}, dur=runtime_s(m, rng),
                res=res, codec="h264", alangs=["rus", "eng"] if "ru" not in m["tags"] else ["rus"])
        return m["title"]

    def tpl_movie_a10(self, rng, rec):
        m = rng.choice([x for x in self.d.movies if x.get("runtime_min")])
        rec.group = m["qid"]
        style = rng.choice(["plex", "scene"])
        path, ref = self.m_movie_simple(rng, rec, m, style)
        folder = path.rsplit("/", 1)[0]
        base = path.rsplit("/", 1)[1].rsplit(".", 1)[0]
        kinds = rng.sample(["trailer", "sample", "featurette", "behind", "deleted", "interview", "nfo", "srt",
                            "poster", "teaser"], rng.randint(2, 4))
        if "sample" not in kinds and "trailer" not in kinds:
            kinds.append(rng.choice(["sample", "trailer"]))
        for k in kinds:
            if k == "trailer":
                p = f"{folder}/{base}-trailer.mkv" if rng.random() < 0.5 else f"{folder}/Trailers/Official Trailer.mkv"
                rec.add(p, {"role": "extra", "extra_type": "trailer", "work": ref}, dur=rng.randint(90, 160),
                        res="1080p", codec="h264", alangs=["eng"])
            elif k == "teaser":
                rec.add(f"{folder}/{base}-teaser.mp4", {"role": "extra", "extra_type": "teaser", "work": ref},
                        dur=rng.randint(40, 80), res="1080p", codec="h264", alangs=["eng"])
            elif k == "sample":
                p = f"{folder}/Sample/{base.lower()}-sample.mkv" if style == "scene" else f"{folder}/sample.mkv"
                rec.add(p, {"role": "sample", "work": ref}, dur=rng.randint(30, 90), res="1080p", codec="h264",
                        alangs=["eng"])
            elif k == "featurette":
                rec.add(f"{folder}/Featurettes/{rng.choice(['Making Of', 'The Look of the Film', 'Visual Effects'])}.mkv",
                        {"role": "extra", "extra_type": "featurette", "work": ref}, dur=rng.randint(300, 1500),
                        res="1080p", codec="h264", alangs=["eng"])
            elif k == "behind":
                rec.add(f"{folder}/Behind The Scenes/On Set.mkv", {"role": "extra", "extra_type": "behind_the_scenes",
                        "work": ref}, dur=rng.randint(200, 900), res="1080p", codec="h264", alangs=["eng"])
            elif k == "deleted":
                rec.add(f"{folder}/Deleted Scenes/Deleted Scene {rng.randint(1, 6)}.mkv",
                        {"role": "extra", "extra_type": "deleted_scene", "work": ref}, dur=rng.randint(60, 300),
                        res="1080p", codec="h264", alangs=["eng"])
            elif k == "interview":
                rec.add(f"{folder}/{base}-interview.mkv", {"role": "extra", "extra_type": "interview", "work": ref},
                        dur=rng.randint(200, 900), res="720p", codec="h264", alangs=["eng"])
            elif k == "nfo":
                rec.side(f"{folder}/{base}.nfo")
            elif k == "srt":
                rec.side(f"{folder}/{base}.{rng.choice(['en', 'ru', 'eng'])}.srt")
            elif k == "poster":
                rec.side(f"{folder}/poster.jpg", size=rng.randint(200000, 900000))
        return m["title"]

    def tpl_movie_a12(self, rng, rec):
        m = rng.choice(self.d.movies)
        rec.group = m["qid"]
        ref = rec.work(m["qid"], movie_work(m))
        t, y = clean(m["title"]), m["year"]
        dur = runtime_s(m, rng) or 6000
        v = rng.randrange(5)
        if v in (0, 1):
            root = rng.choice([f"Movies/{t} ({y})", f"BD/{t.upper()}", f"Movies/{dotted(t)}.{y}.COMPLETE.BLURAY"])
            main = rng.choice(["00000", "00001", "00800", "00002"])
            files = [("BDMV/index.bdmv", 280), ("BDMV/MovieObject.bdmv", 6000), (f"BDMV/PLAYLIST/{main}.mpls", 800),
                     (f"BDMV/CLIPINF/{main}.clpi", 3000), ("CERTIFICATE/id.bdmv", 100)]
            for p, s in files:
                rec.files.append({"path": f"{root}/{p}", "size": s * rng.randint(1, 3)})
            rec.files.append({"path": f"{root}/BDMV/STREAM/{main}.m2ts", "size": int(dur * 4.2e6),
                              "duration_ms": dur * 1000, "container": "mpegts",
                              "video": {"width": 1920, "height": 1080, "codec": "h264"}, "audio_langs": ["eng"]})
            for k in range(rng.randint(1, 3)):
                rec.files.append({"path": f"{root}/BDMV/STREAM/0{k + 1:04d}.m2ts", "size": rng.randint(5, 90) * 10**6,
                                  "duration_ms": rng.randint(10, 200) * 1000, "container": "mpegts"})
            rec.labels.append({"path": root, "unit": "bdmv", "role": "main", "work": ref,
                               "parse": {"title": t if "BLURAY" not in root else undot(dotted(t))} |
                               ({"year": y} if str(y) in root else {})})
        elif v == 2:
            root = f"DVD/{t} ({y})"
            vobs = max(2, dur // 1500)
            rec.files.append({"path": f"{root}/VIDEO_TS/VIDEO_TS.IFO", "size": 12288})
            rec.files.append({"path": f"{root}/VIDEO_TS/VIDEO_TS.BUP", "size": 12288})
            rec.files.append({"path": f"{root}/VIDEO_TS/VIDEO_TS.VOB", "size": rng.randint(100000, 900000)})
            rec.files.append({"path": f"{root}/VIDEO_TS/VTS_01_0.IFO", "size": 65536})
            for k in range(1, vobs + 1):
                rec.files.append({"path": f"{root}/VIDEO_TS/VTS_01_{k}.VOB", "size": 1073709056 if k < vobs else rng.randint(2, 9) * 10**8,
                                  "container": "mpeg"})
            rec.labels.append({"path": root, "unit": "video_ts", "role": "main", "work": ref,
                               "parse": {"title": t, "year": y}})
        else:
            name = rng.choice([f"{dotted(t)}.{y}.BluRay.iso", f"{t} ({y}) [DVD9].iso", f"{t.upper().replace(' ', '_')}.iso"])
            if name.endswith("_.iso") or name == ".iso":
                return None
            parse = {"title": undot(dotted(t)), "year": y, "source": "BluRay"} if name.endswith("BluRay.iso") else \
                {"title": t, "year": y} if "DVD9" in name else {"title": t.upper().replace(" ", "_").replace("_", " ")}
            rec.files.append({"path": f"Images/{name}", "size": rng.randint(7, 45) * 10**9})
            rec.labels.append({"path": f"Images/{name}", "role": "main", "work": ref, "parse": parse})
        return m["title"]

    def tpl_movie_a13(self, rng, rec):
        m = rng.choice([x for x in self.d.movies if x.get("runtime_min") and "old" in x["tags"]] or self.d.movies)
        rec.group = m["qid"]
        ref = rec.work(m["qid"], movie_work(m))
        t, y = m["title"], m["year"]
        dur = runtime_s(m, rng)
        n = 2 if rng.random() < 0.8 or dur < 7000 else 3
        v = rng.randrange(4)
        grp = rng.choice(XVID_GROUPS)
        cuts = sorted(rng.sample(range(int(dur * 0.35), int(dur * 0.65)), n - 1)) if n == 2 else \
            [dur // 3, 2 * dur // 3]
        bounds = [0] + cuts + [dur]
        folder = f"Movies/{dotted(t)}.{y}.DVDRip.XviD-{grp}" if v < 2 else f"Movies/{clean(t)} ({y})"
        for k in range(1, n + 1):
            if v == 0:
                name = f"{dotted(t).lower()}.{y}.dvdrip.xvid-{grp.lower()}.cd{k}.avi"
                parse = {"title": undot(dotted(t).lower()), "year": y, "source": "dvdrip", "release_group": grp.lower(),
                         "part": k}
            elif v == 1:
                name = f"{dotted(t)}.{y}.DVDRip.XviD-{grp}.CD{k}.avi"
                parse = {"title": undot(dotted(t)), "year": y, "source": "DVDRip", "release_group": grp, "part": k}
            elif v == 2:
                name = f"{clean(t)} ({y}) - part{k}.mkv"
                parse = {"title": clean(t), "year": y, "part": k}
            else:
                name = f"{clean(t)} ({y}) Disc {k}.mkv"
                parse = {"title": clean(t), "year": y, "part": k}
            seg = bounds[k] - bounds[k - 1]
            rec.add(f"{folder}/{name}", {"role": "part", "work": ref, "part": k, "parse": parse}, dur=seg,
                    res="576p" if name.endswith(".avi") else "1080p",
                    codec="mpeg4" if name.endswith(".avi") else "h264", alangs=["eng"] if "ru" not in m["tags"] else ["rus"],
                    size=int(seg * 0.9e6 / 8 * 1.0) if name.endswith(".avi") else None)
        if v < 2 and rng.random() < 0.6:
            rec.side(f"{folder}/{dotted(t).lower()}.nfo")
        return m["title"]

    # ===== shows =====
    def show_eps(self, s):
        return [e for e in s["episodes"] if e[0] >= 1 and e[1] >= 1]

    def show_name(self, rng, s, season, eps, style, ru=False):
        t = s["title"]
        if style == "scene":
            res = pick_res(rng)
            src = rng.choice(["WEB", "WEB-DL", "HDTV", "BluRay", "AMZN.WEB-DL", "NF.WEBRip"])
            grp = rng.choice(SCENE)
            code = f"S{season:02d}" + "".join(f"E{e:02d}" for e in eps)
            name = f"{dotted(t)}.{code}.{res}.{src}.{codec_tag(rng, res)[0]}-{grp}.mkv"
            parse = {"title": undot(dotted(t)), "season": season, "episodes": eps, "resolution": res,
                     "source": src, "release_group": grp}
        elif style == "plex":
            res = None
            code = f"S{season:02d}E{eps[0]:02d}" + ("".join(f"-E{e:02d}" for e in eps[1:]))
            name = f"{clean(t)} - {code}.mkv"
            parse = {"title": clean(t), "season": season, "episodes": eps}
        elif style == "x":
            res = None
            code = "-".join(f"{season}x{e:02d}" for e in eps)
            name = f"{clean(t)} {code}.avi"
            parse = {"title": clean(t), "season": season, "episodes": eps}
        else:
            raise ValueError(style)
        return name, parse, res

    def tpl_show(self, rng, rec, cat):
        pool = self.d.shows
        if cat == "A14":
            pool = [s for s in pool if "digits" in s["tags"]]
        if cat == "A7":
            pool = [s for s in pool if s.get("label_ru")]
        s = rng.choice(pool)
        rec.group = s["qid"]
        ref = rec.work(s["qid"], show_work(s))
        eps_all = self.show_eps(s)
        seasons = sorted({e[0] for e in eps_all})
        season = rng.choice(seasons)
        se = [e for e in eps_all if e[0] == season]
        k = rng.randint(2, min(6, len(se))) if len(se) >= 2 else 1
        start = rng.randrange(0, len(se) - k + 1)
        chosen = se[start:start + k]
        rt = (s.get("runtime_min") or 45) * 60
        multi = cat == "A9"
        style = rng.choice(["scene", "plex", "x"]) if cat != "A7" else "ru"
        ru = clean(s.get("label_ru") or "")
        is_ru = "ru" in s["tags"]
        dub = rng.choice(RU_DUB_SHOW)
        root_title = clean(s["title"])
        if style == "ru":
            v = rng.randrange(4)
            root = [f"Сериалы/{ru}/Сезон {season}", f"Сериалы/{ru} ∕ {root_title}/Сезон {season}",
                    f"Сериалы/{ru} (Сезон {season}, серии 1-{len(se)} из {len(se)}) [{dub}]",
                    f"TV/{root_title}/Season {season:02d}"][v]
            if is_ru and v == 1:
                root = f"Сериалы/{ru}/Сезон {season}"
        else:
            root = rng.choice([f"TV/{root_title}/Season {season:02d}", f"TV Shows/{root_title}/Season {season}",
                               f"Series/{dotted(s['title'])}.S{season:02d}"])
        i = 0
        while i < len(chosen):
            group = chosen[i:i + 2] if multi and i + 1 < len(chosen) and (i == 0 or rng.random() < 0.5) else chosen[i:i + 1]
            i += len(group)
            eps = [g[1] for g in group]
            if style == "ru":
                v2 = rng.randrange(4)
                if v2 == 0:
                    name = f"{ru} - {season} сезон {eps[0]} серия.mkv" if len(eps) == 1 else \
                        f"{ru} - {season} сезон {eps[0]}-{eps[-1]} серии.mkv"
                    parse = {"title": ru, "season": season, "episodes": eps}
                elif v2 == 1 and not is_ru:
                    code = f"S{season:02d}" + "".join(f"E{e:02d}" for e in eps)
                    st = dub.replace(" ", ".")
                    name = f"{dotted(s['title'])}.{code}.1080p.rus.{st}.mkv"
                    parse = {"title": undot(dotted(s["title"])), "season": season, "episodes": eps,
                             "resolution": "1080p", "dub_studio": dub}
                elif v2 == 2:
                    tl = dotted(translit(ru))
                    code = f"S{season:02d}" + "".join(f"E{e:02d}" for e in eps)
                    name = f"{tl}.{code}.WEB-DL.1080p.mkv"
                    parse = {"title": undot(tl), "season": season, "episodes": eps, "source": "WEB-DL",
                             "resolution": "1080p"}
                else:
                    name = f"{eps[0]:02d} серия.mkv" if len(eps) == 1 else f"{eps[0]:02d}-{eps[-1]:02d} серии.mkv"
                    parse = {"episodes": eps}
                    if "Сезон" in root:
                        parse["season"] = season
                    tt = root.split("/")[1]
                    parse["title"] = clean(tt.split(" (")[0].split(" ∕ ")[0])
                    if " ∕ " in tt:
                        parse["alt_titles"] = [tt.split(" ∕ ")[1]]
                res = "1080p" if "1080p" in name else None
            else:
                name, parse, res = self.show_name(rng, s, season, eps, style)
            num_eps = [{"type": "regular", "numbering": {"tmdb": {"season": season, "episode": e}}} for e in eps]
            rec.add(f"{root}/{name}", {"role": "main", "work": ref, "episodes": num_eps, "parse": parse},
                    dur=rt * len(eps) + rng.randint(-60, 60), res=res or "1080p", codec="h264",
                    alangs=["rus"] if is_ru else (["rus", "eng"] if style == "ru" else ["eng"]))
        rec.unchecked = True
        rec.notes.append("TMDB positions follow TVmaze aired order (season/number); not checked against the TMDB API.")
        if cat == "A8":
            pass
        return s["title"]

    def tpl_show_a8(self, rng, rec):
        groups = defaultdict(list)
        for s in self.d.shows:
            for t in s["tags"]:
                if t.startswith("remake:"):
                    groups[t].append(s)
        groups = {k: v for k, v in groups.items() if len(v) == 2}
        key = rng.choice(sorted(groups))
        a, b = groups[key]
        sa = {(e[0], e[1]) for e in self.show_eps(a)}
        sb = {(e[0], e[1]) for e in self.show_eps(b)}
        target = rng.choice([a, b])
        other = sb if target is a else sa
        cand = [e for e in self.show_eps(target) if (e[0], e[1]) not in other]
        if not cand:
            return None
        e = rng.choice(cand)
        rec.group = key
        ref = rec.work(target["qid"], show_work(target))
        t = clean(target["title"])
        style = rng.randrange(3)
        if style == 0:
            name = f"{t} - S{e[0]:02d}E{e[1]:02d}.mkv"
            parse = {"title": t, "season": e[0], "episodes": [e[1]]}
        elif style == 1:
            name = f"{dotted(t)}.S{e[0]:02d}E{e[1]:02d}.720p.HDTV.x264.mkv"
            parse = {"title": undot(dotted(t)), "season": e[0], "episodes": [e[1]], "resolution": "720p", "source": "HDTV"}
        else:
            name = f"{t} {e[0]}x{e[1]:02d}.avi"
            parse = {"title": t, "season": e[0], "episodes": [e[1]]}
        rt = (target.get("runtime_min") or 30) * 60
        rec.add(f"TV/{t}/{name}", {"role": "main", "work": ref, "parse": parse,
                                    "episodes": [{"type": "regular", "numbering": {"tmdb": {"season": e[0], "episode": e[1]}}}]},
                dur=rt + rng.randint(-60, 60), res="720p", codec="h264", alangs=["eng"])
        rec.unchecked = True
        rec.notes.append(f"Homonymous series ({a['year']} and {b['year']}); S{e[0]:02d}E{e[1]:02d} exists only in the {target['year']} one (TVmaze lists). TMDB positions from TVmaze aired order.")
        return target["title"]

    def tpl_daily(self, rng, rec):
        s = rng.choice(self.d.daily)
        rec.group = s["qid"]
        ref = rec.work(s["qid"], show_work(s))
        dates = Counter(e[2] for e in s["episodes"] if e[2])
        eps = [e for e in s["episodes"] if e[2] and dates[e[2]] == 1]
        if not eps:
            return None
        k = rng.randint(1, 4)
        start = rng.randrange(0, max(1, len(eps) - k))
        chosen = eps[start:start + k]
        is_ru = "ru" in s["tags"]
        t = clean(s["title"])
        ru = clean(s.get("label_ru") or t)
        style = rng.randrange(4) if not is_ru else rng.choice([4, 5, 0])
        rt = (s.get("runtime_min") or 45) * 60
        for e in chosen:
            d = e[2]
            yy, mm, dd = d.split("-")
            grp = rng.choice(SCENE)
            if style == 0:
                name = f"{dotted(t)}.{yy}.{mm}.{dd}.720p.WEB.h264-{grp}.mkv"
                parse = {"title": undot(dotted(t)), "date": d, "resolution": "720p", "source": "WEB", "release_group": grp}
                folder = f"TV/{t}"
            elif style == 1:
                name = f"{t} - {d}.mkv"
                parse = {"title": t, "date": d}
                folder = f"TV/{t}/Season {yy}"
            elif style == 2:
                name = f"{dotted(t)}.{yy}-{mm}-{dd}.1080p.WEB-DL.mkv"
                parse = {"title": undot(dotted(t)), "date": d, "resolution": "1080p", "source": "WEB-DL"}
                folder = f"Shows/{t}"
            elif style == 3:
                name = f"{t} ({dd}.{mm}.{yy}).mp4"
                parse = {"title": t, "date": d}
                folder = "Recordings"
            elif style == 4:
                name = f"{ru}. Выпуск от {dd}.{mm}.{yy}.mp4"
                parse = {"title": ru, "date": d}
                folder = f"ТВ/{ru}"
            else:
                name = f"{dotted(translit(ru))}.{yy}.{mm}.{dd}.WEB-DL.720p.mkv"
                parse = {"title": undot(dotted(translit(ru))), "date": d, "source": "WEB-DL", "resolution": "720p"}
                folder = f"ТВ/{ru}"
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": parse,
                                         "episodes": [{"type": "regular", "air_date": d}]},
                    dur=(e[3] or rt // 60) * 60 + rng.randint(-90, 30), res="720p" if "720p" in name else "1080p",
                    codec="h264", alangs=["rus"] if is_ru else ["eng"])
        if is_ru:
            rec.cats.append("A7")
        return s["title"]

    # ===== anime =====
    def fansub_name(self, rng, title, epstr, res, style, version=None, bd=False, grp=None):
        """Return (name, parse-extras)."""
        grp = grp or rng.choice(FANSUB)
        c = crc(rng)
        v = f"v{version}" if version else ""
        if style == 0:
            name = f"[{grp}] {title} - {epstr}{v} ({res}) [{c}].mkv"
            extra = {"release_group": grp, "crc32": c, "resolution": res}
        elif style == 1:
            name = f"[{grp}] {title} - {epstr}{v} [{res}][Multiple Subtitle][{c}].mkv"
            extra = {"release_group": grp, "crc32": c, "resolution": res}
        elif style == 2:
            name = f"[{grp}] {title} - {epstr}{v} [{'BD ' if bd else ''}{res}].mkv"
            extra = {"release_group": grp, "resolution": res}
        elif style == 3:
            name = f"[{grp}] {title} - {epstr}{v} [{'BD' if bd else 'WEB'} {res} HEVC 10bit AAC][{c}].mkv"
            extra = {"release_group": grp, "crc32": c, "resolution": res}
        else:
            name = f"{title} - {epstr}{v} [{res}].mkv"
            extra = {"resolution": res}
        if version:
            extra["version"] = version
        return name, extra, grp, c

    def anime_files(self, rng, rec, fr, entry, numbers, *, title, style, res, folder, abs_numbers=None,
                    written=None, parse_extra=None, alangs=("jpn",), slangs=("eng",), version_one=False, cats=None):
        """Add one main file per number (or per tuple for multi-episode)."""
        work = self.d.anime_work(fr, entry)
        ref = rec.work(f"aid:{entry['aid']}", work)
        has_tmdb = "tmdb" in work["ids"]
        sec = entry["fact"].get("episode_seconds") or 1440
        grp = rec.__dict__.setdefault("fansub", rng.choice(FANSUB))  # one group per release folder
        for i, n in enumerate(numbers):
            ns = n if isinstance(n, tuple) else (n,)
            wr = written[i] if written else ns
            epstr = "-".join(f"{x:02d}" if x < 100 else str(x) for x in wr)
            version = 2 if (version_one and i == 0) else None
            name, extra, _, c = self.fansub_name(rng, title, epstr, res, style, version, grp=grp)
            parse = {"title": title}
            if abs_numbers:
                parse["absolute"] = list(wr)
            else:
                parse["episodes"] = list(wr)
            parse.update(extra)
            if parse_extra:
                parse.update(parse_extra)
            eps = [self.d.anime_episode(fr, entry, x, has_tmdb) for x in ns]
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "episodes": eps, "parse": parse},
                    dur=sec * len(ns) + rng.randint(-20, 20), res=res, codec=rng.choice(["h264", "hevc"]),
                    alangs=list(alangs), slangs=list(slangs), anime=True)
        rec.unchecked = True
        return ref

    def pick_entry_numbers(self, rng, entry, k):
        total = entry["fact"]["episodes"]
        k = min(k, total)
        start = rng.randint(1, total - k + 1)
        return list(range(start, start + k))

    def anime_note(self, rec):
        rec.notes.append("AniDB entry and episode count from anime-offline-database; TMDB/TVDB positions follow "
                         "the maintainer rules in titles.yaml (not checked against the TMDB/TVDB APIs).")

    def tpl_anime_a1(self, rng, rec):
        """Absolute numbering in the name."""
        pool = [f for f in self.d.franchises if f.get("abs") and (f.get("long") or len(f["entries"]) > 1)]
        fr = rng.choice(pool)
        rec.group = fr["key"]
        total = sum(e["fact"]["episodes"] for e in fr["entries"])
        k = rng.randint(1, 4)
        start = rng.randint(1, total - k + 1)
        absn = list(range(start, start + k))
        title = fr["names"]["romaji"] if rng.random() < 0.7 or "en" not in fr["names"] else fr["names"]["en"]
        res = pick_res(rng, anime=True)
        style = rng.randrange(5)
        folder = f"Anime/{title}"
        by_entry = defaultdict(list)
        for a in absn:
            e = next(x for x in fr["entries"] if x["abs_offset"] < a <= x["abs_offset"] + x["fact"]["episodes"])
            by_entry[e["aid"]].append((e, a))
        for aid, lst in by_entry.items():
            e = lst[0][0]
            self.anime_files(rng, rec, fr, e, [a - e["abs_offset"] for _, a in lst], title=title, style=style,
                             res=res, folder=folder, abs_numbers=True, written=[(a,) for _, a in lst])
            if e["abs_offset"] > 0:
                rec.cats += ["A2", "A3"]
        if not fr.get("abs"):
            pass
        self.anime_note(rec)
        return title

    def tpl_anime_a2(self, rng, rec):
        """A later season named as its own title or as season N of the series."""
        pool = [(f, e) for f in self.d.franchises for i, e in enumerate(f["entries"]) if i > 0]
        fr, e = rng.choice(pool)
        rec.group = fr["key"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 5))
        res = pick_res(rng, anime=True)
        v = rng.randrange(3)
        if v == 0 or not e.get("tmdb_season"):
            title = e["names"]["romaji"] if rng.random() < 0.7 or "en" not in e["names"] else e["names"]["en"]
            self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.randrange(5), res=res,
                             folder=f"Anime/{title}")
        elif v == 1:
            # scene style SxxEyy with TMDB season numbering
            title = fr["names"].get("en") or fr["names"]["romaji"]
            work = self.d.anime_work(fr, e)
            ref = rec.work(f"aid:{e['aid']}", work)
            s = e["tmdb_season"]
            for n in nums:
                te = e.get("tmdb_offset", 0) + n
                grp = rng.choice(SCENE + FANSUB[:5])
                name = f"{dotted(title)}.S{s:02d}E{te:02d}.{res}.WEB.H.264-{grp}.mkv"
                parse = {"title": undot(dotted(title)), "season": s, "episodes": [te], "resolution": res,
                         "source": "WEB", "release_group": grp}
                rec.add(f"TV/{clean(title)}/Season {s:02d}/{name}", {"role": "main", "work": ref, "parse": parse,
                        "episodes": [self.d.anime_episode(fr, e, n, "tmdb" in work["ids"])]},
                        dur=1440 + rng.randint(-20, 20), res=res, codec="h264", alangs=["jpn"], slangs=["eng"], anime=True)
                if te != n:
                    rec.cats.append("A3")
            rec.unchecked = True
        else:
            title = fr["names"]["romaji"]
            s = e.get("tmdb_season")
            folder = f"Anime/{title}/Season {s}"
            self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.randrange(5), res=res, folder=folder,
                             written=[(e.get("tmdb_offset", 0) + n,) for n in nums])
            for lab in rec.labels:
                lab["parse"]["season"] = s
            if e.get("tmdb_offset"):
                rec.cats.append("A3")
        self.anime_note(rec)
        return e["names"]["romaji"]

    def tpl_anime_a3(self, rng, rec):
        """Releaser numbering differs from AniDB in-cour numbering (continuing count or TMDB season count)."""
        pool = [(f, e) for f in self.d.franchises for e in f["entries"] if e.get("tmdb_offset")
                or (f.get("abs") and e["abs_offset"] > 0)]
        fr, e = rng.choice(pool)
        rec.group = fr["key"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 4))
        res = pick_res(rng, anime=True)
        if e.get("tmdb_offset") and rng.random() < 0.6:
            written = [(e["tmdb_offset"] + n,) for n in nums]
            base = fr["entries"][[x["aid"] for x in fr["entries"]].index(e["aid"]) - 1]["names"]
            title = base["romaji"]
            self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.randrange(5), res=res,
                             folder=f"Anime/{title}", written=written)
            rec.notes.append(f"Releaser continued the count of TMDB season {e['tmdb_season']}; AniDB restarts at 1.")
        else:
            written = [(e["abs_offset"] + n,) for n in nums]
            title = fr["names"]["romaji"]
            self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.randrange(5), res=res,
                             folder=f"Anime/{title}", written=written, abs_numbers=True)
            rec.cats.append("A1")
            rec.notes.append("Releaser used the running count; AniDB restarts at 1 in this entry.")
        rec.cats.append("A2")
        self.anime_note(rec)
        return title

    def tpl_anime_a16(self, rng, rec):
        """SxxEyy in TMDB order for a franchise whose absolute order differs."""
        pool = [(f, e) for f in self.d.franchises for e in f["entries"]
                if f.get("abs") and e.get("tmdb_season") and e["abs_offset"] > 0 and f.get("tmdb")]
        fr, e = rng.choice(pool)
        rec.group = fr["key"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 4))
        work = self.d.anime_work(fr, e)
        ref = rec.work(f"aid:{e['aid']}", work)
        s = e["tmdb_season"]
        title = fr["names"].get("en") or fr["names"]["romaji"]
        res = pick_res(rng, anime=True)
        v = rng.randrange(3)
        for n in nums:
            te = e.get("tmdb_offset", 0) + n
            if v == 0:
                name = f"{clean(title)} - S{s:02d}E{te:02d} - {res}.mkv"
                parse = {"title": clean(title), "season": s, "episodes": [te], "resolution": res}
            elif v == 1:
                grp = rng.choice(FANSUB)
                name = f"[{grp}] {fr['names']['romaji']} S{s:02d}E{te:02d} [{res}].mkv"
                parse = {"title": fr["names"]["romaji"], "season": s, "episodes": [te], "resolution": res,
                         "release_group": grp}
            else:
                name = f"{clean(title)} {s}x{te:02d}.mkv"
                parse = {"title": clean(title), "season": s, "episodes": [te]}
            folder = f"TV/{clean(title)}/Season {s:02d}"
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": parse,
                    "episodes": [self.d.anime_episode(fr, e, n, True)]},
                    dur=1440 + rng.randint(-20, 20), res=res, codec="h264", alangs=["jpn", "eng"], slangs=["eng"],
                    anime=True)
        rec.unchecked = True
        rec.cats.append("A2")
        self.anime_note(rec)
        return title

    def tpl_anime_a4(self, rng, rec):
        """OVA/ONA/special works and NCOP/NCED extras."""
        v = rng.random()
        if v < 0.5:
            pool = [f for f in self.d.franchises if f["entries"][0]["fact"]["anime_type"] in ("ova", "ona", "special")]
            fr = rng.choice(pool)
            e = fr["entries"][0]
            rec.group = fr["key"]
            nums = self.pick_entry_numbers(rng, e, rng.randint(1, 4))
            title = fr["names"]["romaji"] if rng.random() < 0.7 or "en" not in fr["names"] else fr["names"]["en"]
            self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.randrange(5), res=pick_res(rng, anime=True),
                             folder=f"Anime/{title}")
        else:
            fr = rng.choice([f for f in self.d.franchises if f["entries"][0]["fact"]["anime_type"] == "tv"])
            e = rng.choice(fr["entries"])
            rec.group = fr["key"]
            title = e["names"]["romaji"]
            grp = rng.choice(FANSUB)
            folder = f"Anime/[{grp}] {title} (BD 1080p)"
            nums = self.pick_entry_numbers(rng, e, rng.randint(1, 3))
            work = self.d.anime_work(fr, e)
            ref = rec.work(f"aid:{e['aid']}", work)
            for n in nums:
                c = crc(rng)
                name = f"[{grp}] {title} - {n:02d} (BD 1080p) [{c}].mkv"
                rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": {
                    "title": title, "episodes": [n], "release_group": grp, "crc32": c, "resolution": "1080p",
                    "source": "BD"}, "episodes": [self.d.anime_episode(fr, e, n, "tmdb" in work["ids"])]},
                    dur=1440, res="1080p", codec="hevc", alangs=["jpn"], slangs=["eng"], anime=True)
            for kind, et in rng.sample([("NCOP", "opening"), ("NCED", "ending"), ("NCOP2", "opening"),
                                        ("NCED2", "ending")], rng.randint(1, 3)):
                c = crc(rng)
                sub = rng.choice(["", "Extras/"])
                name = f"[{grp}] {title} - {kind} (BD 1080p) [{c}].mkv"
                rec.add(f"{folder}/{sub}{name}", {"role": "extra", "extra_type": et, "work": ref, "parse": {
                    "title": title, "episode_kind": kind[:4].lower(), "release_group": grp, "crc32": c,
                    "resolution": "1080p", "source": "BD"}},
                    dur=rng.randint(85, 95), res="1080p", codec="hevc", alangs=["jpn"], anime=True)
            rec.cats += ["A6", "A10"]
            rec.unchecked = True
        self.anime_note(rec)
        return fr["names"]["romaji"]

    def tpl_anime_a5(self, rng, rec):
        """Romaji folder, English/Russian/Japanese file names (or the other way round)."""
        pool = [f for f in self.d.franchises if "en" in f["names"] or "ru" in f["names"]]
        fr = rng.choice(pool)
        e = fr["entries"][0]
        rec.group = fr["key"]
        names = [fr["names"][k] for k in ("romaji", "en", "ru") if k in fr["names"]]
        folder_t, file_t = rng.sample(names, 2)
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 4))
        self.anime_files(rng, rec, fr, e, nums, title=file_t, style=rng.randrange(5),
                         res=pick_res(rng, anime=True), folder=f"Anime/{folder_t}",
                         parse_extra={"alt_titles": [folder_t]})
        if any(re.search("[а-яё]", n, re.I) for n in (folder_t, file_t)):
            rec.cats.append("A7")
        self.anime_note(rec)
        return fr["names"]["romaji"]

    def tpl_anime_a6(self, rng, rec):
        fr = rng.choice(self.d.franchises)
        e = rng.choice(fr["entries"])
        rec.group = fr["key"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(2, 6))
        title = e["names"]["romaji"]
        style = rng.choice([0, 1, 3])
        self.anime_files(rng, rec, fr, e, nums, title=title, style=style, res=pick_res(rng, anime=True),
                         folder=f"Anime/{title}", version_one=rng.random() < 0.3)
        self.anime_note(rec)
        return title

    def tpl_anime_a7(self, rng, rec):
        """Russian anime releases (dub studios, Cyrillic titles, tracker-style folders)."""
        fr = rng.choice([f for f in self.d.franchises if "ru" in f["names"]])
        e = fr["entries"][0] if rng.random() < 0.7 else rng.choice(fr["entries"])
        rec.group = fr["key"]
        ru, ro = fr["names"]["ru"], e["names"]["romaji"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 4))
        dub = rng.choice(RU_DUB_ANIME)
        work = self.d.anime_work(fr, e)
        ref = rec.work(f"aid:{e['aid']}", work)
        total = e["fact"]["episodes"]
        v = rng.randrange(4)
        if v == 0:
            folder = f"Аниме/{clean(ru)} ∕ {ro} [TV] [{total} из {total}] [RUS(ext), JAP+Sub]"
        elif v == 1:
            folder = f"Аниме/{clean(ru)}"
        elif v == 2:
            folder = f"Anime/{ro} [{dub}]"
        else:
            folder = f"Аниме/{ro}"
        for n in nums:
            if v == 0:
                name = f"[{dub}] {ro} [{n:02d}] [1080p].mkv"
                parse = {"title": ro, "episodes": [n], "dub_studio": dub, "resolution": "1080p",
                         "alt_titles": [clean(ru)]}
            elif v == 1:
                name = f"{clean(ru)} - {n} серия [{dub}].mkv"
                parse = {"title": clean(ru), "episodes": [n], "dub_studio": dub}
            elif v == 2:
                name = f"{ro} - {n:02d} [WEBRip 1080p].mkv"
                parse = {"title": ro, "episodes": [n], "dub_studio": dub, "resolution": "1080p", "source": "WEBRip"}
            else:
                name = f"{dotted(translit(clean(ru)))}.{n:02d}.seriya.{dub.replace(' ', '.')}.mkv"
                parse = {"title": undot(dotted(translit(clean(ru)))), "episodes": [n], "dub_studio": dub,
                         "alt_titles": [ro]}
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": parse,
                    "episodes": [self.d.anime_episode(fr, e, n, "tmdb" in work["ids"])]},
                    dur=(e["fact"].get("episode_seconds") or 1440) + rng.randint(-20, 20), res="1080p", codec="h264",
                    alangs=["rus", "jpn"], slangs=["rus"], anime=True)
            if v == 0 and rng.random() < 0.5:
                rec.side(f"{folder}/RUS Sub/[{dub}] {ro} [{n:02d}].ass")
        rec.unchecked = True
        self.anime_note(rec)
        return ro

    def tpl_anime_a8(self, rng, rec):
        """Homonymous anime (remakes); file names without year, the episode number or count tells them apart."""
        groups = defaultdict(list)
        for f in self.d.franchises:
            if f.get("homonym"):
                groups[f["homonym"]].append(f)
        groups = {k: v for k, v in groups.items() if len(v) >= 2}
        key = rng.choice(sorted(groups))
        frs = groups[key]
        fr = rng.choice(frs)
        e = fr["entries"][0]
        others = max(o["entries"][0]["fact"]["episodes"] for o in frs if o is not fr)
        total = e["fact"]["episodes"]
        if total > others and rng.random() < 0.6:
            nums = sorted(rng.sample(range(others + 1, total + 1), min(rng.randint(1, 3), total - others)))
            rec.notes.append(f"No year; episode numbers above {others} exist only in this entry.")
        else:
            nums = self.pick_entry_numbers(rng, e, rng.randint(2, 5))
            rec.notes.append("No year; homonymous entries are told apart by context (runtime, video, episode count).")
        rec.group = key
        title = fr["names"]["romaji"]
        title = re.sub(r" \(\d{4}\)$", "", title)
        old = (e["fact"].get("year") or 2000) < 2006
        self.anime_files(rng, rec, fr, e, nums, title=title, style=rng.choice([2, 4]),
                         res="480p" if old else "1080p", folder=f"Anime/{title}")
        self.anime_note(rec)
        return title

    def tpl_anime_a9(self, rng, rec):
        fr = rng.choice([f for f in self.d.franchises])
        e = rng.choice(fr["entries"])
        if e["fact"]["episodes"] < 4:
            return None
        rec.group = fr["key"]
        k = rng.randint(1, 3)
        nums = self.pick_entry_numbers(rng, e, 2 * k)
        pairs = [tuple(nums[i:i + 2]) for i in range(0, len(nums), 2)]
        title = e["names"]["romaji"]
        self.anime_files(rng, rec, fr, e, pairs, title=title, style=rng.randrange(5),
                         res=pick_res(rng, anime=True), folder=f"Anime/{title}")
        self.anime_note(rec)
        return title

    def tpl_anime_a10(self, rng, rec):
        fr = rng.choice([f for f in self.d.franchises if f["entries"][0]["fact"]["anime_type"] == "tv"])
        e = fr["entries"][0]
        rec.group = fr["key"]
        title = e["names"]["romaji"]
        grp = rng.choice(FANSUB)
        folder = f"Anime/[{grp}] {title} [BD 1080p]"
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 3))
        work = self.d.anime_work(fr, e)
        ref = rec.work(f"aid:{e['aid']}", work)
        for n in nums:
            name = f"[{grp}] {title} - {n:02d} [BD 1080p].mkv"
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": {
                "title": title, "episodes": [n], "release_group": grp, "resolution": "1080p", "source": "BD"},
                "episodes": [self.d.anime_episode(fr, e, n, "tmdb" in work["ids"])]},
                dur=1440, res="1080p", codec="hevc", alangs=["jpn"], slangs=["eng"], anime=True)
        extras = rng.sample([("Menu", "menu", 30), ("PV", "trailer", 90), ("CM", "other", 30), ("Preview", "preview", 30),
                             ("Teaser", "teaser", 60), ("NCOP", "opening", 90), ("NCED", "ending", 90)], rng.randint(2, 4))
        for i, (tag, et, d) in enumerate(extras):
            sub = rng.choice(["Extras", "SPs", "Bonus"])
            name = f"[{grp}] {title} - {tag}{rng.randint(1, 3):02d} [BD 1080p].mkv"
            rec.add(f"{folder}/{sub}/{name}", {"role": "extra", "extra_type": et, "work": ref},
                    dur=d + rng.randint(-5, 5), res="1080p", codec="hevc", alangs=["jpn"], anime=True)
        if rng.random() < 0.4:
            rec.side(f"{folder}/Scans/Booklet_{rng.randint(1, 9):02d}.jpg", size=rng.randint(10**6, 4 * 10**6))
        rec.cats.append("A6")
        if any(et in ("opening", "ending") for _, et, _ in extras):
            rec.cats.append("A4")
        rec.unchecked = True
        self.anime_note(rec)
        return title

    # ===== anime movies =====
    def am_name(self, rng, m, style):
        f = m["fact"]
        y = f["year"]
        names = m["names"]
        t = names.get("en") if (style in (1, 2) and names.get("en")) else names["romaji"]
        t = clean(t)
        res = rng.choice(["1080p", "1080p", "2160p", "720p"])
        if style == 0:
            grp = rng.choice(FANSUB)
            c = crc(rng)
            name = f"[{grp}] {t} ({y}) [BD {res}] [{c}].mkv"
            parse = {"title": t, "year": y, "release_group": grp, "crc32": c, "resolution": res, "source": "BD"}
        elif style == 1:
            grp = rng.choice(SCENE)
            name = f"{dotted(t)}.{y}.{res}.BluRay.x264-{grp}.mkv"
            parse = {"title": undot(dotted(t)), "year": y, "resolution": res, "source": "BluRay", "release_group": grp}
        elif style == 2:
            name = f"{t} ({y}).mkv"
            parse = {"title": t, "year": y}
        else:
            name = f"{t} [{res}].mkv"
            parse = {"title": t, "resolution": res}
        return name, parse, res

    def tpl_anime_movie(self, rng, rec, cat):
        pool = self.d.anime_movies
        if cat == "A14":
            pool = [m for m in pool if m.get("digits")]
        m = rng.choice(pool)
        rec.group = f"am:{m['aid']}"
        ref = rec.work(f"aid:{m['aid']}", self.d.anime_movie_work(m))
        dur = (m["fact"].get("episode_seconds") or 6000) + rng.randint(-30, 30)
        if cat == "A5" or (cat == "A17" and rng.random() < 0.2):
            ja = m["fact"].get("label_ja")
            alts = [x for x in (m["names"].get("en"), m["names"].get("ru"), ja) if x]
            if not alts:
                return None
            other = clean(rng.choice(alts))
            y = m["fact"]["year"]
            ro = clean(m["names"]["romaji"])
            if rng.random() < 0.5:
                folder, name, parse = f"Anime Movies/{ro} ({y})", f"{other} ({y}).mkv", {"title": other, "year": y, "alt_titles": [ro]}
            else:
                folder, name, parse = f"Anime Movies/{other}", f"{ro} ({y}) [1080p].mkv", {"title": ro, "year": y, "resolution": "1080p", "alt_titles": [other]}
            rec.add(f"{folder}/{name}", {"role": "main", "work": ref, "parse": parse}, dur=dur, res="1080p",
                    codec="h264", alangs=["jpn"], slangs=["eng"], anime=True)
            if cat != "A5":
                rec.cats.append("A5")
            if re.search("[а-яё]", other, re.I):
                rec.cats.append("A7")
            return m["names"]["romaji"]
        if cat == "A12":
            t, y = clean(m["names"].get("en") or m["names"]["romaji"]), m["fact"]["year"]
            root = f"Anime/{t} ({y}) [BDMV]"
            for p, s in [("BDMV/index.bdmv", 280), ("BDMV/MovieObject.bdmv", 6000), ("BDMV/PLAYLIST/00001.mpls", 900),
                         ("CERTIFICATE/id.bdmv", 100)]:
                rec.files.append({"path": f"{root}/{p}", "size": s})
            rec.files.append({"path": f"{root}/BDMV/STREAM/00001.m2ts", "size": int(dur * 4.5e6), "duration_ms": dur * 1000,
                              "container": "mpegts", "video": {"width": 1920, "height": 1080, "codec": "h264"},
                              "audio_langs": ["jpn"]})
            rec.files.append({"path": f"{root}/BDMV/STREAM/00010.m2ts", "size": rng.randint(20, 90) * 10**6,
                              "duration_ms": rng.randint(30, 120) * 1000, "container": "mpegts"})
            rec.labels.append({"path": root, "unit": "bdmv", "role": "main", "work": ref,
                               "parse": {"title": t, "year": y}})
            return m["names"]["romaji"]
        style = rng.randrange(4)
        name, parse, res = self.am_name(rng, m, style)
        folder = rng.choice(["Anime Movies", "Anime/Movies", "Movies/Anime", "Аниме/Фильмы"])
        path = f"{folder}/{name}" if rng.random() < 0.5 else f"{folder}/{name.rsplit('.', 1)[0]}/{name}"
        rec.add(path, {"role": "main", "work": ref, "parse": parse}, dur=dur, res=res,
                codec=rng.choice(["h264", "hevc"]), alangs=["jpn"] + (["eng"] if rng.random() < 0.4 else []),
                slangs=["eng"], anime=True)
        if cat == "A10" or rng.random() < 0.15:
            base = path.rsplit("/", 1)[0]
            rec.add(f"{base}/Extras/Trailer {rng.randint(1, 3)}.mkv", {"role": "extra", "extra_type": "trailer", "work": ref},
                    dur=rng.randint(60, 150), res="1080p", codec="h264", alangs=["jpn"], anime=True)
            rec.cats.append("A10")
        return m["names"]["romaji"]

    def tpl_anime_franchise_movie(self, rng, rec):
        """Franchise movie sitting in the TV series folder (A17 + A15)."""
        pool = [m for m in self.d.anime_movies if m.get("franchise") in self.d.fr_by_key]
        m = rng.choice(pool)
        fr = self.d.fr_by_key[m["franchise"]]
        e = fr["entries"][0]
        rec.group = fr["key"]
        title = fr["names"]["romaji"]
        nums = self.pick_entry_numbers(rng, e, rng.randint(1, 3))
        res = pick_res(rng, anime=True)
        folder = f"Anime/{title}"
        self.anime_files(rng, rec, fr, e, nums, title=title, style=2, res=res, folder=folder)
        ref = rec.work(f"aid:{m['aid']}", self.d.anime_movie_work(m))
        mt = clean(m["names"]["romaji"])
        y = m["fact"]["year"]
        sub = rng.choice(["Movies/", "Movie/", "", "Specials/"])
        grp = rng.choice(FANSUB)
        name = f"[{grp}] {mt} ({y}) [BD {res}].mkv"
        rec.add(f"{folder}/{sub}{name}", {"role": "main", "work": ref, "parse": {
            "title": mt, "year": y, "release_group": grp, "resolution": res, "source": "BD"}},
            dur=(m["fact"].get("episode_seconds") or 6000) + rng.randint(-30, 30), res=res, codec="hevc",
            alangs=["jpn"], slangs=["eng"], anime=True)
        rec.cats += ["A15", "A6"]
        rec.notes.append("Franchise movie in the series folder is its own work (GUIDE §10.2).")
        self.anime_note(rec)
        return title

    # ===== dumps =====
    def personal(self, rng):
        import datetime
        d = datetime.date(2015, 1, 1) + datetime.timedelta(days=rng.randrange(3800))
        fmt = rng.choice(PERSONAL)
        return fmt.format(d=d, t=f"{rng.randrange(24):02d}{rng.randrange(60):02d}{rng.randrange(60):02d}",
                          n=rng.randint(1000, 9999), h=f"{rng.randrange(24):02d}", m=f"{rng.randrange(60):02d}",
                          s=f"{rng.randrange(60):02d}", ms=f"{rng.randrange(1000):03d}")

    def tpl_dump(self, rng, rec):
        root = rng.choice(["Downloads", "Загрузки", "Video", "Desktop/new", "Torrents/complete", "NAS/incoming"])
        n_movies = rng.randint(2, 3)
        allow_eps = rng.random() < 0.35
        for m in rng.sample(self.d.movies, n_movies):
            style = rng.choice(["scene", "plain", "brackets"])
            name, parse, res, ctag = movie_main_name(rng, m, style)
            ref = rec.work(m["qid"], movie_work(m))
            p = f"{root}/{name}" if rng.random() < 0.6 else f"{root}/{name.rsplit('.', 1)[0]}/{name}"
            rec.add(p, {"role": "main", "work": ref, "parse": parse}, dur=runtime_s(m, rng), res=res,
                    codec="mpeg4" if ctag in ("XviD", "DivX") else "h264", alangs=["eng"])
        if rng.random() < 0.6:
            am = rng.choice(self.d.anime_movies)
            name, parse, res = self.am_name(rng, am, rng.randrange(4))
            ref = rec.work(f"aid:{am['aid']}", self.d.anime_movie_work(am))
            rec.add(f"{root}/{name}", {"role": "main", "work": ref, "parse": parse},
                    dur=(am["fact"].get("episode_seconds") or 6000), res=res, codec="hevc", alangs=["jpn"], anime=True)
            rec.cats.append("A17")
        if allow_eps:
            s = rng.choice(self.d.shows)
            e = rng.choice(self.show_eps(s))
            name, parse, res = self.show_name(rng, s, e[0], [e[1]], "scene")
            ref = rec.work(s["qid"], show_work(s))
            rec.add(f"{root}/{name}", {"role": "main", "work": ref, "parse": parse,
                    "episodes": [{"type": "regular", "numbering": {"tmdb": {"season": e[0], "episode": e[1]}}}]},
                    dur=(s.get("runtime_min") or 45) * 60, res=res, codec="h264", alangs=["eng"])
            rec.unchecked = True
            rec.notes.append("TMDB positions of the episode follow TVmaze aired order.")
        for _ in range(rng.randint(1, 2)):
            p = f"{root}/{self.personal(rng)}"
            rec.add(p, {"role": "unknown"}, dur=rng.randint(5, 900), res=rng.choice(["1080p", "720p"]),
                    codec="h264", alangs=["und"])
        if rng.random() < 0.3:
            rec.add(f"{root}/{rng.choice(['clip', 'out', 'test', 'tmp'])}.mp4", {"role": "ignore"},
                    dur=rng.randint(1, 15), res="480p", codec="h264")
        if rng.random() < 0.3:
            rec.side(f"{root}/readme.txt")
        return "dump"


# ---------------------------------------------------------------- driver

QUOTAS = {  # records per primary category (secondary categories come on top)
    "A1": 120, "A2": 115, "A3": 110, "A4": 120, "A5": 120, "A6": 120, "A7": 150, "A8": 130,
    "A9": 120, "A10": 120, "A11": 120, "A12": 115, "A13": 110, "A14": 130, "A15": 115, "A16": 100, "A17": 120,
}


def templates(g: Gen):
    return {
        "A1": [(g.tpl_anime_a1, 1)],
        "A2": [(g.tpl_anime_a2, 1)],
        "A3": [(g.tpl_anime_a3, 1)],
        "A4": [(g.tpl_anime_a4, 1)],
        "A5": [(g.tpl_movie_a5, 2), (g.tpl_anime_a5, 2), (lambda r, x: g.tpl_anime_movie(r, x, "A5"), 1)],
        "A6": [(g.tpl_movie_a6, 2), (g.tpl_anime_a6, 2)],
        "A7": [(g.tpl_movie_a7, 3), (lambda r, x: g.tpl_show(r, x, "A7"), 2), (g.tpl_anime_a7, 2)],
        "A8": [(g.tpl_movie_a8, 3), (g.tpl_show_a8, 1), (g.tpl_anime_a8, 1)],
        "A9": [(lambda r, x: g.tpl_show(r, x, "A9"), 1), (g.tpl_anime_a9, 1)],
        "A10": [(g.tpl_movie_a10, 3), (g.tpl_anime_a10, 2), (lambda r, x: g.tpl_anime_movie(r, x, "A10"), 1)],
        "A11": [(g.tpl_daily, 1)],
        "A12": [(g.tpl_movie_a12, 4), (lambda r, x: g.tpl_anime_movie(r, x, "A12"), 1)],
        "A13": [(g.tpl_movie_a13, 1)],
        "A14": [(g.tpl_movie_a14, 3), (lambda r, x: g.tpl_show(r, x, "A14"), 1),
                (lambda r, x: g.tpl_anime_movie(r, x, "A14"), 1)],
        "A15": [(g.tpl_dump, 3), (g.tpl_anime_franchise_movie, 1)],
        "A16": [(g.tpl_anime_a16, 1)],
        "A17": [(lambda r, x: g.tpl_anime_movie(r, x, "A17"), 3), (g.tpl_anime_franchise_movie, 1)],
    }


def derive_categories(primary, rec: Rec):
    cats = [primary] + [c for c in rec.cats if c != primary]
    labels = rec.labels
    parses = [l.get("parse", {}) for l in labels]
    if any(p.get("release_group") or p.get("crc32") for p in parses):
        cats.append("A6")
    if any(l["role"] in ("extra", "sample") for l in labels):
        cats.append("A10")
    if any(l.get("unit") in ("bdmv", "video_ts") or l["path"].lower().endswith(".iso") for l in labels):
        cats.append("A12")
    if any(l["role"] == "part" for l in labels):
        cats.append("A13")
    if any(len(l.get("episodes", [])) >= 2 for l in labels):
        cats.append("A9")
    if len(rec.works) >= 2:
        cats.append("A15")
    if any(w["kind"] == "anime" and w.get("anime_type") == "movie" for w in rec.works):
        cats.append("A17")
    if any(w.get("anime_type") in ("ova", "ona", "special") for w in rec.works) or \
            any(l.get("extra_type") in ("opening", "ending") for l in labels):
        cats.append("A4")
    if any("absolute" in p for p in parses):
        cats.append("A1")
    if any(p.get("dub_studio") for p in parses) or any(re.search("[а-яё]", f["path"], re.I) for f in rec.files):
        cats.append("A7")
    if any(p.get("alt_titles") for p in parses):
        cats.append("A5")
    if any("air_date" in e for l in labels for e in l.get("episodes", [])):
        cats.append("A11")
    if any(p.get("title") and has_digit(p["title"]) and not re.search(r"\b(19|20)\d\d\b", p["title"]) and
           any(has_digit(w["title"]) for w in rec.works) for p in parses):
        cats.append("A14")
    # A16: TMDB season order and TVDB absolute order disagree
    for l in labels:
        for e in l.get("episodes", []):
            n = e.get("numbering", {})
            if "tmdb" in n and "tvdb_absolute" in n and n["tvdb_absolute"] != n["tmdb"]["episode"]:
                cats.append("A16")
    out = []
    for c in cats:
        if c not in out:
            out.append(c)
    # drop categories whose promise the record does not keep (validator rules)
    return out


def finalize(rec: Rec, primary, idx, title):
    cats = derive_categories(primary, rec)
    # A3 needs an episode in two schemes, A2 needs a later AniDB entry: keep only when true
    if "A3" in cats and not any(len(e.get("numbering", {})) >= 2 for l in rec.labels for e in l.get("episodes", [])):
        cats.remove("A3")
    if "A16" in cats and not any(sum(k in e.get("numbering", {}) for k in ("tmdb", "tvdb_absolute", "tvdb_aired",
                                                                          "tvdb_dvd", "tmdb_group")) >= 2
                                 for l in rec.labels for e in l.get("episodes", [])):
        cats.remove("A16")
    if cats[0] != primary:
        return None
    # status
    verified = not rec.unchecked
    for w in rec.works:
        canon = w["ids"].get("anidb") if w["kind"] == "anime" else w["ids"].get("tmdb")
        if not canon:
            verified = False
    rid = f"v0-{primary.lower()}-{slug(title)}-{idx:03d}"
    review = {"status": "verified" if verified else "draft", "annotators": [ANNOTATOR]}
    notes = list(dict.fromkeys(rec.notes))
    if notes:
        review["notes"] = " ".join(notes)
    # stable label order: by path
    order = {f["path"]: i for i, f in enumerate(rec.files)}
    labels = sorted(rec.labels, key=lambda l: (order.get(l["path"], -1), l["path"]))
    return {
        "schema_version": 1,
        "id": rid,
        "categories": cats,
        "split": None,
        "source": {"kind": "synthetic", "license": LICENSE,
                   "origin": "META-002 generator (tools/recognition-eval/corpus/gen) over a hand-curated title list"},
        "input": {"files": rec.files},
        "labels": {"works": rec.works, "files": labels},
        "review": review,
    }, rec.group


def split_of(group, seed):
    h = int(hashlib.sha256(f"split:{seed}:{group}".encode()).hexdigest(), 16) % 100
    return "train" if h < 60 else "dev" if h < 80 else "test"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--titles", default=os.path.join(HERE, "titles.yaml"))
    ap.add_argument("--facts", default=os.path.join(HERE, "facts.json"))
    ap.add_argument("--out-dir", default=os.path.join(HERE, "..", "v0"))
    ap.add_argument("--seed", type=int, default=2)
    args = ap.parse_args()
    titles = yaml.safe_load(open(args.titles, encoding="utf-8"))
    facts = json.load(open(args.facts, encoding="utf-8"))
    data = Data(titles, facts)
    gen = Gen(data, args.seed)
    tpls = templates(gen)
    records = []
    seen = set()
    for cat, quota in QUOTAS.items():
        made, attempt = 0, 0
        while made < quota and attempt < quota * 40:
            rng = gen.rng_for(cat, attempt)
            attempt += 1
            choices = tpls[cat]
            fn = rng.choices([c[0] for c in choices], weights=[c[1] for c in choices])[0]
            rec = Rec(rng, None)
            try:
                title = fn(rng, rec)
            except (ValueError, IndexError, StopIteration):
                continue
            if title is None or not rec.files:
                continue
            sig = tuple(sorted(f["path"] for f in rec.files))
            if sig in seen or len({f["path"] for f in rec.files}) != len(rec.files):
                continue
            out = finalize(rec, cat, made + 1, title)
            if out is None:
                continue
            seen.add(sig)
            r, group = out
            r["split"] = split_of(group, args.seed)
            records.append(r)
            made += 1
        if made < quota:
            print(f"warning: {cat} produced {made}/{quota}")
    os.makedirs(args.out_dir, exist_ok=True)
    by_split = defaultdict(list)
    for r in records:
        by_split[r["split"]].append(r)
    for sp in ("train", "dev", "test"):
        rows = sorted(by_split[sp], key=lambda r: r["id"])
        with open(os.path.join(args.out_dir, f"{sp}.jsonl"), "w", encoding="utf-8") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n")
    cat_counts = Counter(c for r in records for c in r["categories"])
    status = Counter(r["review"]["status"] for r in records)
    print(f"records: {len(records)}; splits: { {k: len(v) for k, v in sorted(by_split.items())} }; status: {dict(status)}")
    print("per category:", dict(sorted(cat_counts.items(), key=lambda kv: int(kv[0][1:]))))


if __name__ == "__main__":
    main()
