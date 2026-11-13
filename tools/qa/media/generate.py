#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Generates the synthetic test-media set described in tests/media/manifest.toml.

Every synthetic file is produced from FFmpeg test sources (testsrc2, sine,
aevalsrc) plus small writers in this directory (PGS, subtitles, HDR metadata),
so the whole set is reproducible and CC0. Binaries are NOT committed: run

    just tests::media            # or: python3 tools/qa/media/generate.py

and then `just tests::media-verify`. See tests/media/README.md.

Needs: FFmpeg >= 7.1 with libx264, libx265, libsvtav1, libvpx, libopus,
libvorbis, libmp3lame, libtheora (GPL encoders run only on the developer's
machine; nothing generated here is shipped) and cargo (for `hdrmeta`, only for
the HDR10+ and Dolby Vision samples).
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import manifest as mf  # noqa: E402
import pgs  # noqa: E402
import texts  # noqa: E402

REPO = HERE.parents[2]
HDRMETA_DIR = HERE / "hdrmeta"
HDRMETA_BIN = HDRMETA_DIR / "target" / "release" / "pa-hdrmeta"

FF = ["nice", "-n", "10", "ffmpeg", "-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
BITEXACT = ["-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact", "-map_metadata", "-1"]
THREADS = ["-threads", "2"]

MDCV = "G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)"
X265_HDR10 = f"hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display={MDCV}:max-cll=1000,400"
X265_HLG = "repeat-headers=1:colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc"
PQ_TAGS = ["-color_primaries", "bt2020", "-color_trc", "smpte2084", "-colorspace", "bt2020nc"]
HLG_TAGS = ["-color_primaries", "bt2020", "-color_trc", "arib-std-b67", "-colorspace", "bt2020nc"]

# Channel i of a multichannel tone gets 300 + 100*i Hz, so mapping errors are audible
# and measurable (FL=300, FR=400, FC=500, LFE=600, ...).
TONE_EXPR = "|".join(f"0.25*sin(2*PI*{300 + 100 * i}*t)" for i in range(8))


def vsrc(size="640x360", rate="24", dur=3.0, pattern="testsrc2"):
    return ["-f", "lavfi", "-i", f"{pattern}=size={size}:rate={rate}:duration={dur}"]


def asrc(layout="stereo", dur=3.0, rate=48000):
    n = {"mono": 1, "stereo": 2, "5.1": 6, "5.1(side)": 6, "7.1": 8}[layout]
    expr = "|".join(TONE_EXPR.split("|")[:n])
    return ["-f", "lavfi", "-i", f"aevalsrc=exprs={expr}:s={rate}:d={dur}:c={layout}"]


def run(cmd: list[str], cwd: Path | None = None) -> None:
    res = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if res.returncode != 0:
        raise RuntimeError(f"command failed ({res.returncode}): {' '.join(map(str, cmd))}\n{res.stderr}")


def ff(*args) -> None:
    run(FF + [str(a) for a in args])


# --------------------------------------------------------------------------
# Recipes: name -> function(ctx). A recipe may produce several manifest files;
# it writes them under ctx.root using the manifest paths.
# --------------------------------------------------------------------------

RECIPES = {}


def recipe(name):
    def deco(fn):
        RECIPES[name] = fn
        return fn

    return deco


class Ctx:
    def __init__(self, root: Path, tmp: Path):
        self.root = root
        self.tmp = tmp

    def out(self, rel: str) -> Path:
        p = self.root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        return p


H264 = ["-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-g", "48", "-pix_fmt", "yuv420p"]
AAC = ["-c:a", "aac", "-b:a", "96k"]


def simple(rel, v, a, codec_args, extra=()):
    def fn(ctx: Ctx):
        ff(*v, *a, *THREADS, *codec_args, *BITEXACT, *extra, ctx.out(rel))

    return fn


# ---- containers and codecs (640x360, 3 s) --------------------------------
V = vsrc()
A2 = asrc()
recipe("v-h264-aac-mp4")(simple("synthetic/video/h264-aac.mp4", V, A2, H264 + AAC, ["-movflags", "+faststart"]))
recipe("v-h264-aac-moov-end")(simple("synthetic/video/h264-aac-moov-at-end.mp4", V, A2, H264 + AAC))
recipe("v-h264-hi10p-mkv")(
    simple(
        "synthetic/video/h264-hi10p-flac.mkv",
        V,
        A2,
        ["-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-pix_fmt", "yuv420p10le", "-c:a", "flac"],
    )
)
recipe("v-hevc-main10-mkv")(
    simple(
        "synthetic/video/hevc-main10-opus.mkv",
        V,
        A2,
        ["-c:v", "libx265", "-preset", "fast", "-crf", "30", "-pix_fmt", "yuv420p10le",
         "-x265-params", "log-level=error:pools=2", "-c:a", "libopus", "-b:a", "96k"],
    )
)
recipe("v-hevc-mp4")(
    simple(
        "synthetic/video/hevc-hvc1-aac.mp4",
        V,
        A2,
        ["-c:v", "libx265", "-preset", "fast", "-crf", "30", "-pix_fmt", "yuv420p",
         "-x265-params", "log-level=error:pools=2", "-tag:v", "hvc1", *AAC],
        ["-movflags", "+faststart"],
    )
)
SVT = ["-c:v", "libsvtav1", "-preset", "10", "-crf", "40", "-svtav1-params", "lp=2"]
recipe("v-av1-mkv")(simple("synthetic/video/av1-opus.mkv", V, A2, SVT + ["-pix_fmt", "yuv420p", "-c:a", "libopus"]))
recipe("v-av1-mp4")(simple("synthetic/video/av1-aac.mp4", V, A2, SVT + ["-pix_fmt", "yuv420p", *AAC], ["-movflags", "+faststart"]))
recipe("v-vp9-webm")(
    simple("synthetic/video/vp9-opus.webm", V, A2,
           ["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "40", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libopus"])
)
recipe("v-vp8-webm")(
    simple("synthetic/video/vp8-vorbis.webm", V, A2,
           ["-c:v", "libvpx", "-b:v", "500k", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libvorbis"])
)
recipe("v-theora-ogv")(
    simple("synthetic/video/theora-vorbis.ogv", V, A2, ["-c:v", "libtheora", "-q:v", "5", "-c:a", "libvorbis"])
)
recipe("v-mpeg2-ts")(
    simple(
        "synthetic/video/mpeg2-interlaced-ac3.ts",
        vsrc("720x576", "25"),
        A2,
        ["-c:v", "mpeg2video", "-b:v", "3M", "-top", "1", "-field_order", "tt", "-vf", "setfield=tff",
         "-aspect", "16:9", "-c:a", "ac3", "-b:a", "192k"],
        ["-flags:v", "+bitexact+ilme+ildct"],  # after BITEXACT, which would override -flags
    )
)
recipe("v-mpeg2-vob")(
    simple(
        "synthetic/video/mpeg2-ac3.vob",
        vsrc("720x480", "30000/1001"),
        A2,
        ["-c:v", "mpeg2video", "-b:v", "3M", "-aspect", "4:3", "-c:a", "ac3", "-b:a", "192k", "-f", "vob"],
    )
)
recipe("v-h264-m2ts")(
    simple(
        "synthetic/video/h264-ac3.m2ts",
        vsrc("1280x720", "24000/1001"),
        asrc("5.1"),
        H264 + ["-c:a", "ac3", "-b:a", "384k", "-mpegts_m2ts_mode", "1"],
    )
)
recipe("v-mpeg4asp-avi")(
    simple("synthetic/video/mpeg4-asp-mp3.avi", V, A2,
           ["-c:v", "mpeg4", "-vtag", "XVID", "-q:v", "5", "-c:a", "libmp3lame", "-b:a", "128k"])
)
recipe("v-wmv")(simple("synthetic/video/wmv2-wmav2.wmv", V, A2, ["-c:v", "wmv2", "-b:v", "800k", "-c:a", "wmav2"]))
recipe("v-prores-mov")(
    simple("synthetic/video/prores-proxy-pcm.mov", vsrc("640x360", "25"), A2,
           ["-c:v", "prores_ks", "-profile:v", "0", "-c:a", "pcm_s24le"])
)
recipe("v-flv")(simple("synthetic/video/h264-aac.flv", V, A2, H264 + AAC))

# ---- AFR: H.264 1280x720 at every common rate ------------------------------
AFR_RATES = {"23.976": "24000/1001", "24": "24", "25": "25", "29.97": "30000/1001", "50": "50", "59.94": "60000/1001"}
for _name, _rate in AFR_RATES.items():
    recipe(f"afr-{_name}")(
        simple(f"synthetic/afr/h264-720p-{_name}.mp4", vsrc("1280x720", _rate), A2, H264 + AAC,
               ["-movflags", "+faststart"])
    )

# ---- HDR -------------------------------------------------------------------
HDR_SIZE, HDR_RATE, HDR_DUR = "1920x1080", "24000/1001", 2.0


def x265_raw(ctx: Ctx, params: str, name: str, size=HDR_SIZE, dur=HDR_DUR) -> Path:
    raw = ctx.tmp / f"{name}.hevc"
    ff(*vsrc(size, HDR_RATE, dur), *THREADS, "-pix_fmt", "yuv420p10le", "-c:v", "libx265", "-preset", "fast",
       "-crf", "28", "-x265-params", f"log-level=error:pools=2:{params}", "-f", "hevc", raw)
    return raw


def hevc_to(ctx: Ctx, raw: Path, rels: list[str], tags: list[str], dovi=False, mp4_tag="hvc1"):
    """Raw HEVC -> MP4 (timestamps) -> requested containers, optionally with DV config."""
    mid = ctx.tmp / (raw.stem + ".mp4")
    ff("-fflags", "+genpts", "-r", HDR_RATE, "-i", raw, "-c", "copy", "-strict", "unofficial", mid)
    for rel in rels:
        out = ctx.out(rel)
        args = ["-i", mid, "-c", "copy", *tags, *BITEXACT]
        if dovi:
            args += ["-bsf:v", "dovi_rpu=compression=none", "-strict", "unofficial"]  # writes dvcC / BlockAddIDType from RPUs
        if rel.endswith(".mp4"):
            args += ["-tag:v", mp4_tag, "-movflags", "+faststart"]
        ff(*args, out)


def hdrmeta(*args) -> None:
    if not HDRMETA_BIN.exists():
        run(["nice", "cargo", "build", "--release", "--quiet", "-j", "4"], cwd=HDRMETA_DIR)
    run([str(HDRMETA_BIN), *map(str, args)])


@recipe("hdr-hdr10-hevc")
def _(ctx):
    raw = x265_raw(ctx, X265_HDR10, "hdr10")
    hevc_to(ctx, raw, ["synthetic/hdr/hdr10-hevc-1080p.mkv", "synthetic/hdr/hdr10-hevc-1080p.mp4"], PQ_TAGS)


@recipe("hdr-hdr10-hevc-2160p")
def _(ctx):
    raw = x265_raw(ctx, X265_HDR10, "hdr10-2160p", size="3840x2160")
    hevc_to(ctx, raw, ["synthetic/hdr/hdr10-hevc-2160p.mkv"], PQ_TAGS)


@recipe("hdr-hlg-hevc")
def _(ctx):
    raw = x265_raw(ctx, X265_HLG, "hlg")
    hevc_to(ctx, raw, ["synthetic/hdr/hlg-hevc-1080p.mkv"], HLG_TAGS)


@recipe("hdr-hdr10plus")
def _(ctx):
    raw = x265_raw(ctx, X265_HDR10, "hdr10p-base")
    inj = ctx.tmp / "hdr10plus.hevc"
    hdrmeta("hdr10plus", raw, inj)
    hevc_to(ctx, inj, ["synthetic/hdr/hdr10plus-hevc-1080p.mkv", "synthetic/hdr/hdr10plus-hevc-1080p.mp4"], PQ_TAGS)


@recipe("hdr-dv-p5")
def _(ctx):
    # Profile 5: like real P5 streams, the VUI colour description is left
    # unspecified (the base layer is IPTPQc2, not BT.2020) and FFmpeg's
    # dovi_rpu BSF therefore signals profile 5 / compatibility 0. The picture
    # is the plain test pattern, so players without RPU reshaping show it
    # wrong — that is what the sample checks.
    raw = x265_raw(ctx, "repeat-headers=1", "dv5-base")
    inj = ctx.tmp / "dv5.hevc"
    hdrmeta("dovi", "5", raw, inj)
    hevc_to(ctx, inj, ["synthetic/hdr/dv-p5-hevc-1080p.mp4", "synthetic/hdr/dv-p5-hevc-1080p.mkv"], [],
            dovi=True, mp4_tag="dvh1")


@recipe("hdr-dv-p81")
def _(ctx):
    raw = x265_raw(ctx, X265_HDR10, "dv81-base")
    inj = ctx.tmp / "dv81.hevc"
    hdrmeta("dovi", "8.1", raw, inj)
    hevc_to(ctx, inj, ["synthetic/hdr/dv-p8.1-hevc-1080p.mkv", "synthetic/hdr/dv-p8.1-hevc-1080p.mp4"], PQ_TAGS,
            dovi=True)


@recipe("hdr-dv-p84")
def _(ctx):
    raw = x265_raw(ctx, X265_HLG, "dv84-base")
    inj = ctx.tmp / "dv84.hevc"
    hdrmeta("dovi", "8.4", raw, inj)
    hevc_to(ctx, inj, ["synthetic/hdr/dv-p8.4-hlg-hevc-1080p.mp4"], HLG_TAGS, dovi=True)


@recipe("hdr-hdr10-av1")
def _(ctx):
    ff(*vsrc(HDR_SIZE, HDR_RATE, HDR_DUR), *THREADS, "-pix_fmt", "yuv420p10le", "-c:v", "libsvtav1", "-preset", "10",
       "-crf", "40", "-svtav1-params",
       "lp=2:color-primaries=bt2020:transfer-characteristics=smpte2084:matrix-coefficients=bt2020-ncl:"
       "mastering-display=G(0.265,0.69)B(0.15,0.06)R(0.68,0.32)WP(0.3127,0.329)L(1000,0.0001):"
       "content-light=1000,400",
       *PQ_TAGS, *BITEXACT, ctx.out("synthetic/hdr/hdr10-av1-1080p.mkv"))


@recipe("hdr-hdr10-vp9")
def _(ctx):
    ff(*vsrc(HDR_SIZE, HDR_RATE, HDR_DUR), *THREADS, "-pix_fmt", "yuv420p10le", "-c:v", "libvpx-vp9", "-profile:v", "2",
       "-b:v", "0", "-crf", "45", "-deadline", "realtime", "-cpu-used", "8", "-vf",
       "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc", *BITEXACT,
       ctx.out("synthetic/hdr/hdr10-vp9-1080p.webm"))


# ---- audio formats (tiny video + one audio track) --------------------------
VT = vsrc("320x180", "24", 3.0)
for _id, _rel, _layout, _args in [
    ("a-ac3-51", "synthetic/audio/ac3-5.1.mkv", "5.1", ["-c:a", "ac3", "-b:a", "448k"]),
    ("a-eac3-51", "synthetic/audio/eac3-5.1.mkv", "5.1", ["-c:a", "eac3", "-b:a", "640k"]),
    ("a-truehd-51", "synthetic/audio/truehd-5.1.mkv", "5.1(side)", ["-c:a", "truehd", "-strict", "experimental"]),
    ("a-dts-51", "synthetic/audio/dts-5.1.mkv", "5.1(side)", ["-c:a", "dca", "-strict", "experimental"]),
    ("a-flac-71", "synthetic/audio/flac-7.1-24bit.mkv", "7.1", ["-c:a", "flac", "-sample_fmt", "s32"]),
    ("a-pcm-71", "synthetic/audio/pcm-s24le-7.1.mkv", "7.1", ["-c:a", "pcm_s24le"]),
    ("a-opus-51", "synthetic/audio/opus-5.1.mkv", "5.1", ["-c:a", "libopus", "-b:a", "256k"]),
    ("a-aac-51", "synthetic/audio/aac-5.1.mp4", "5.1", ["-c:a", "aac", "-b:a", "384k"]),
    ("a-lpcm-m2ts", "synthetic/audio/lpcm-5.1.m2ts", "5.1", ["-c:a", "pcm_bluray", "-mpegts_m2ts_mode", "1"]),
]:
    recipe(_id)(simple(_rel, VT, asrc(_layout), H264 + _args))


# ---- subtitles ---------------------------------------------------------------
SUB_EVENTS = [(0.5, 1.5), (2.0, 3.0), (3.5, 4.5)]
VS = vsrc("1280x720", "24", 5.0)


def sup(ctx: Ctx, size=(1280, 720)) -> Path:
    p = ctx.tmp / f"pgs-{size[0]}.sup"
    if not p.exists():
        pgs.write_sup(p, SUB_EVENTS, size)
    return p


@recipe("s-bitmap")
def _(ctx):
    s = sup(ctx)
    common = [*VS, "-fix_sub_duration", "-i", s, "-map", "0:v", "-map", "1:s", *THREADS, *H264, "-metadata:s:s:0", "language=eng"]
    ff(*common, "-c:s", "copy", *BITEXACT, ctx.out("synthetic/subs/pgs.mkv"))
    ff(*common, "-c:s", "dvdsub", *BITEXACT, ctx.out("synthetic/subs/vobsub.mkv"))
    ff(*common, "-c:s", "dvbsub", *BITEXACT, ctx.out("synthetic/subs/dvbsub.ts"))


@recipe("s-text")
def _(ctx):
    srt = ctx.tmp / "en.srt"
    srt.write_text(texts.srt("en"), encoding="utf-8")
    vtt = ctx.tmp / "en.vtt"
    vtt.write_text(texts.vtt(), encoding="utf-8")
    ff(*VS, "-i", srt, "-map", "0:v", "-map", "1", *THREADS, *H264, "-c:s", "mov_text",
       "-metadata:s:s:0", "language=eng", *BITEXACT, ctx.out("synthetic/subs/mov_text.mp4"))
    ff(*VS, "-i", vtt, "-map", "0:v", "-map", "1", *THREADS, "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "45",
       "-deadline", "realtime", "-cpu-used", "8", "-c:s", "webvtt", "-metadata:s:s:0", "language=eng", *BITEXACT,
       ctx.out("synthetic/subs/webvtt.webm"))


def find_font() -> Path:
    for p in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    ]:
        if Path(p).exists():
            return Path(p)
    raise RuntimeError("DejaVuSans.ttf not found (install fonts-dejavu-core)")


@recipe("s-ass-fonts")
def _(ctx):
    ass = ctx.tmp / "styled.ass"
    ass.write_text(texts.ass(), encoding="utf-8")
    ff(*VS, "-i", ass, "-attach", find_font(), "-metadata:s:t", "mimetype=font/ttf",
       "-map", "0:v", "-map", "1", *THREADS, *H264, "-c:s", "copy", "-metadata:s:s:0", "language=rus",
       *BITEXACT, ctx.out("synthetic/subs/ass-attached-font.mkv"))


@recipe("t-multi")
def _(ctx):
    """1 video, 3 audio (lang/default/commentary), 4 subs (default/forced/SDH), chapters, font."""
    srt_en, srt_ru, srt_forced = ctx.tmp / "m-en.srt", ctx.tmp / "m-ru.srt", ctx.tmp / "m-forced.srt"
    srt_en.write_text(texts.srt("en"), encoding="utf-8")
    srt_ru.write_text(texts.srt("ru"), encoding="utf-8")
    srt_forced.write_text(texts.srt("forced"), encoding="utf-8")
    ass = ctx.tmp / "m.ass"
    ass.write_text(texts.ass(), encoding="utf-8")
    meta = ctx.tmp / "chapters.txt"
    meta.write_text(texts.ffmetadata_chapters(5.0, ["Пролог", "Middle", "Финал"]), encoding="utf-8")
    ff(*VS, *asrc("stereo", 5.0), *asrc("5.1", 5.0), *asrc("stereo", 5.0),
       "-i", srt_en, "-i", srt_ru, "-i", srt_forced, "-i", ass, "-i", meta,
       "-attach", find_font(), "-metadata:s:t", "mimetype=font/ttf",
       "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:a",
       "-map", "4", "-map", "5", "-map", "6", "-map", "7", "-map_chapters", "8",
       *THREADS, *H264,
       "-c:a:0", "aac", "-c:a:1", "ac3", "-c:a:2", "libopus", "-c:s:0", "srt", "-c:s:1", "srt", "-c:s:2", "srt",
       "-c:s:3", "copy",
       "-metadata:s:a:0", "language=eng", "-metadata:s:a:0", "title=English Stereo",
       "-metadata:s:a:1", "language=rus", "-metadata:s:a:1", "title=Русский 5.1",
       "-metadata:s:a:2", "language=jpn", "-metadata:s:a:2", "title=Commentary",
       "-metadata:s:s:0", "language=eng", "-metadata:s:s:0", "title=English SDH",
       "-metadata:s:s:1", "language=rus",
       "-metadata:s:s:2", "language=rus", "-metadata:s:s:2", "title=Forced",
       "-metadata:s:s:3", "language=rus", "-metadata:s:s:3", "title=Надписи (ASS)",
       "-disposition:a:0", "default", "-disposition:a:1", "0", "-disposition:a:2", "comment",
       "-disposition:s:0", "hearing_impaired", "-disposition:s:1", "default", "-disposition:s:2", "forced",
       "-disposition:s:3", "0",
       "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
       ctx.out("synthetic/tracks/multi-audio-subs-chapters.mkv"))


@recipe("t-chapters-mp4")
def _(ctx):
    meta = ctx.tmp / "chapters-mp4.txt"
    meta.write_text(texts.ffmetadata_chapters(6.0, ["Opening", "Part A", "Ending"]), encoding="utf-8")
    ff(*vsrc("640x360", "24", 6.0), *asrc("stereo", 6.0), "-i", meta, "-map", "0", "-map", "1",
       "-map_chapters", "2", *THREADS, *H264, *AAC, "-movflags", "+faststart",
       "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
       ctx.out("synthetic/tracks/chapters.mp4"))


@recipe("t-many-tracks")
def _(ctx):
    srt = ctx.tmp / "many.srt"
    srt.write_text(texts.srt("en"), encoding="utf-8")
    langs = ["eng", "rus", "jpn", "fre", "ger", "spa", "ita", "por", "pol", "ukr", "chi", "kor", "ara", "tur", "dut", "swe"]
    args = [*vsrc("320x180", "24", 5.0), *asrc("stereo", 5.0)]
    for _ in langs:
        args += ["-i", srt]
    args += ["-map", "0:v", *sum([["-map", "1:a"] for _ in range(4)], [])]
    args += sum([["-map", f"{i + 2}"] for i in range(len(langs))], [])
    args += [*THREADS, *H264, "-c:a", "aac", "-c:s", "srt"]
    for i, lang in enumerate(langs[:4]):
        args += [f"-metadata:s:a:{i}", f"language={lang}"]
    for i, lang in enumerate(langs):
        args += [f"-metadata:s:s:{i}", f"language={lang}"]
    ff(*args, *BITEXACT, ctx.out("synthetic/tracks/many-tracks.mkv"))


@recipe("sidecar")
def _(ctx):
    """A movie folder with external subtitles (LIB-007 sidecar rules)."""
    base = "synthetic/sidecar/Test Movie (2024)/Test Movie (2024)"
    ff(*vsrc("640x360", "24", 5.0), *asrc("stereo", 5.0), *THREADS, *H264, *AAC, *BITEXACT, ctx.out(base + ".mkv"))
    ctx.out(base + ".ru.forced.srt").write_text(texts.srt("forced"), encoding="utf-8")
    # Legacy Russian file in Windows-1251, no language-neutral hint except the suffix.
    ctx.out(base + ".rus.srt").write_bytes(texts.srt("ru").encode("cp1251"))
    # UTF-8 with BOM and CRLF line endings (typical Windows-made SDH file).
    ctx.out(base + ".en.sdh.srt").write_bytes(b"\xef\xbb\xbf" + texts.srt("en").replace("\n", "\r\n").encode("utf-8"))
    ctx.out(base + ".en.ass").write_text(texts.ass(), encoding="utf-8")
    ctx.out(base + ".de.vtt").write_text(texts.vtt(), encoding="utf-8")
    shutil.copy(sup(ctx, (640, 360)), ctx.out(base + ".en.sup"))


# ---- edge cases ------------------------------------------------------------
@recipe("e-basic")
def _(ctx):
    ff(*vsrc("640x360", "24", 3.0), *THREADS, *H264, "-an", "-movflags", "+faststart", *BITEXACT,
       ctx.out("synthetic/edge/video-only.mp4"))
    ff(*asrc("stereo", 3.0), "-c:a", "libopus", *BITEXACT, ctx.out("synthetic/edge/audio-only.mka"))
    # VFR: frame timestamps jitter between 1/24 and 1/12 s.
    ff(*vsrc("640x360", "24", 3.0), *THREADS, "-vf", "setpts='N/24/TB+if(mod(N,5),0,1/24/TB)'", "-fps_mode", "vfr",
       *H264, *BITEXACT, ctx.out("synthetic/edge/vfr.mkv"))
    plain = ctx.tmp / "plain.mp4"
    ff(*vsrc("640x360", "24", 3.0), *THREADS, *H264, *BITEXACT, plain)
    # Stream copy keeps the display matrix; re-encoding would auto-rotate the frames.
    ff("-display_rotation", "90", "-i", plain, "-c", "copy", ctx.out("synthetic/edge/rotated-90.mp4"))
    ff(*vsrc("720x576", "25", 3.0), *THREADS, "-vf", "setsar=64/45", *H264, *BITEXACT,
       ctx.out("synthetic/edge/anamorphic-pal-16x9.mkv"))
    # PTS close to the 33-bit MPEG-TS wrap (2^33 / 90 kHz = 95443.7 s).
    ff(*vsrc("640x360", "25", 4.0), *asrc("stereo", 4.0), *THREADS, *H264, "-c:a", "mp2",
       "-output_ts_offset", "95441", *BITEXACT, ctx.out("synthetic/edge/pts-wrap.ts"))
    ff(*vsrc("7680x4320", "24", 1.0), *THREADS, "-c:v", "libx265", "-preset", "ultrafast", "-crf", "35",
       "-x265-params", "log-level=error:pools=2", "-pix_fmt", "yuv420p", *BITEXACT,
       ctx.out("synthetic/edge/hevc-4320p.mkv"))


@recipe("e-broken")
def _(ctx):
    good = ctx.tmp / "good.mkv"
    ff(*vsrc("640x360", "24", 4.0), *asrc("stereo", 4.0), *THREADS, *H264, *AAC, *BITEXACT, good)
    data = good.read_bytes()
    ctx.out("synthetic/edge/truncated.mkv").write_bytes(data[: len(data) * 6 // 10])
    corrupt = bytearray(data)
    mid = len(corrupt) // 2
    corrupt[mid : mid + 4096] = bytes((i * 151 + 7) & 0xFF for i in range(4096))  # deterministic garbage
    ctx.out("synthetic/edge/corrupt-middle.mkv").write_bytes(bytes(corrupt))
    ctx.out("synthetic/edge/wrong-extension.avi").write_bytes(data)  # really Matroska
    ctx.out("synthetic/edge/имя с пробелами «юникод» 日本語.mkv").write_bytes(data)
    ctx.out("synthetic/edge/zero-bytes.mkv").write_bytes(b"")
    for rel, faststart in [("truncated-faststart.mp4", True), ("truncated-moov-at-end.mp4", False)]:
        mp4 = ctx.tmp / rel
        ff(*vsrc("640x360", "24", 4.0), *asrc("stereo", 4.0), *THREADS, *H264, *AAC, *BITEXACT,
           *(["-movflags", "+faststart"] if faststart else []), mp4)
        d = mp4.read_bytes()
        ctx.out("synthetic/edge/" + rel).write_bytes(d[: len(d) * 6 // 10])


# ---- music -----------------------------------------------------------------
TAGS = ["-metadata", "artist=Синтетика", "-metadata", "album_artist=Синтетика",
        "-metadata", "album=Gapless Test", "-metadata", "date=2024", "-metadata", "genre=Test"]
# Track lengths in samples at 44.1 kHz: deliberately not multiples of any codec frame size.
GAPLESS = [("01 Rise", 132301), ("02 Hold", 88207), ("03 Fall", 110251)]


@recipe("m-gapless")
def _(ctx):
    total = sum(n for _, n in GAPLESS)
    src = ctx.tmp / "sweep.wav"
    # One continuous sweep: any gap or overlap at a track boundary is audible as a click.
    ff("-f", "lavfi", "-i", f"aevalsrc=exprs=0.4*sin(2*PI*(200+400*t/{total / 44100})*t):s=44100:c=stereo",
       "-af", f"atrim=end_sample={total}", "-c:a", "pcm_s16le", src)
    start = 0
    for i, (title, n) in enumerate(GAPLESS, 1):
        trim = ["-af", f"atrim=start_sample={start}:end_sample={start + n},asetpts=N/SR/TB"]
        meta = [*TAGS, "-metadata", f"title={title[3:]}", "-metadata", f"track={i}/{len(GAPLESS)}"]
        for ext, codec in [("flac", ["-c:a", "flac"]), ("mp3", ["-c:a", "libmp3lame", "-b:a", "192k"]),
                           ("m4a", ["-c:a", "aac", "-b:a", "160k"]), ("opus", ["-c:a", "libopus", "-b:a", "128k"])]:
            ff("-i", src, *trim, *codec, *meta, "-fflags", "+bitexact", "-flags:a", "+bitexact",
               ctx.out(f"synthetic/music/gapless/{ext}/{title}.{ext}"))
        start += n


@recipe("m-formats")
def _(ctx):
    meta = [*TAGS, "-metadata", "album=Formats", "-metadata", "title=Tone"]
    bx = ["-fflags", "+bitexact", "-flags:a", "+bitexact"]
    ff(*asrc("stereo", 3.0, 96000), "-c:a", "flac", "-sample_fmt", "s32", *meta, *bx,
       ctx.out("synthetic/music/formats/hires-24bit-96k.flac"))
    ff(*asrc("stereo", 3.0, 44100), "-c:a", "alac", *meta, *bx, ctx.out("synthetic/music/formats/alac-16-44.m4a"))
    cover = ctx.tmp / "cover.png"
    ff("-f", "lavfi", "-i", "testsrc2=size=600x600:rate=1", "-frames:v", "1", cover)
    ff(*asrc("stereo", 3.0, 44100), "-i", cover, "-map", "0", "-map", "1", "-c:a", "libmp3lame", "-b:a", "192k",
       "-c:v", "png", "-disposition:v", "attached_pic", "-id3v2_version", "3", *meta, *bx,
       ctx.out("synthetic/music/formats/id3v2-cover.mp3"))
    # Single-file album + CUE sheet.
    ff(*asrc("stereo", 9.0, 44100), "-c:a", "flac", *TAGS, "-metadata", "album=CUE Album", *bx,
       ctx.out("synthetic/music/cue/CUE Album.flac"))
    ctx.out("synthetic/music/cue/CUE Album.cue").write_text(texts.cue("CUE Album.flac"), encoding="utf-8")
    # Track with synced lyrics sidecar.
    ff(*asrc("stereo", 6.0, 44100), "-c:a", "flac", *TAGS, "-metadata", "album=Lyrics", "-metadata", "title=Song",
       *bx, ctx.out("synthetic/music/lyrics/Song.flac"))
    ctx.out("synthetic/music/lyrics/Song.lrc").write_text(texts.lrc(), encoding="utf-8")


# --------------------------------------------------------------------------


def check_tools() -> list[str]:
    problems = []
    if not shutil.which("ffmpeg"):
        return ["ffmpeg not found"]
    enc = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    for e in ["libx264", "libx265", "libsvtav1", "libvpx-vp9", "libvpx ", "libopus", "libvorbis", "libmp3lame",
              "libtheora", "truehd", "dca", "eac3", "wmv2", "wmav2", "prores_ks", "dvdsub", "dvbsub", "pcm_bluray"]:
        if f" {e}" not in enc and f" {e} " not in enc:
            problems.append(f"ffmpeg encoder missing: {e.strip()}")
    bsf = subprocess.run(["ffmpeg", "-hide_banner", "-bsfs"], capture_output=True, text=True).stdout
    if "dovi_rpu" not in bsf:
        problems.append("ffmpeg bitstream filter dovi_rpu missing (need FFmpeg >= 7.1)")
    if not HDRMETA_BIN.exists() and not shutil.which("cargo"):
        problems.append("cargo not found (needed once to build tools/qa/media/hdrmeta)")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, default=REPO / "tests" / "media", help="media root (default tests/media)")
    ap.add_argument("--only", nargs="*", help="recipe names to run (default: all)")
    ap.add_argument("-j", "--jobs", type=int, default=2, help="parallel recipes (max 4)")
    ap.add_argument("--list", action="store_true", help="list recipes and exit")
    args = ap.parse_args()

    entries = mf.load()
    needed = sorted({e["recipe"] for e in entries if e["source"] == "synthetic"})
    missing = [r for r in needed if r not in RECIPES]
    unused = [r for r in RECIPES if r not in needed]
    if missing or unused:
        print(f"manifest/recipe mismatch: missing recipes {missing}, recipes not in manifest {unused}", file=sys.stderr)
        return 2
    if args.list:
        print("\n".join(needed))
        return 0
    problems = check_tools()
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 2

    todo = args.only or needed
    jobs = max(1, min(args.jobs, 4))
    failed = []
    with tempfile.TemporaryDirectory(prefix="pa-media-") as tmp:
        def one(name):
            t = Path(tmp) / name
            t.mkdir()
            RECIPES[name](Ctx(args.out, t))
            return name

        with cf.ThreadPoolExecutor(jobs) as pool:
            futs = {pool.submit(one, n): n for n in todo}
            for fut in cf.as_completed(futs):
                name = futs[fut]
                try:
                    fut.result()
                    print(f"ok   {name}")
                except Exception as e:  # noqa: BLE001
                    failed.append(name)
                    print(f"FAIL {name}: {e}", file=sys.stderr)
    produced = [e for e in entries if e["source"] == "synthetic" and (args.out / e["path"]).exists()]
    size = sum((args.out / e["path"]).stat().st_size for e in produced)
    print(f"{len(produced)} synthetic files, {size / 1e6:.1f} MB in {os.path.relpath(args.out / 'synthetic')}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
