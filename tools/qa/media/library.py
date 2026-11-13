#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Builds a small fake media library: realistic folder and file names, tiny synthetic content.

For release 0.1 screenshots, demos and scanner/parser tests
(docs/dev/release-0.1-contract.md): movies, series, anime and music laid out
the way real users keep them, including messy scene-style names. Films and
series are (almost all) public-domain or Creative Commons works, so TMDB
posters in screenshots are of free works; anime and music names are made up.
The content of every file is a test pattern + tone.

    python3 tools/qa/media/library.py [--out tests/media/library] [--seconds 30]

Every video file is the same tiny H.264/AAC clip (hard-linked when possible),
so the whole tree is a few MB. Nothing here is committed.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
FF = ["nice", "-n", "10", "ffmpeg", "-hide_banner", "-nostdin", "-loglevel", "error", "-y"]

# (relative path, kind). Names follow common real-world conventions, on purpose
# inconsistent: Plex/Jellyfin style folders, scene names, Russian titles, extras.
MOVIES = [
    "Movies/Big Buck Bunny (2008)/Big Buck Bunny (2008).mkv",
    "Movies/Sintel (2010)/Sintel (2010) - 1080p.mkv",
    "Movies/Sintel (2010)/Sintel (2010) - 1080p.ru.srt",
    "Movies/Tears of Steel (2012)/Tears.of.Steel.2012.1080p.BluRay.x264-GRP.mkv",
    "Movies/Elephants Dream (2006)/Elephants Dream (2006).mp4",
    "Movies/Spring (2019)/Spring.2019.2160p.HDR.WEB-DL.HEVC.mkv",
    "Movies/Cosmos Laundromat (2015)/Cosmos Laundromat (2015).mkv",
    "Movies/Cosmos Laundromat (2015)/Cosmos Laundromat (2015)-trailer.mp4",
    "Movies/Night of the Living Dead (1968)/Night of the Living Dead (1968).avi",
    "Movies/Nosferatu (1922)/Nosferatu.1922.Restored.720p.mkv",
    "Movies/The General (1926)/The General (1926).mkv",
    "Movies/Metropolis (1927)/Metropolis.1927.Remastered.1080p.mkv",
    "Movies/Metropolis (1927)/Featurettes/Restoration.mkv",
    "Movies/Plan 9 from Outer Space (1959).mkv",
    "Movies/Charade (1963)/Charade (1963) {tmdb-4808}.mkv",
    "Movies/His Girl Friday (1940)/His.Girl.Friday.1940.DVDRip.XviD.avi",
    "Movies/Ironiya.sudby.ili.S.legkim.parom.1975.WEB-DL.1080p.mkv",
    "Movies/Агент 327 (2017)/Агент 327 (2017).mkv",
    "Movies/Charge (2022)/Charge (2022) - part1.mkv",
    "Movies/Charge (2022)/Charge (2022) - part2.mkv",
]
SERIES = [
    *[f"TV Shows/Pioneer One (2010)/Season 01/Pioneer One - S01E{e:02}.mkv" for e in range(1, 7)],
    *[f"TV Shows/Sherlock Holmes (1954)/Season 1/Sherlock.Holmes.1954.1x{e:02}.DVDRip.avi" for e in range(1, 4)],
    "TV Shows/Sherlock Holmes (1954)/Season 1/Sherlock.Holmes.1954.1x01.DVDRip.en.srt",
    *[f"TV Shows/The Beverly Hillbillies (1962)/S01/The Beverly Hillbillies S01E{e:02}.mp4" for e in (1, 2, 3, 5)],
    "TV Shows/The Beverly Hillbillies (1962)/Specials/The Beverly Hillbillies S00E01.mp4",
]
ANIME = [
    *[f"Anime/Test Hoshi (2024)/[TestSubs] Test Hoshi - {e:02} (1080p) [ABCD{e:04}].mkv" for e in range(1, 5)],
    "Anime/Test Hoshi (2024)/[TestSubs] Test Hoshi - NCOP (1080p).mkv",
    "Anime/Test Hoshi (2024)/[TestSubs] Test Hoshi - NCED (1080p).mkv",
    *[f"Anime/Kaijuu Test S2/[Group] Kaijuu Test S2 - {e:02} [1080p HEVC 10bit].mkv" for e in range(13, 16)],
]
# artist, album, year, format, tracks
MUSIC = [
    ("Синтетика", "Первый свет", 2023, "flac", ["Рассвет", "Полдень", "Закат", "Ночь"]),
    ("Test Pattern Orchestra", "Colour Bars", 2021, "mp3", ["Red", "Green", "Blue", "White", "Black"]),
    ("Sine Wave Collective", "Gapless Live", 2024, "flac", ["Intro", "Build", "Drop", "Outro"]),
]


