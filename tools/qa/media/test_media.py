# SPDX-License-Identifier: MPL-2.0
"""Unit tests for the test-media tooling (no FFmpeg needed).

    python3 -m unittest discover -s tools/qa/media -p 'test_*.py'
"""

from __future__ import annotations

import shutil
import struct
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import fetch  # noqa: E402
import generate  # noqa: E402
import manifest as mf  # noqa: E402
import pgs  # noqa: E402
import texts  # noqa: E402
import verify  # noqa: E402


def decode_rle(data: bytes, width: int) -> list[list[int]]:
    """Reference PGS RLE decoder (mirrors FFmpeg pgssubdec)."""
    rows, row, i = [], [], 0
    while i < len(data):
        b = data[i]
        i += 1
        if b:
            row.append(b)
            continue
        flags = data[i]
        i += 1
        if flags == 0:
            rows.append(row)
            row = []
            continue
        n = flags & 0x3F
        if flags & 0x40:
            n = (n << 8) | data[i]
            i += 1
        color = 0
        if flags & 0x80:
            color = data[i]
            i += 1
        row.extend([color] * n)
    assert all(len(r) == width for r in rows), "row width mismatch"
    return rows


class PgsTest(unittest.TestCase):
    def test_rle_roundtrip(self):
        rows = [[0] * 70 + [1, 1] + [2] * 5 + [3] * 300 + [0, 1], [3] * 377, [1, 0, 1] * 125 + [2, 2]]
        for row in rows:
            self.assertEqual(decode_rle(pgs.rle_line(row), len(row)), [row])

    def test_sup_structure(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "t.sup"
            pgs.write_sup(p, [(0.5, 1.5), (2.0, 3.0)], (1280, 720))
            data = p.read_bytes()
        kinds, pos = [], 0
        while pos < len(data):
            self.assertEqual(data[pos : pos + 2], b"PG")
            _pts, _dts, kind, size = struct.unpack(">IIBH", data[pos + 2 : pos + 13])
            kinds.append(kind)
            pos += 13 + size
        self.assertEqual(pos, len(data))
        # empty epoch + 2 × (PCS WDS PDS ODS END + PCS WDS END)
        self.assertEqual(kinds.count(pgs.ODS), 2)
        self.assertEqual(kinds.count(pgs.END), 5)


class ManifestTest(unittest.TestCase):
    def test_manifest_loads(self):
        entries = mf.load()
        self.assertGreater(len(entries), 80)
        for e in entries:
            self.assertTrue(e["licence"], e["id"])

    def test_every_synthetic_entry_has_a_recipe(self):
        needed = {e["recipe"] for e in mf.load() if e["source"] == "synthetic"}
        self.assertEqual(needed, set(generate.RECIPES))

    def test_external_entries_have_url_and_size(self):
        for e in mf.load():
            if e["source"] != "synthetic":
                self.assertTrue(e["url"].startswith("https://"), e["id"])
                self.assertIsInstance(e["size"], int, e["id"])

    def test_external_budget(self):
        total = sum(e.get("size") or 0 for e in mf.load() if e["source"] != "synthetic")
        self.assertLess(total, 1.5e9, "external samples must leave room for synthetic ones (2 GB total)")

    def test_rejects_unknown_keys(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "m.toml"
            p.write_text('schema = 1\n[sources.synthetic]\nlicence = "CC0-1.0"\n'
                         '[[file]]\nid = "a"\npath = "synthetic/a"\nwhat = "x"\ntypo = 1\n')
            with self.assertRaises(ValueError):
                mf.load(p)


class VerifyTest(unittest.TestCase):
    def test_stream_comparison(self):
        s = {"index": 0, "codec_type": "audio", "codec_name": "flac", "channels": 2, "sample_rate": "48000",
             "tags": {"LANGUAGE": "rus"}, "disposition": {"default": 1, "forced": 0}}
        ok = {"type": "audio", "codec": "flac", "channels": 2, "sample_rate": 48000, "lang": "rus", "default": True,
              "forced": False}
        self.assertEqual(verify.check_stream(0, ok, s, Path("x")), [])
        errs = verify.check_stream(0, {"channels": 6, "forced": True}, s, Path("x"))
        self.assertEqual(len(errs), 2)

    def test_text_checks(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "x.srt"
            p.write_bytes(texts.srt("ru").encode("cp1251"))
            self.assertEqual(verify.check_text({"encoding": "cp1251", "contains": "Ёлки"}, p), [])
            self.assertTrue(verify.check_text({"encoding": "utf-8"}, p))


class FetchTest(unittest.TestCase):
    def test_pin_inserts_sha_after_size(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "manifest.toml"
            shutil.copy(mf.MANIFEST, p)
            old = mf.MANIFEST
            mf.MANIFEST = p
            try:
                fetch.pin("x-tos-subs-en", "ab" * 32)
                entry = next(e for e in mf.load(p) if e["id"] == "x-tos-subs-en")
                other = next(e for e in mf.load(p) if e["id"] == "x-tos-subs-ru")
            finally:
                mf.MANIFEST = old
        self.assertEqual(entry["sha256"], "ab" * 32)
        self.assertNotIn("sha256", other)


if __name__ == "__main__":
    unittest.main()
