// SPDX-License-Identifier: MPL-2.0
// Icon linter (DSN-004): checks every icons/svg/*.svg against the style rules
// in icons/svg/README.md. Exit code 1 on any violation. No dependencies.

import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { DESIGN_DIR, NAME_RE, SPDX_TAG, loadManifest } from './lib/icons.mjs';

const ALLOWED_ELEMENTS = new Set(['path', 'circle', 'ellipse', 'rect', 'line', 'polyline', 'polygon', 'g']);
const ALLOWED_PAINT = new Set(['none', 'currentColor']);
const MAX_BYTES = 4096;

/** Returns a list of problems for one SVG source; empty list = OK. */
export function lintSvg(name, src, { strokeWidths = [1.75, 2] } = {}) {
  const errors = [];
  const err = (m) => errors.push(`${name}: ${m}`);

  if (!NAME_RE.test(name)) err('file name must be kebab-case');
  if (Buffer.byteLength(src) > MAX_BYTES) err(`larger than ${MAX_BYTES} bytes, optimise it (npm run icons:import)`);
  if (!src.startsWith(`<!-- ${SPDX_TAG}:`)) err('missing SPDX licence comment on the first line');

  const root = src.match(/<svg\b([^>]*)>/);
  if (!root) return [...errors, `${name}: no <svg> root`];
  const attrs = Object.fromEntries([...root[1].matchAll(/([\w:-]+)="([^"]*)"/g)].map((m) => [m[1], m[2]]));
  const expect = (k, v) => attrs[k] !== v && err(`<svg ${k}="${attrs[k] ?? ''}">, expected "${v}"`);
  expect('viewBox', '0 0 24 24');
  expect('fill', 'none');
  expect('stroke', 'currentColor');
  expect('stroke-linecap', 'round');
  expect('stroke-linejoin', 'round');
  if (!strokeWidths.includes(Number(attrs['stroke-width']))) {
    err(`stroke-width="${attrs['stroke-width']}", expected one of ${strokeWidths.join(', ')}`);
  }
  for (const k of ['width', 'height', 'style', 'class']) if (k in attrs) err(`<svg> must not set ${k}`);

  const body = src.slice(root.index + root[0].length);
  for (const m of body.matchAll(/<([a-zA-Z][\w:-]*)\b([^>]*)>/g)) {
    const [, tag, rest] = m;
    if (!ALLOWED_ELEMENTS.has(tag)) {
      err(`element <${tag}> is not allowed (only ${[...ALLOWED_ELEMENTS].join(', ')})`);
      continue;
    }
    for (const a of rest.matchAll(/([\w:-]+)="([^"]*)"/g)) {
      const [, k, v] = a;
      if ((k === 'fill' || k === 'stroke') && !ALLOWED_PAINT.has(v)) err(`<${tag} ${k}="${v}">: colours must be currentColor`);
      if (k === 'stroke-width' && !strokeWidths.includes(Number(v))) err(`<${tag}> overrides stroke-width="${v}"`);
      if (k === 'style' || k === 'class' || k.startsWith('on') || k.endsWith('href')) err(`<${tag}> must not use ${k}`);
    }
  }
  return errors;
}

async function main() {
  const manifest = await loadManifest();
  const dir = path.join(DESIGN_DIR, 'icons', 'svg');
  const files = (await readdir(dir)).filter((f) => f.endsWith('.svg'));
  const names = new Set(files.map((f) => f.slice(0, -4)));
  const errors = [];

  for (const file of files) {
    const name = file.slice(0, -4);
    if (!(name in manifest.icons)) errors.push(`${name}: not listed in icons/manifest.json`);
    errors.push(...lintSvg(name, await readFile(path.join(dir, file), 'utf8')));
  }
  for (const name of Object.keys(manifest.icons)) {
    if (!names.has(name)) errors.push(`${name}: listed in manifest but icons/svg/${name}.svg is missing`);
    if (!NAME_RE.test(name)) errors.push(`${name}: manifest key must be kebab-case`);
  }

  if (errors.length) {
    console.error(errors.join('\n'));
    console.error(`\nicons:lint failed: ${errors.length} problem(s)`);
    process.exit(1);
  }
  console.log(`icons:lint OK: ${files.length} icons`);
}

if (import.meta.url === `file://${process.argv[1]}`) main();