def ff(*args) -> None:
    res = subprocess.run(FF + [str(a) for a in args], capture_output=True, text=True)
    if res.returncode != 0:
        raise SystemExit(f"ffmpeg failed: {' '.join(map(str, args))}\n{res.stderr}")


def place(src: Path, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        dest.unlink()
    try:
        os.link(src, dest)
    except OSError:
        shutil.copyfile(src, dest)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, default=REPO / "tests" / "media" / "library")
    ap.add_argument("--seconds", type=int, default=30, help="length of every video clip")
    ap.add_argument("--clean", action="store_true", help="delete --out first")
    args = ap.parse_args()
    if args.clean and args.out.exists():
        shutil.rmtree(args.out)

    with tempfile.TemporaryDirectory(prefix="pa-library-") as tmp:
        t = Path(tmp)
        s = args.seconds
        src = ["-f", "lavfi", "-i", f"testsrc2=size=320x180:rate=24:duration={s}",
               "-f", "lavfi", "-i", f"sine=frequency=440:sample_rate=48000:duration={s}"]
        h264 = ["-c:v", "libx264", "-preset", "veryfast", "-crf", "35", "-pix_fmt", "yuv420p", "-c:a", "aac",
                "-b:a", "64k", "-threads", "2"]
        clips = {}
        for ext, extra in [("mkv", []), ("mp4", ["-movflags", "+faststart"]), ("avi", [])]:
            clips[ext] = t / f"clip.{ext}"
            if ext == "avi":
                ff(*src, "-c:v", "mpeg4", "-vtag", "XVID", "-q:v", "8", "-c:a", "libmp3lame", "-b:a", "64k",
                   clips[ext])
            else:
                ff(*src, *h264, *extra, clips[ext])
        srt = t / "subs.srt"
        srt.write_text("1\n00:00:01,000 --> 00:00:04,000\nТестовые субтитры / test subtitles\n", encoding="utf-8")

        count = 0
        for rel in MOVIES + SERIES + ANIME:
            ext = rel.rsplit(".", 1)[1]
            place(srt if ext == "srt" else clips[ext], args.out / rel)
            count += 1

        for artist, album, year, fmt, titles in MUSIC:
            folder = args.out / "Music" / artist / f"{album} ({year})"
            folder.mkdir(parents=True, exist_ok=True)
            cover = folder / "cover.jpg"
            ff("-f", "lavfi", "-i", f"testsrc2=size=600x600:rate=1:duration=1,hue=h={year % 360}",
               "-frames:v", "1", cover)
            for n, title in enumerate(titles, 1):
                out = folder / f"{n:02} {title}.{fmt}"
                codec = ["-c:a", "flac"] if fmt == "flac" else ["-c:a", "libmp3lame", "-b:a", "128k"]
                ff("-f", "lavfi", "-i", f"sine=frequency={220 * n}:sample_rate=44100:duration=20", "-i", cover,
                   "-map", "0", "-map", "1", *codec, "-c:v", "mjpeg", "-disposition:v", "attached_pic",
                   "-metadata", f"artist={artist}", "-metadata", f"album_artist={artist}",
                   "-metadata", f"album={album}", "-metadata", f"date={year}", "-metadata", f"title={title}",
                   "-metadata", f"track={n}/{len(titles)}", "-metadata", "genre=Test", out)
                count += 1
            count += 1

    size = sum(p.stat().st_size for p in args.out.rglob("*") if p.is_file())
    print(f"{count} files in {args.out} ({size / 1e6:.1f} MB on disk if not hard-linked)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
