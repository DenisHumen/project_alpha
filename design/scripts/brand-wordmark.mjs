#!/usr/bin/env node
// SPDX-License-Identifier: MPL-2.0
//
// Outlines the "Kinolune" wordmark (DSN-006) from Inter Display SemiBold into
// design/brand/src/wordmark.svg, so the logo never depends on an installed font.
// Run only when the wordmark changes; the output is committed.
//
//   node scripts/brand-wordmark.mjs --font <InterDisplay-SemiBold.ttf>
//
// The font comes from the pinned Inter 4.1 archive (design/fonts/fonts.lock.json,
// file extras/ttf/InterDisplay-SemiBold.ttf). Inter is OFL-1.1: outlined glyphs
// in a logo are artwork made with the font, not a modified font.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import opentype from 'opentype.js';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const TEXT = 'Kinolune';
const SIZE = 200; // em size in SVG units
const TRACKING = -0.01; // em; Inter Display is already tight at large sizes

const { values } = parseArgs({ options: { font: { type: 'string' }, out: { type: 'string' } } });
if (!values.font) {
  console.error('usage: brand-wordmark.mjs --font <InterDisplay-SemiBold.ttf> [--out file]');
  process.exit(2);
}
const buf = readFileSync(values.font);
const font = opentype.parse(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));

const scale = SIZE / font.unitsPerEm;
// No shaping: plain cmap lookup (opentype.js cannot run every Inter GSUB lookup).
const glyphs = [...TEXT].map((c) => font.charToGlyph(c));
const bb = { x1: Infinity, x2: -Infinity, y1: Infinity, y2: -Infinity };
let x = 0;
const parts = [];
for (let i = 0; i < glyphs.length; i++) {
  const g = glyphs[i];
  const p = g.getPath(x, 0, SIZE);
  const gb = p.getBoundingBox();
  bb.x1 = Math.min(bb.x1, gb.x1);
  bb.x2 = Math.max(bb.x2, gb.x2);
  bb.y1 = Math.min(bb.y1, gb.y1);
  bb.y2 = Math.max(bb.y2, gb.y2);
  parts.push(p.toPathData(2));
  x += g.advanceWidth * scale + TRACKING * SIZE;
  if (i < glyphs.length - 1) x += font.getKerningValue(g, glyphs[i + 1]) * scale;
}
const left = Math.floor(bb.x1);
const ascent = font.tables.os2.sCapHeight * scale; // cap height: "K", "l" ascender is close
const xHeight = font.tables.os2.sxHeight * scale;
const top = Math.floor(bb.y1);
const bottom = Math.ceil(bb.y2);
const h = bottom - top;
const w = Math.ceil(bb.x2) - left;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${left} ${top} ${w} ${h}" width="${w}" height="${h}">
  <!-- SPDX-License-Identifier: MPL-2.0 -->
  <!-- Kinolune wordmark (DSN-006): Inter Display SemiBold (OFL-1.1) outlined by
       scripts/brand-wordmark.mjs; size ${SIZE}, tracking ${TRACKING} em, baseline y=0,
       cap height ${ascent.toFixed(1)}, x-height ${xHeight.toFixed(1)}. Do not edit by hand. -->
  <path id="wordmark" fill="#f3efe4" d="${parts.join('')}"/>
</svg>
`;
const out = values.out ?? join(ROOT, 'brand/src/wordmark.svg');
writeFileSync(out, svg);
console.log(`wrote ${out} (${w}×${h})`);
