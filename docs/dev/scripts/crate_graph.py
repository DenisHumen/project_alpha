#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Generate the workspace crate graph (Mermaid) into docs/dev/crate-graph.md.

Reads `cargo metadata --no-deps` and draws only workspace-internal edges
(dependencies between workspace members). External crates are only counted,
not drawn, so the picture stays readable.

Usage:
    python3 docs/dev/scripts/crate_graph.py          # rewrite the file
    python3 docs/dev/scripts/crate_graph.py --check  # exit 1 if it is stale

Owned by the DOC workstream (DOC-003). Stdlib only, deterministic output.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

BEGIN = "<!-- BEGIN GENERATED: crate-graph -->"
END = "<!-- END GENERATED: crate-graph -->"
OUT = pathlib.Path("docs/dev/crate-graph.md")

# Display order and labels of directory groups (prefix of the manifest dir).
GROUPS = [
    ("core/crates/", "core/crates — ядро"),
    ("server", "server — медиасервер"),
    ("tools/", "tools — инструменты"),
]
OTHER = "прочее"


def cargo_metadata(cwd: pathlib.Path) -> dict:
    raw = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(raw)


def node_id(name: str) -> str:
    return name.replace("-", "_")


def render(meta: dict) -> str:
    root = pathlib.Path(meta["workspace_root"])
    members = set(meta["workspace_members"])
    pkgs = sorted(
        (p for p in meta["packages"] if p["id"] in members), key=lambda p: p["name"]
    )
    names = {p["name"] for p in pkgs}

    def rel_dir(p: dict) -> str:
        return pathlib.Path(p["manifest_path"]).parent.relative_to(root).as_posix()

    def group_of(p: dict) -> str:
        d = rel_dir(p)
        for prefix, label in GROUPS:
            if d == prefix.rstrip("/") or d.startswith(prefix):
                return label
        return OTHER

    edges: set[tuple[str, str, str]] = set()
    external: dict[str, int] = {}
    for p in pkgs:
        ext = set()
        for d in p["dependencies"]:
            kind = d.get("kind") or "normal"
            if d["name"] in names:
                edges.add((p["name"], d["name"], kind))
            elif kind == "normal":
                ext.add(d["name"])
        external[p["name"]] = len(ext)

    lines = [BEGIN, ""]
    if not pkgs:
        lines += ["В workspace пока нет крейтов.", ""]
    else:
        lines += ["```mermaid", "flowchart LR"]
        by_group: dict[str, list[dict]] = {}
        for p in pkgs:
            by_group.setdefault(group_of(p), []).append(p)
        order = [label for _, label in GROUPS] + [OTHER]
        for i, label in enumerate(g for g in order if g in by_group):
            lines.append(f'  subgraph g{i}["{label}"]')
            for p in by_group[label]:
                lines.append(f'    {node_id(p["name"])}["{p["name"]}"]')
            lines.append("  end")
        for src, dst, kind in sorted(edges):
            arrow = "-->" if kind == "normal" else f"-. {kind} .->"
            lines.append(f"  {node_id(src)} {arrow} {node_id(dst)}")
        lines += ["```", ""]
        lines += [
            "| Крейт | Каталог | Внешних зависимостей | Описание |",
            "|---|---|---|---|",
        ]
        for p in pkgs:
            desc = (p.get("description") or "").replace("|", "\\|").strip()
            lines.append(
                f'| `{p["name"]}` | `{rel_dir(p)}` | {external[p["name"]]} | {desc} |'
            )
        lines.append("")
    lines.append(END)
    return "\n".join(lines)


def splice(text: str, block: str) -> str:
    start, end = text.find(BEGIN), text.find(END)
    if start == -1 or end == -1 or end < start:
        sys.exit(f"{OUT}: markers {BEGIN!r} / {END!r} not found")
    return text[:start] + block + text[end + len(END) :]


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the file is stale")
    args = ap.parse_args()

    meta = cargo_metadata(pathlib.Path.cwd())
    path = pathlib.Path(meta["workspace_root"]) / OUT
    old = path.read_text(encoding="utf-8")
    new = splice(old, render(meta))
    if args.check:
        if new != old:
            print(
                f"{OUT} is stale: run `just docs::crate-graph` and commit the result",
                file=sys.stderr,
            )
            return 1
        print(f"{OUT} is up to date")
        return 0
    if new != old:
        path.write_text(new, encoding="utf-8")
        print(f"updated {OUT}")
    else:
        print(f"{OUT} already up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main())
