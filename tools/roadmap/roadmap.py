#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Lint docs/ROADMAP.md and list tasks that are ready to start.

Standard library only (Python >= 3.10), so it runs on every CI runner and dev
machine without a virtualenv. Task format: docs/ROADMAP.md §0.1.

    python3 tools/roadmap/roadmap.py lint [--file PATH]
    python3 tools/roadmap/roadmap.py next [WORKSTREAM ...] [--offline] [--json]
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_FILE = Path(__file__).resolve().parents[2] / "docs" / "ROADMAP.md"

ID_RE = r"[A-Z]+(?:-[A-Z]+)*-\d{3}"
TASK_RE = re.compile(
    rf"^- \[(?P<mark>[ xX])\] (?P<strike>~~)?\*\*(?P<id>{ID_RE})\*\*\s*(?P<rest>.*)$"
)
# Anything that looks like a task line, used to report malformed ones.
TASK_LIKE_RE = re.compile(r"^- \[[^\]]*\]")
FIELD_RE = re.compile(r"^\s+- (?P<key>Что|deps|done when):\s*(?P<value>.*)$")
SECTION_RE = re.compile(rf"^###\s+[\d.]+\s+(?P<ws>[A-Z]+(?:-[A-Z]+)*)\s+—")
H2_RE = re.compile(r"^##\s")
DEPS_RE = re.compile(
    r"^(?P<deps>.+?) · size: (?P<size>[SML]) · parallel: (?P<parallel>yes|no)"
    r" · веха: M(?P<milestone>\d+) · ref: (?P<ref>.+)$"
)
BLOCKED_RE = re.compile(rf"\(blocked: (?P<ids>{ID_RE}(?:,\s*{ID_RE})*)\)")
PR_ID_RE = re.compile(rf"\[({ID_RE})\]")
NO_DEPS = "—"
SUMMARY_HEADING = "Сводка по количеству задач"
SUMMARY_ROW_RE = re.compile(r"\|\s*([A-Z]+(?:-[A-Z]+)*)\s*\|\s*(\d+)\s*(?=\|)")


@dataclass
class Task:
    id: str
    line: int
    done: bool
    cancelled: bool
    owner: bool
    title: str
    workstream: str
    section: str | None = None
    deps: list[str] = field(default_factory=list)
    blocked_by: list[str] = field(default_factory=list)
    size: str | None = None
    parallel: str | None = None
    milestone: int | None = None
    fields: dict[str, list[str]] = field(default_factory=dict)


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def error(self, line: int, msg: str) -> None:
        self.errors.append(f"line {line}: {msg}")

    def warn(self, line: int, msg: str) -> None:
        self.warnings.append(f"line {line}: {msg}")


def workstream_of(task_id: str) -> str:
    return task_id.rsplit("-", 1)[0]


def parse(text: str, report: Report | None = None) -> list[Task]:
    """Parse task entries, skipping fenced code blocks."""
    report = report if report is not None else Report()
    tasks: list[Task] = []
    current: Task | None = None
    section: str | None = None
    in_fence = False

    for lineno, line in enumerate(text.splitlines(), start=1):
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue

        if line.startswith("#"):
            current = None
            m = SECTION_RE.match(line)
            if m:
                section = m.group("ws")
            elif H2_RE.match(line):
                section = None
            continue

        m = TASK_RE.match(line)
        if m:
            rest = m.group("rest")
            cancelled = m.group("strike") is not None
            title = rest.split("~~", 1)[0].strip() if cancelled else rest.strip()
            blocked = BLOCKED_RE.search(rest)
            current = Task(
                id=m.group("id"),
                line=lineno,
                done=m.group("mark") in "xX",
                cancelled=cancelled,
                owner="(owner)" in rest,
                title=title,
                workstream=workstream_of(m.group("id")),
                section=section,
                blocked_by=re.split(r",\s*", blocked.group("ids")) if blocked else [],
            )
            tasks.append(current)
            continue
        if TASK_LIKE_RE.match(line):
            report.error(lineno, f"malformed task line: {line.strip()[:60]}")
            current = None
            continue

        f = FIELD_RE.match(line)
        if f and current is not None:
            current.fields.setdefault(f.group("key"), []).append(f.group("value"))
            if f.group("key") == "deps" and len(current.fields["deps"]) == 1:
                _parse_deps(current, f.group("value"), lineno, report)
            continue
        if line.strip() and not line.startswith(" "):
            current = None

    return tasks


