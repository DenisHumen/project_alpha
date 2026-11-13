// SPDX-License-Identifier: MPL-2.0
// Token code generation and CI gates (DSN-003): `npm test` in design/.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { validate } from '../validate-tokens.mjs';
import { generateAll, staleTargets, TARGETS } from '../tokens-build.mjs';
import { buildModel, TOKENS_DIR } from '../lib/tokens-model.mjs';
import { emitSwift } from '../lib/emit-swift.mjs';
import { emitKotlin } from '../lib/emit-kotlin.mjs';
import { emitCss } from '../lib/emit-css.mjs';

/** Copies design/tokens to a temp dir, lets `edit` change one JSON file, runs `fn(dir)`. */
async function withTokens(file, edit, fn) {
  const dir = await mkdtemp(path.join(tmpdir(), 'pa-tokens-'));
  try {
    await cp(TOKENS_DIR, dir, { recursive: true });
    const f = path.join(dir, file);
    const json = JSON.parse(await readFile(f, 'utf8'));
    edit(json);
    await writeFile(f, JSON.stringify(json));
    return await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

const srgb = (hex, alpha) => ({
  colorSpace: 'srgb',
  components: [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255),
  hex,
  ...(alpha !== undefined && { alpha }),
});

test('committed token code is up to date for every platform', async () => {
  const stale = await staleTargets(await generateAll());
  assert.deepEqual(stale.map((o) => o.lang), []);
  assert.deepEqual(TARGETS.map((t) => t.lang), ['Dart', 'Swift', 'Kotlin', 'CSS']);
});

test('changing a token without regenerating makes every output stale', async () => {
  await withTokens('primitive.tokens.json', (j) => { j.color.lune['600'].$value = srgb('#00787e'); }, async (dir) => {
    const outputs = await generateAll(dir);
    const stale = await staleTargets(outputs);
    assert.deepEqual(stale.map((o) => o.lang), ['Dart', 'Swift', 'Kotlin', 'CSS']);
    const byLang = Object.fromEntries(outputs.map((o) => [o.lang, o.content]));
    assert.match(byLang.Dart, /accentDefault = Color\(0xFF00787E\)/);
    assert.match(byLang.Swift, /accentDefault: Color = Color\(paRGB: 0x00787E\)/);
    assert.match(byLang.Kotlin, /accentDefault: Int = 0xFF00787E\.toInt\(\)/);
    assert.match(byLang.CSS, /--pa-accent-default: #00787e;/);
  });
});

test('a text pair below 4.5:1 fails validation', async () => {
  await withTokens('semantic.tokens.json', (j) => { j.text.secondary.$value = srgb('#ffffff', 0.3); }, async (dir) => {
    const { errors } = await validate({ dir });
    assert.ok(errors.some((e) => /^contrast dark: text\.secondary on bg\.base = .* < 4\.5:1/.test(e)), errors.join('\n'));
  });
});

test('lowering a contrast minimum needs a note and never goes below 3:1', async () => {
  await withTokens('contrast.pairs.json', (j) => { j.pairs[0].min = 3; j.pairs[1].min = 2; }, async (dir) => {
    const errs = (await validate({ dir })).errors.join('\n');
    assert.match(errs, /text\.primary on bg\.base: min 3 < 4\.5 needs a "note"/);
    assert.match(errs, /text\.primary on bg\.elevated: min must be ≥ 3/);
  });
});

test('model splits tokens by theme and density; platforms share the shape', async () => {
  const m = await buildModel();
  assert.deepEqual(m.themes, ['dark', 'light']);
  assert.deepEqual(m.densities, ['desktop', 'phone', 'tablet', 'tv']);
  assert.ok(m.byTheme.some((e) => e.p === 'bg.base'));
  assert.ok(m.byDensity.some((e) => e.p === 'type.body'));
  assert.ok(m.constant.some((e) => e.p === 'accent.default'));
  const swift = emitSwift(m);
  const kotlin = emitKotlin(m);
  for (const e of [...m.constant, ...m.byTheme, ...m.byDensity].slice(0, 20)) {
    const name = e.p.split(/[.-]/).map((s, i) => (i ? s[0].toUpperCase() + s.slice(1) : s)).join('');
    assert.ok(swift.includes(` ${name}:`), `Swift misses ${name}`);
    assert.ok(kotlin.includes(` ${name}:`), `Kotlin misses ${name}`);
  }
  assert.match(swift, /static let tv = PaDensity\(/);
  assert.match(kotlin, /val tv = PaDensity\(/);
});

test('CSS: defaults on :root, overrides by data attributes, composite values', async () => {
  const css = emitCss(await buildModel());
  assert.match(css, /:root,\n\[data-theme="dark"\] \{\n {2}color-scheme: dark;/);
  assert.match(css, /\[data-theme="light"\] \{/);
  assert.match(css, /:root,\n\[data-density="desktop"\] \{/);
  assert.match(css, /\[data-density="tv"\] \{[^}]*--pa-type-body: 500 29px\/1\.241 "Inter", system-ui, sans-serif;/);
  assert.match(css, /--pa-type-timecode-font-feature-settings: "tnum";/);
  assert.match(css, /--pa-scrim-bottom: linear-gradient\(to top, #000000d9 0%, #00000000 100%\);/);
});

test('names that are keywords on a platform are rejected', () => {
  const model = { themes: ['dark'], densities: ['tv'], byTheme: [], byDensity: [], constant: [{ p: 'default', type: 'number', value: 1 }] };
  assert.throws(() => emitSwift(model), /reserved word/);
  model.constant[0].p = 'object';
  assert.throws(() => emitKotlin(model), /reserved word/);
});
