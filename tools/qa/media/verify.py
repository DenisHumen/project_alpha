#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Checks test media against the expectations in tests/media/manifest.toml.

Every present file is probed with ffprobe and compared with its `expect`,
`streams` and `text` tables. Missing files are reported (synthetic: "run the
generator"; external: "run fetch"), but only fail with --strict.

    python3 tools/qa/media/verify.py [--strict] [--only ID ...] [--external]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import manifest as mf  # noqa: E402

ROOT = HERE.parents[2] / "tests" / "media"

STREAM_FIELDS = {
    # manifest key -> ffprobe stream key
    "type": "codec_type",
    "codec": "codec_name",
    "codec_tag": "codec_tag_string",
    "profile": "profile",
    "width": "width",
    "height": "height",
    "pix_fmt": "pix_fmt",
    "field_order": "field_order",
    "fps": "r_frame_rate",
    "sar": "sample_aspect_ratio",
    "color_transfer": "color_transfer",
    "color_primaries": "color_primaries",
    "channels": "channels",
    "layout": "channel_layout",
    "sample_rate": "sample_rate",
    "sample_fmt": "sample_fmt",
    "bits": "bits_per_raw_sample",
    "duration_ts": "duration_ts",
}
DISPOSITIONS = {"default", "forced", "comment", "hearing_impaired", "attached_pic"}


def ffprobe(path: Path) -> dict | None:
    cmd = ["ffprobe", "-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters",
           str(path)]
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        return None
    data = json.loads(res.stdout or "{}")
    return data if data.get("streams") else None


def frame_side_data(path: Path, index: int) -> set[str]:
    cmd = ["ffprobe", "-v", "error", "-select_streams", str(index), "-read_intervals", "%+#24", "-show_frames",
           "-show_entries", "frame=side_data_list", "-print_format", "json", str(path)]
    res = subprocess.run(cmd, capture_output=True, text=True)
    frames = json.loads(res.stdout or "{}").get("frames", [])
    return {sd.get("side_data_type", "") for f in frames for sd in f.get("side_data_list", [])}


def same(want, got) -> bool:
    if isinstance(want, bool) or got is None:
        return want == got
    if isinstance(want, int | float):
        try:
            return float(got) == float(want)
        except (TypeError, ValueError):
            return False
    return str(want) == str(got)


def check_stream(i: int, want: dict, s: dict, path: Path) -> list[str]:
    errs = []
    tags = {k.lower(): v for k, v in s.get("tags", {}).items()}
    side = s.get("side_data_list", [])
    for key, exp in want.items():
        if key in STREAM_FIELDS:
            got = s.get(STREAM_FIELDS[key])
        elif key == "lang":
            got = tags.get("language")
        elif key in ("title", "filename", "mimetype"):
            got = tags.get(key)
        elif key in DISPOSITIONS:
            got = bool(s.get("disposition", {}).get(key))
        elif key == "side_data":
            types = {d.get("side_data_type", "") for d in side}
            missing = [t for t in exp if t not in types]
            if missing:
                errs.append(f"stream {i}: stream side data {missing} not in {sorted(types)}")
            continue
        elif key == "frame_side_data":
            types = frame_side_data(path, s["index"])
            missing = [t for t in exp if t not in types]
            if missing:
                errs.append(f"stream {i}: frame side data {missing} not in {sorted(types)}")
            continue
        elif key in ("dv_profile", "dv_compat", "rotation"):
            src = {"dv_profile": "dv_profile", "dv_compat": "dv_bl_signal_compatibility_id", "rotation": "rotation"}
            got = next((d[src[key]] for d in side if src[key] in d), None)
        else:
            errs.append(f"stream {i}: unknown expectation `{key}`")
            continue
        if not same(exp, got):
            errs.append(f"stream {i}: {key} = {got!r}, expected {exp!r}")
    return errs


def check_text(want: dict, path: Path) -> list[str]:
    raw = path.read_bytes()
    errs = []
    bom = raw.startswith(b"\xef\xbb\xbf")
    if want.get("bom", False) != bom:
        errs.append(f"text: BOM present = {bom}, expected {want.get('bom', False)}")
    crlf = b"\r\n" in raw
    if "crlf" in want and want["crlf"] != crlf:
        errs.append(f"text: CRLF = {crlf}, expected {want['crlf']}")
    enc = want.get("encoding", "utf-8")
    try:
        text = raw.decode("utf-8-sig" if enc == "utf-8" else enc)
    except UnicodeDecodeError as e:
        return errs + [f"text: not valid {enc}: {e}"]
    if enc != "utf-8":
        try:
            raw.decode("utf-8")
            errs.append(f"text: decodes as UTF-8 too, expected legacy {enc}")
        except UnicodeDecodeError:
            pass
    if want.get("contains") and want["contains"] not in text:
        errs.append(f"text: does not contain {want['contains']!r}")
    return errs


def check(entry: dict, path: Path) -> list[str]:
    exp = entry.get("expect", {})
    if "text" in entry:
        errs = check_text(entry["text"], path)
        if not entry.get("streams"):
            return errs
    else:
        errs = []
    probe = ffprobe(path)
    if exp.get("probe", "ok") == "error":
        return errs + ([] if probe is None else ["ffprobe succeeded, expected failure"])
    if probe is None:
        return errs + ["ffprobe failed or found no streams"]
    fmt = probe.get("format", {})
    if "format" in exp and exp["format"] != fmt.get("format_name"):
        errs.append(f"format = {fmt.get('format_name')!r}, expected {exp['format']!r}")
    if "duration" in exp:
        got = float(fmt.get("duration") or 0)
        tol = max(0.15, exp["duration"] * 0.03)
        if abs(got - exp["duration"]) > tol:
            errs.append(f"duration = {got:.3f}, expected {exp['duration']} ±{tol:.2f}")
    chapters = probe.get("chapters", [])
    if "chapters" in exp and len(chapters) != exp["chapters"]:
        errs.append(f"chapters = {len(chapters)}, expected {exp['chapters']}")
    if "chapter_titles" in exp:
        titles = [c.get("tags", {}).get("title") for c in chapters]
        if titles != exp["chapter_titles"]:
            errs.append(f"chapter titles = {titles}, expected {exp['chapter_titles']}")
    ftags = {k.lower(): v for k, v in fmt.get("tags", {}).items()}
    for k, v in exp.get("tags", {}).items():
        if ftags.get(k.lower()) != v:
            errs.append(f"tag {k} = {ftags.get(k.lower())!r}, expected {v!r}")
    streams = probe["streams"]
    want = entry.get("streams", [])
    if want and len(streams) != len(want):
        errs.append(f"{len(streams)} streams, expected {len(want)}")
    for i, (w, s) in enumerate(zip(want, streams)):
        errs += check_stream(i, w, s, path)
    return errs


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", type=Path, default=ROOT)
    ap.add_argument("--only", nargs="*", help="entry ids")
    ap.add_argument("--external", action="store_true", help="also check external (downloaded) files")
    ap.add_argument("--strict", action="store_true", help="missing files are errors")
    args = ap.parse_args()

    entries = mf.load()
    if args.only:
        entries = [e for e in entries if e["id"] in args.only]
    if not args.external:
        entries = [e for e in entries if e["source"] == "synthetic"]
    failed = missing = ok = 0
    total_size = 0
    for e in entries:
        path = args.root / e["path"]
        if not path.exists():
            missing += 1
            print(f"MISS {e['id']}: {e['path']}")
            continue
        total_size += path.stat().st_size
        errs = check(e, path)
        if errs:
            failed += 1
            print(f"FAIL {e['id']}: {e['path']}")
            for err in errs:
                print(f"     - {err}")
        else:
            ok += 1
    print(f"{ok} ok, {failed} failed, {missing} missing; {total_size / 1e6:.1f} MB checked")
    if total_size > 2e9:
        print("FAIL: test-media set exceeds the 2 GB budget (QA-002)")
        return 1
    return 1 if failed or (args.strict and missing) else 0


if __name__ == "__main__":
    sys.exit(main())