def _parse_deps(task: Task, value: str, lineno: int, report: Report) -> None:
    m = DEPS_RE.match(value.strip())
    if not m:
        report.error(
            lineno,
            f"{task.id}: deps line must be "
            "'deps: … · size: S|M|L · parallel: yes|no · веха: MN · ref: …'",
        )
        return
    task.size = m.group("size")
    task.parallel = m.group("parallel")
    task.milestone = int(m.group("milestone"))
    deps = m.group("deps").strip()
    if deps == NO_DEPS:
        return
    for dep in re.split(r",\s*", deps):
        if re.fullmatch(ID_RE, dep):
            task.deps.append(dep)
        else:
            report.error(lineno, f"{task.id}: bad dependency ID {dep!r}")


def find_cycles(tasks: dict[str, Task]) -> list[list[str]]:
    """Return one representative path per dependency cycle (iterative DFS)."""
    white, grey, black = 0, 1, 2
    color = dict.fromkeys(tasks, white)
    cycles: list[list[str]] = []
    for root in sorted(tasks):
        if color[root] != white:
            continue
        stack: list[tuple[str, int]] = [(root, 0)]
        path: list[str] = []
        while stack:
            node, idx = stack.pop()
            if idx == 0:
                color[node] = grey
                path.append(node)
            deps = [d for d in tasks[node].deps + tasks[node].blocked_by if d in tasks]
            if idx < len(deps):
                stack.append((node, idx + 1))
                dep = deps[idx]
                if color[dep] == grey:
                    cycles.append(path[path.index(dep):] + [dep])
                elif color[dep] == white:
                    stack.append((dep, 0))
            else:
                color[node] = black
                path.pop()
    return cycles


def lint(text: str) -> tuple[list[Task], Report]:
    report = Report()
    tasks = parse(text, report)
    by_id: dict[str, Task] = {}

    for t in tasks:
        if t.id in by_id:
            report.error(t.line, f"duplicate ID {t.id} (first at line {by_id[t.id].line})")
            continue
        by_id[t.id] = t

    for t in tasks:
        if t.section is not None and t.workstream != t.section:
            report.error(t.line, f"{t.id} is listed under workstream {t.section}")
        if t.cancelled:
            if t.done:
                report.error(t.line, f"{t.id}: a cancelled task stays '- [ ]'")
            continue
        if not t.title:
            report.error(t.line, f"{t.id}: empty title")
        for key in ("Что", "deps", "done when"):
            values = t.fields.get(key, [])
            if not values:
                report.error(t.line, f"{t.id}: missing '- {key}:' line")
            elif len(values) > 1:
                if len(set(values)) == 1:
                    report.warn(t.line, f"{t.id}: '- {key}:' line is repeated")
                else:
                    report.error(t.line, f"{t.id}: conflicting '- {key}:' lines")
        for dep in t.deps + t.blocked_by:
            if dep == t.id:
                report.error(t.line, f"{t.id} depends on itself")
            elif dep not in by_id:
                report.error(t.line, f"{t.id}: unknown dependency {dep}")
            elif by_id[dep].cancelled:
                report.error(t.line, f"{t.id}: depends on cancelled task {dep}")
            elif t.done and not by_id[dep].done:
                report.error(t.line, f"{t.id} is ticked but dependency {dep} is open")
        for dep in t.deps:
            d = by_id.get(dep)
            if (
                d is not None
                and d.milestone is not None
                and t.milestone is not None
                and d.milestone > t.milestone
            ):
                report.error(
                    t.line,
                    f"{t.id} (M{t.milestone}) depends on {dep} from a later "
                    f"milestone M{d.milestone}",
                )

    _check_summary(text, tasks, report)

    for cycle in find_cycles(by_id):
        report.error(by_id[cycle[0]].line, "dependency cycle: " + " -> ".join(cycle))

    return tasks, report


