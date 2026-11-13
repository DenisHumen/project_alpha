# SPDX-License-Identifier: MPL-2.0
"""Loads and validates tests/media/manifest.toml (shared by generate/verify/fetch)."""

from __future__ import annotations

import tomllib
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[3] / "tests" / "media" / "manifest.toml"

ENTRY_KEYS = {"id", "path", "source", "recipe", "what", "used_by", "licence", "attribution",
              "url", "size", "sha256", "expect", "streams", "text"}


def load(path: Path = MANIFEST) -> list[dict]:
    """Returns file entries with `source`, `recipe` and `licence` resolved; raises on schema errors."""
    with open(path, "rb") as f:
        doc = tomllib.load(f)
    if doc.get("schema") != 1:
        raise ValueError(f"{path}: unsupported schema {doc.get('schema')!r}")
    sources = doc.get("sources", {})
    entries, ids, paths, errors = [], set(), set(), []
    for e in doc.get("file", []):
        e = dict(e)
        where = e.get("id", "<no id>")
        unknown = set(e) - ENTRY_KEYS
        if unknown:
            errors.append(f"{where}: unknown keys {sorted(unknown)}")
        for key in ("id", "path", "what"):
            if not e.get(key):
                errors.append(f"{where}: missing `{key}`")
        e.setdefault("source", "synthetic")
        e.setdefault("recipe", e.get("id"))
        src = sources.get(e["source"])
        if src is None:
            errors.append(f"{where}: unknown source {e['source']!r}")
            src = {}
        e.setdefault("licence", src.get("licence"))
        e.setdefault("attribution", src.get("attribution"))
        if not e.get("licence"):
            errors.append(f"{where}: no licence")
        if e["source"] != "synthetic" and not e.get("url"):
            errors.append(f"{where}: external file without `url`")
        if e["id"] in ids:
            errors.append(f"{where}: duplicate id")
        if e["path"] in paths:
            errors.append(f"{where}: duplicate path {e['path']}")
        prefix = "synthetic/" if e["source"] == "synthetic" else "external/"
        if not e["path"].startswith(prefix):
            errors.append(f"{where}: path must start with {prefix}")
        ids.add(e["id"])
        paths.add(e["path"])
        entries.append(e)
    if errors:
        raise ValueError(f"{path}:\n  " + "\n  ".join(errors))
    return entries
