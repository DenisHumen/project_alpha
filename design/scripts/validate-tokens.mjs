// SPDX-License-Identifier: MPL-2.0
// Validates design/tokens (DSN-002):
//   1. DTCG structure, $type and literal values of every *.tokens.json;
//   2. every reference resolves, is type-compatible and acyclic — for each theme × density;
//   3. themes/* override only existing semantic tokens; density/* define identical token sets;
//   4. WCAG contrast of the pairs in tokens/contrast.pairs.json for every theme.
// Usage: node scripts/validate-tokens.mjs [--report]   (no dependencies)

import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { contrast, flatten, hexToRgb, over, resolve, validateSet } from './lib/dtcg.mjs';
import { DESIGN_DIR } from './lib/icons.mjs';

const TOKENS_DIR = path.join(DESIGN_DIR, 'tokens');

async function loadFile(dir, rel, errors) {
  const file = path.join(dir, rel);
  let tree;
  try {
    tree = JSON.parse(await readFile(file, 'utf8'));
  } catch (e) {
    errors.push(`${rel}: ${e.message}`);
    return new Map();
  }
  return flatten(tree, rel, errors);
}

const listFiles = async (dir, sub) =>
  (await readdir(path.join(dir, sub))).filter((f) => f.endsWith('.tokens.json')).sort();

/** Composites a stack of colour tokens (top first) over an opaque backdrop. */
function flattenStack(tokens, stack, backdrop) {
  let base = backdrop;
  for (const p of [...stack].reverse()) base = over(resolve(tokens, p), base);
  return base;
}

export async function validate({ report = false, dir = TOKENS_DIR } = {}) {
  const errors = [];
  const load = (rel, errs) => loadFile(dir, rel, errs);
  const tokenFiles = (sub) => listFiles(dir, sub);
  const primitive = await load('primitive.tokens.json', errors);
  const semantic = await load('semantic.tokens.json', errors);
  const component = await load('component.tokens.json', errors);
  const themes = { dark: new Map() };
  for (const f of await tokenFiles('themes')) themes[f.replace('.tokens.json', '')] = await load(`themes/${f}`, errors);
  const densities = {};
  for (const f of await tokenFiles('density')) densities[f.replace('.tokens.json', '')] = await load(`density/${f}`, errors);

  // Layer rules.
  for (const [name, t] of Object.entries(themes)) {
    for (const [p, tok] of t) {
      const base = semantic.get(p);
      if (!base) errors.push(`themes/${name}: ${p} does not exist in semantic.tokens.json (themes may only override)`);
      else if (base.type !== tok.type) errors.push(`themes/${name}: ${p} is ${tok.type}, semantic has ${base.type}`);
    }
  }
  const dnames = Object.keys(densities);
  if (!dnames.length) errors.push('density/: no density profiles');
  const ref = densities[dnames[0]] ?? new Map();
  for (const n of dnames) {
    const d = densities[n];
    for (const p of ref.keys()) if (!d.has(p)) errors.push(`density/${n}: missing ${p} (defined in density/${dnames[0]})`);
    for (const [p, t] of d) {
      if (!ref.has(p)) errors.push(`density/${n}: ${p} not defined in density/${dnames[0]}`);
      else if (ref.get(p).type !== t.type) errors.push(`density/${n}: ${p} type ${t.type} differs from density/${dnames[0]}`);
      if (t.type === 'color') errors.push(`density/${n}: ${p}: density profiles must not define colours`);
    }
  }
  const layers = { primitive, semantic, density: ref, component };
  const seen = new Map();
  for (const [layer, m] of Object.entries(layers)) {
    for (const p of m.keys()) {
      if (seen.has(p)) errors.push(`${p} is defined in both ${seen.get(p)} and ${layer} layers`);
      else seen.set(p, layer);
    }
  }

  // Resolve every theme × density combination.
  const sets = [];
  for (const [tn, theme] of Object.entries(themes)) {
    for (const [dn, density] of Object.entries(densities)) {
      const all = new Map([...primitive, ...semantic, ...theme, ...density, ...component]);
      const label = `[${tn}/${dn}]`;
      const errs = validateSet(all, label);
      // Same error repeats across combinations; keep the first occurrence of each message body.
      for (const e of errs) {
        const body = e.slice(label.length);
        if (!errors.some((x) => x.endsWith(body))) errors.push(e);
      }
      sets.push({ tn, dn, all });
    }
  }

  // Contrast — colours do not depend on density, so one combination per theme.
  const pairs = JSON.parse(await readFile(path.join(dir, 'contrast.pairs.json'), 'utf8')).pairs;
  // Lowering a pair below WCAG AA for normal text needs a written reason; nothing goes below 3:1.
  for (const pair of pairs) {
    const id = `${pair.fg} on ${pair.bg.join(' / ')}`;
    if (!(typeof pair.min === 'number' && pair.min >= 3)) errors.push(`contrast.pairs.json: ${id}: min must be ≥ 3 (4.5 for normal text)`);
    else if (pair.min < 4.5 && !pair.note) errors.push(`contrast.pairs.json: ${id}: min ${pair.min} < 4.5 needs a "note" (large/inactive text or non-text UI)`);
  }
  const rows = [];
  if (!errors.length) {
    for (const [tn] of Object.entries(themes)) {
      const all = sets.find((s) => s.tn === tn).all;
      for (const pair of pairs) {
        const backdrops = (pair.backdrops ?? [null]).map((h) => (h ? hexToRgb(h) : null));
        for (const bd of backdrops) {
          const bottom = resolve(all, pair.bg.at(-1));
          if (!bd && (bottom.alpha ?? 1) < 1) {
            errors.push(`contrast ${tn}: ${pair.bg.at(-1)} is translucent; add "backdrops" to the pair`);
            continue;
          }
          const bg = flattenStack(all, pair.bg, bd ?? [0, 0, 0]);
          const fg = over(resolve(all, pair.fg), bg);
          const ratio = contrast(fg, bg);
          const on = pair.bg.join(' / ') + (pair.backdrops ? ` over ${pair.backdrops[backdrops.indexOf(bd)]}` : '');
          rows.push({ theme: tn, fg: pair.fg, on, ratio, min: pair.min });
          if (ratio + 1e-9 < pair.min) errors.push(`contrast ${tn}: ${pair.fg} on ${on} = ${ratio.toFixed(2)}:1 < ${pair.min}:1`);
        }
      }
    }
  }

  if (report) {
    console.log('| Тема | Текст | Фон | Контраст | Минимум |\n|---|---|---|---|---|');
    for (const r of rows) console.log(`| ${r.theme} | \`${r.fg}\` | \`${r.on}\` | ${r.ratio.toFixed(2)}:1 | ${r.min}:1 |`);
  }
  const count = primitive.size + semantic.size + component.size + ref.size;
  return { errors, count, combos: sets.length, pairs: rows.length };
}

async function main() {
  const { errors, count, combos, pairs } = await validate({ report: process.argv.includes('--report') });
  if (errors.length) {
    console.error(errors.join('\n'));
    console.error(`\ntokens:validate failed: ${errors.length} problem(s)`);
    process.exit(1);
  }
  console.log(`tokens:validate OK: ${count} tokens, ${combos} theme×density sets resolved, ${pairs} contrast checks ≥ AA`);
}

if (import.meta.url === `file://${process.argv[1]}`) main();
