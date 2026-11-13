// SPDX-License-Identifier: MPL-2.0
// Imports the curated icon subset (icons/manifest.json) from lucide-static,
// normalises it to our style (stroke width from the manifest, currentColor,
// round caps/joins) and optimises it with svgo into icons/svg/<name>.svg.
//
// Usage: npm run icons:import   (needs `npm ci` in design/)

import { readFile, writeFile, readdir, unlink } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { optimize } from 'svgo';
import { DESIGN_DIR, SPDX_TAG, loadManifest } from './lib/icons.mjs';

const require = createRequire(import.meta.url);
const lucideDir = path.join(path.dirname(require.resolve('lucide-static/package.json')), 'icons');
const outDir = path.join(DESIGN_DIR, 'icons', 'svg');

// Icons that Lucide derived from Feather (MIT) are listed in its LICENSE under
// their old names; map the renamed ones so their SPDX header keeps MIT.
const RENAMED = { 'alert-triangle': 'triangle-alert', 'alert-circle': 'circle-alert', 'more-horizontal': 'ellipsis', 'more-vertical': 'ellipsis-vertical', 'help-circle': 'circle-help', 'x-circle': 'circle-x' };
async function featherIcons() {
  const lic = await readFile(path.join(lucideDir, '..', 'LICENSE'), 'utf8');
  const list = lic.match(/derived from the Feather project:\s*\n\s*\n([^\n]+)/)?.[1];
  if (!list) throw new Error('lucide-static LICENSE: Feather icon list not found; update icons-import.mjs');
  return new Set(list.split(',').map((s) => s.trim()).flatMap((n) => [n, RENAMED[n]].filter(Boolean)));
}

export function normalise(svg, strokeWidth, spdx = 'ISC') {
  const { data } = optimize(svg, {
    multipass: true,
    plugins: [
      {
        name: 'preset-default',
        params: { overrides: { convertShapeToPath: false } },
      },
      'removeComments',
      'sortAttrs',
    ],
  });
  // Root attributes are rewritten deterministically so every icon has the same header.
  const body = data.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
  const root =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" ' +
    `stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round">`;
  // Tag assembled so REUSE does not parse this template as a licence header.
  return `<!-- ${SPDX_TAG}: ${spdx} -->\n${root}${body}</svg>\n`;
}

async function main() {
  const manifest = await loadManifest();
  const wanted = new Set(Object.keys(manifest.icons));
  const feather = await featherIcons();
  for (const [name, { lucide }] of Object.entries(manifest.icons)) {
    if (!lucide) continue; // own icon: the SVG in icons/svg is the source, only linted
    const src = await readFile(path.join(lucideDir, `${lucide}.svg`), 'utf8');
    const spdx = feather.has(lucide) ? 'ISC AND MIT' : 'ISC'; // Feather-derived icons keep MIT
    await writeFile(path.join(outDir, `${name}.svg`), normalise(src, manifest.strokeWidth, spdx));
  }
  // Drop icons that are no longer in the manifest.
  for (const file of await readdir(outDir)) {
    if (file.endsWith('.svg') && !wanted.has(file.slice(0, -4))) {
      await unlink(path.join(outDir, file));
      console.log(`removed ${file}`);
    }
  }
  console.log(`imported ${wanted.size} icons into ${path.relative(process.cwd(), outDir) || outDir}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
