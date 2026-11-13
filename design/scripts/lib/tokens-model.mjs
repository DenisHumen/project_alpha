// SPDX-License-Identifier: MPL-2.0
// Platform-neutral model of design/tokens for the code generators (DSN-003).
//
// Every semantic, density and component token is resolved for every theme × density.
// A token whose value differs by theme goes to the colour scheme, one that differs by
// density goes to the density profile, the rest are constants. Primitives are not
// exported: platform code uses roles only. A token may not depend on both axes.

import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { flatten, resolve } from './dtcg.mjs';
import { DESIGN_DIR } from './icons.mjs';

export const TOKENS_DIR = path.join(DESIGN_DIR, 'tokens');

async function readTree(dir, rel) {
  const errors = [];
  const m = flatten(JSON.parse(await readFile(path.join(dir, rel), 'utf8')), rel, errors);
  if (errors.length) throw new Error(errors.join('\n'));
  return m;
}

const listTokenFiles = async (dir, sub) =>
  (await readdir(path.join(dir, sub))).filter((f) => f.endsWith('.tokens.json')).sort();

/** camelCase identifier from a token path: `bg.elevated-2` → `bgElevated2`. */
export function camelName(p) {
  const id = p
    .split(/[.-]/)
    .map((s, i) => (i === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)))
    .join('');
  return /^[0-9]/.test(id) ? `t${id}` : id;
}

/**
 * @typedef {{p: string, type: string, ext?: object}} TokenInfo
 * @typedef {{themes: string[], densities: string[],
 *   constant: (TokenInfo & {value: any})[],
 *   byTheme: (TokenInfo & {values: Record<string, any>})[],
 *   byDensity: (TokenInfo & {values: Record<string, any>})[]}} TokenModel
 */

/** Loads design/tokens (or `dir`) and classifies resolved tokens. @returns {Promise<TokenModel>} */
export async function buildModel(dir = TOKENS_DIR) {
  const primitive = await readTree(dir, 'primitive.tokens.json');
  const semantic = await readTree(dir, 'semantic.tokens.json');
  const component = await readTree(dir, 'component.tokens.json');
  // Dark is the default theme and lives in semantic.tokens.json; themes/* override it.
  const themes = { dark: new Map() };
  for (const f of await listTokenFiles(dir, 'themes')) themes[f.replace('.tokens.json', '')] = await readTree(dir, `themes/${f}`);
  const densities = {};
  for (const f of await listTokenFiles(dir, 'density')) densities[f.replace('.tokens.json', '')] = await readTree(dir, `density/${f}`);
  const tn = Object.keys(themes);
  const dn = Object.keys(densities);

  const sets = {};
  for (const t of tn) for (const d of dn) sets[`${t}/${d}`] = new Map([...primitive, ...semantic, ...themes[t], ...densities[d], ...component]);
  const paths = [...new Set([...semantic.keys(), ...densities[dn[0]].keys(), ...component.keys()])];

  const model = { themes: tn, densities: dn, constant: [], byTheme: [], byDensity: [] };
  for (const p of paths) {
    const set0 = sets[`${tn[0]}/${dn[0]}`];
    const tok = set0.get(p);
    const val = (t, d) => resolve(sets[`${t}/${d}`], p);
    const ext = (t, d) => sets[`${t}/${d}`].get(p).extensions;
    const key = (t, d) => JSON.stringify([val(t, d), ext(t, d) ?? null]);
    const themeVar = tn.some((t) => key(t, dn[0]) !== key(tn[0], dn[0]));
    const densVar = dn.some((d) => key(tn[0], d) !== key(tn[0], dn[0]));
    if (themeVar && densVar) throw new Error(`${p} depends on both theme and density; split the token`);
    const info = { p, type: tok.type, ext: tok.extensions };
    if (themeVar) model.byTheme.push({ ...info, values: Object.fromEntries(tn.map((t) => [t, val(t, dn[0])])) });
    else if (densVar) {
      model.byDensity.push({
        ...info,
        values: Object.fromEntries(dn.map((d) => [d, val(tn[0], d)])),
        exts: Object.fromEntries(dn.map((d) => [d, ext(tn[0], d)])),
      });
    } else model.constant.push({ ...info, value: val(tn[0], dn[0]) });
  }
  return model;
}

/** Throws if two token paths map to the same identifier or an identifier is reserved. */
export function checkNames(names, reserved = new Set(), label = 'identifier') {
  const seen = new Map();
  for (const [p, n] of names) {
    if (reserved.has(n)) throw new Error(`${p}: ${label} "${n}" is a reserved word`);
    if (seen.has(n)) throw new Error(`${label} clash: ${seen.get(n)} and ${p} both map to "${n}"`);
    seen.set(n, p);
  }
}

// ---- unit helpers shared by emitters ----

/** Dimension in px (rem → 16 px). */
export const toPx = (d) => (d.unit === 'rem' ? d.value * 16 : d.value);
/** Duration in ms. */
export const toMs = (d) => Math.round(d.unit === 's' ? d.value * 1000 : d.value);
/** 8-bit sRGB channels and alpha 0–255. */
export const rgba8 = (c) => [...c.components.map((x) => Math.round(x * 255)), Math.round((c.alpha ?? 1) * 255)];
/** Number formatted with at most 4 decimals. */
export const fmt = (n) => String(Math.round(n * 10000) / 10000);
/** First family of a fontFamily value. */
export const firstFamily = (v) => (Array.isArray(v) ? v[0] : v);
/** Font features of a token (`$extensions["org.project-alpha"].fontFeatures`). */
export const fontFeatures = (ext) => ext?.['org.project-alpha']?.fontFeatures ?? [];
/** Shadow as a list of layers. */
export const shadowLayers = (v) => (Array.isArray(v) ? v : [v]);