def _check_summary(text: str, tasks: list[Task], report: Report) -> None:
    """Compare the per-workstream table in the summary section with real counts."""
    in_summary = False
    counts: dict[str, int] = {}
    for t in tasks:
        counts[t.workstream] = counts.get(t.workstream, 0) + 1
    for lineno, line in enumerate(text.splitlines(), start=1):
        if line.startswith("## "):
            in_summary = SUMMARY_HEADING in line
            continue
        if not in_summary:
            continue
        for ws, n in SUMMARY_ROW_RE.findall(line):
            if counts.get(ws, 0) != int(n):
                report.warn(
                    lineno, f"summary says {ws} has {n} tasks, found {counts.get(ws, 0)}"
                )


def ready_tasks(
    tasks: list[Task], workstreams: list[str] | None = None, taken: set[str] | None = None
) -> list[Task]:
    """Open, non-owner tasks whose deps (and blockers) are all ticked."""
    by_id = {t.id: t for t in tasks}
    wanted = {w.upper() for w in workstreams} if workstreams else None
    taken = taken or set()
    out = []
    for t in tasks:
        if t.done or t.cancelled or t.owner or t.id in taken:
            continue
        if wanted is not None and t.workstream not in wanted:
            continue
        if all(by_id.get(d) is not None and by_id[d].done for d in t.deps + t.blocked_by):
            out.append(t)
    return out


def open_pr_ids() -> set[str] | None:
    """IDs in titles of open PRs, or None when `gh` is unavailable."""
    if shutil.which("gh") is None:
        return None
    try:
        res = subprocess.run(
            ["gh", "pr", "list", "--state", "open", "--limit", "500", "--json", "title"],
            capture_output=True,
            text=True,
            check=True,
            timeout=60,
        )
    except (subprocess.SubprocessError, OSError):
        return None
    ids: set[str] = set()
    for pr in json.loads(res.stdout or "[]"):
        ids.update(PR_ID_RE.findall(pr.get("title", "")))
    return ids


def cmd_lint(args: argparse.Namespace) -> int:
    tasks, report = lint(Path(args.file).read_text(encoding="utf-8"))
    for w in report.warnings:
        print(f"warning: {w}", file=sys.stderr)
    for e in report.errors:
        print(f"error: {e}", file=sys.stderr)
    done = sum(t.done for t in tasks)
    print(
        f"roadmap lint: {len(tasks)} tasks ({done} done), "
        f"{len(report.errors)} errors, {len(report.warnings)} warnings"
    )
    return 1 if report.errors else 0


def cmd_next(args: argparse.Namespace) -> int:
    tasks, report = lint(Path(args.file).read_text(encoding="utf-8"))
    if report.errors:
        print("roadmap has lint errors; run `just roadmap lint`", file=sys.stderr)
        return 1
    taken: set[str] = set()
    if not args.offline:
        ids = open_pr_ids()
        if ids is None:
            print(
                "warning: `gh` unavailable, tasks with open PRs are not filtered "
                "(use --offline to silence)",
                file=sys.stderr,
            )
        else:
            taken = ids
    ready = ready_tasks(tasks, args.workstream, taken)
    if args.json:
        rows = [
            {
                "id": t.id,
                "workstream": t.workstream,
                "title": t.title,
                "size": t.size,
                "parallel": t.parallel,
                "milestone": f"M{t.milestone}" if t.milestone is not None else None,
                "deps": t.deps,
            }
            for t in ready
        ]
        print(json.dumps(rows, ensure_ascii=False, indent=2))
        return 0
    for t in ready:
        print(f"{t.id:<14} M{t.milestone} {t.size} par:{t.parallel:<3} {t.title}")
    print(f"{len(ready)} ready tasks", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--file", default=str(DEFAULT_FILE), help="path to ROADMAP.md")
    parser = argparse.ArgumentParser(prog="roadmap", description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)

    p_lint = sub.add_parser("lint", parents=[common], help="validate the task list")
    p_lint.set_defaults(func=cmd_lint)

    p_next = sub.add_parser("next", parents=[common], help="list tasks ready to start")
    p_next.add_argument("workstream", nargs="*", help="filter, e.g. CORE PLAY-AND")
    p_next.add_argument("--offline", action="store_true", help="do not query open PRs")
    p_next.add_argument("--json", action="store_true", help="machine-readable output")
    p_next.set_defaults(func=cmd_next)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
