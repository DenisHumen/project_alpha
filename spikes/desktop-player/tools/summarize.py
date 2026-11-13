#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Turns out/results.jsonl from bench-headless.sh into a Markdown table."""
import json
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "out/results.jsonl"
print("| run | VO / context | hwdec | delivered fps | CPU, % of 1 core | render ms avg / max | RSS, MB |")
print("|---|---|---|---|---|---|---|")
for line in open(path, encoding="utf-8"):
    d = json.loads(line)
    r = d["r"]
    window = r.get("window_s") or 0
    if r.get("mode") == "texture" and window:
        fps = f'{r["populate_calls_in_window"] / window:.1f} (src {r.get("container-fps", 0):.0f})'
        render = f'{r["render_ms_avg"]:.1f} / {r["render_ms_max"]:.0f}'
    else:
        fps = f'n/a (src {r.get("container-fps", 0):.0f}, drops {r.get("frame-drop-count", 0):.0f})'
        render = "—"
    vo = r.get("current-vo", "?")
    if r.get("current-gpu-context"):
        vo += "/" + r["current-gpu-context"]
    cpu = r.get("cpu_percent_one_core") or 0
    print(f'| {d["run"]} | {vo} | {r.get("hwdec-current", "?")} | {fps} | {cpu:.1f} | {render} | {r["max_rss_kb"] / 1024:.0f} |')
