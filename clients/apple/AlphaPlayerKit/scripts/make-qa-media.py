#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""PLAY-APL-001: generate the QA-002 MKV samples the Apple engine must play.

Runs selected recipes of tools/qa/media/generate.py directly (its full tool
check wants every encoder of the complete set). A recipe whose encoder is
missing from this FFmpeg is reported and skipped; at least --min recipes must
succeed.

Usage: scripts/make-qa-media.py <out-dir> [--min N] [recipe ...]
"""

from __future__ import annotations

import argparse
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / "tools" / "qa" / "media"))

import generate  # noqa: E402

# MKV samples with the containers, codecs and tracks MPVKit plays in phase 1.
DEFAULT = [
    "v-h264-hi10p-mkv",
    "v-hevc-main10-mkv",
    "a-ac3-51",
    "a-eac3-51",
    "a-truehd-51",
    "a-dts-51",
    "a-flac-71",
    "s-bitmap",
    "t-multi",
    "hdr-hdr10-hevc",
]

# DejaVu Sans is only attached to MKVs; on macOS any system TrueType font
# does for a test file that is never distributed.
FALLBACK_FONTS = [
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Verdana.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
]


def find_font() -> Path:
    try:
        return ORIGINAL_FIND_FONT()
    except RuntimeError:
        for p in FALLBACK_FONTS:
            if Path(p).exists():
                return Path(p)
        raise


ORIGINAL_FIND_FONT = generate.find_font
generate.find_font = find_font


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path)
    ap.add_argument("--min", type=int, default=5, help="recipes that must succeed")
    ap.add_argument("recipes", nargs="*", default=DEFAULT)
    args = ap.parse_args()

    ok, failed = [], []
    with tempfile.TemporaryDirectory(prefix="pa-qa-media-") as tmp:
        for name in args.recipes:
            work = Path(tmp) / name
            work.mkdir()
            try:
                generate.RECIPES[name](generate.Ctx(args.out, work))
                ok.append(name)
                print(f"ok   {name}")
            except Exception as e:  # noqa: BLE001
                failed.append(name)
                print(f"SKIP {name}: {str(e).splitlines()[0] if str(e) else e!r}", file=sys.stderr)
    for path in sorted(p for p in args.out.rglob("*") if p.suffix != ".mkv" and p.is_file()):
        path.unlink()  # keep only MKV
    mkvs = sorted(args.out.rglob("*.mkv"))
    print(f"{len(ok)} recipes ok, {len(failed)} skipped; {len(mkvs)} MKV files in {args.out}")
    return 0 if len(ok) >= args.min else 1


if __name__ == "__main__":
    sys.exit(main())
