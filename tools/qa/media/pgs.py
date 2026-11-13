# SPDX-License-Identifier: MPL-2.0
"""Minimal PGS (Blu-ray HDMV presentation graphics, `.sup`) writer.

FFmpeg has no PGS encoder, so the synthetic PGS track is written here. The
same `.sup` is then transcoded by FFmpeg to VobSub (`dvdsub`) and DVB
(`dvbsub`): bitmap-to-bitmap conversion is supported, text-to-bitmap is not.

Format reference: US patent 2009/0185789 / "Presentation Graphic Stream (SUP
files) BluRay Subtitle Format" (segment layout as parsed by FFmpeg's
`pgssubdec.c`).
"""

from __future__ import annotations

import struct

PCS, WDS, PDS, ODS, END = 0x16, 0x17, 0x14, 0x15, 0x80

# Palette entries: (id, Y, Cr, Cb, alpha), BT.709 limited range.
PALETTE = [
    (1, 235, 128, 128, 255),  # white (frame)
    (2, 210, 146, 16, 255),  # yellow (event blocks)
    (3, 16, 128, 128, 160),  # translucent black (background)
]


def _segment(kind: int, pts: int, payload: bytes) -> bytes:
    return b"PG" + struct.pack(">IIBH", pts, 0, kind, len(payload)) + payload


def rle_line(row: list[int]) -> bytes:
    """Run-length encodes one bitmap line (PGS flavour) and appends end-of-line."""
    out = bytearray()
    i = 0
    while i < len(row):
        color = row[i]
        n = 1
        while i + n < len(row) and row[i + n] == color and n < 16383:
            n += 1
        if color:
            if n < 3:
                out += bytes([color]) * n
            elif n < 64:
                out += bytes([0, 0x80 | n, color])
            else:
                out += bytes([0, 0xC0 | (n >> 8), n & 0xFF, color])
        elif n < 64:
            out += bytes([0, n])
        else:
            out += bytes([0, 0x40 | (n >> 8), n & 0xFF])
        i += n
    return bytes(out + b"\x00\x00")


def _bitmap(width: int, height: int, blocks: int) -> list[list[int]]:
    """Frame + translucent background + `blocks` yellow squares (event number)."""
    rows = []
    for y in range(height):
        row = []
        for x in range(width):
            if y < 3 or y >= height - 3 or x < 3 or x >= width - 3:
                row.append(1)
            elif 10 <= y < height - 10 and (x - 10) // 40 < blocks and (x - 10) % 40 < 30 and x >= 10:
                row.append(2)
            else:
                row.append(3)
        rows.append(row)
    return rows


def write_sup(path, events: list[tuple[float, float]], video=(1920, 1080)) -> None:
    """Writes `events` [(start_s, end_s), ...]; event N shows N yellow blocks."""
    vw, vh = video
    out = bytearray()
    for k, (start, end) in enumerate(events):
        blocks = k + 1
        w, h = 20 + 40 * blocks, 50
        x, y = (vw - w) // 2, vh - h - vh // 12
        rle = b"".join(rle_line(r) for r in _bitmap(w, h, blocks))
        pts = round(start * 90000)
        pcs = struct.pack(">HHBHBBBB", vw, vh, 0x10, 2 * k, 0x80, 0, 0, 1)
        pcs += struct.pack(">HBBHH", 0, 0, 0, x, y)
        wds = struct.pack(">BBHHHH", 1, 0, x, y, w, h)
        pds = struct.pack(">BB", 0, 0) + b"".join(bytes(e) for e in PALETTE)
        ods = struct.pack(">HBB", 0, 0, 0xC0) + (len(rle) + 4).to_bytes(3, "big")
        ods += struct.pack(">HH", w, h) + rle
        out += _segment(PCS, pts, pcs) + _segment(WDS, pts, wds) + _segment(PDS, pts, pds)
        out += _segment(ODS, pts, ods) + _segment(END, pts, b"")
        # Clearing display set: composition without objects.
        pts = round(end * 90000)
        pcs = struct.pack(">HHBHBBBB", vw, vh, 0x10, 2 * k + 1, 0x00, 0, 0, 0)
        out += _segment(PCS, pts, pcs) + _segment(WDS, pts, wds) + _segment(END, pts, b"")
    with open(path, "wb") as f:
        f.write(out)
