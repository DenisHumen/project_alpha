#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Downloads the external (freely licensed) samples listed in tests/media/manifest.toml.

Each file is checked against `size` and, when pinned, `sha256`. Files that are
already present and valid are skipped. `--pin` records the sha256 of files
that have none yet (maintainer step after the first download).

    python3 tools/qa/media/fetch.py [--only ID ...] [--pin]
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import manifest as mf  # noqa: E402

ROOT = HERE.parents[2] / "tests" / "media"
USER_AGENT = "project_alpha-test-media-fetch/1"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def valid(entry: dict, path: Path) -> str | None:
    """Returns an error string, or None when the file matches the manifest."""
    if not path.exists():
        return "missing"
    if entry.get("size") is not None and path.stat().st_size != entry["size"]:
        return f"size {path.stat().st_size} != {entry['size']}"
    if entry.get("sha256") and sha256(path) != entry["sha256"]:
        return "sha256 mismatch"
    return None


def download(url: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp, open(part, "wb") as out:
        while chunk := resp.read(1 << 20):
            out.write(chunk)
    part.replace(dest)


def pin(entry_id: str, digest: str) -> None:
    text = mf.MANIFEST.read_text(encoding="utf-8")
    m = re.search(rf'^id = "{re.escape(entry_id)}"\n(?:(?!^\[\[file\]\]).*\n)*?^size = \d+\n', text, re.M)
    if not m:
        raise SystemExit(f"cannot find `size` line of {entry_id} in the manifest")
    text = text[: m.end()] + f'sha256 = "{digest}"\n' + text[m.end():]
    mf.MANIFEST.write_text(text, encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--only", nargs="*", help="entry ids")
    ap.add_argument("--pin", action="store_true", help="write sha256 of unpinned files into the manifest")
    args = ap.parse_args()

    entries = [e for e in mf.load() if e["source"] != "synthetic"]
    if args.only:
        entries = [e for e in entries if e["id"] in args.only]
    total = sum(e.get("size") or 0 for e in entries)
    print(f"{len(entries)} external files, {total / 1e6:.0f} MB")
    failed = 0
    for e in entries:
        path = ROOT / e["path"]
        if valid(e, path) is not None:
            print(f"get  {e['id']}  ({e['licence']}) {e['url']}")
            try:
                download(e["url"], path)
            except OSError as err:
                print(f"FAIL {e['id']}: {err}")
                failed += 1
                continue
        err = valid(e, path)
        if err:
            print(f"FAIL {e['id']}: {err}")
            failed += 1
            continue
        if not e.get("sha256"):
            digest = sha256(path)
            if args.pin:
                pin(e["id"], digest)
                print(f"pin  {e['id']}  {digest}")
            else:
                print(f"warn {e['id']}: no sha256 in manifest (run with --pin)")
        print(f"ok   {e['id']}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
