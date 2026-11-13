// SPDX-License-Identifier: MPL-2.0
// Unit tests for the design pipeline: `npm test` in design/.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { deflateRawSync } from 'node:zlib';
import { checkValue, contrast, flatten, hexToRgb, over, resolve, validateSet } from '../lib/dtcg.mjs';
import { SPDX_TAG, camelCase } from '../lib/icons.mjs';
import { lintSvg } from '../icons-lint.mjs';
import { unzipEntry } from '../fonts-fetch.mjs';
import { validate } from '../validate-tokens.mjs';
import { dartName } from '../tokens-dart.mjs';

const color = (hex, alpha) => ({ colorSpace: 'srgb', components: hexToRgb(hex), hex, ...(alpha !== undefined && { alpha }) });

test('repository tokens validate', async () => {
  const { errors, pairs } = await validate();
  assert.deepEqual(errors, []);
  assert.ok(pairs > 40);
});

test('WCAG contrast: white on black is 21:1, compositing works', () => {
  assert.equal(contrast([1, 1, 1], [0, 0, 0]).toFixed(1), '21.0');
  assert.deepEqual(over(color('#ffffff', 0.5), [0, 0, 0]), [0.5, 0.5, 0.5]);
});

test('broken and mistyped references are reported', () => {
  const errors = [];
  const m = flatten(
    {
      a: { $type: 'color', $value: color('#000000') },
      b: { $type: 'color', $value: '{missing.token}' },
      c: { $type: 'dimension', $value: '{a}' },
      d: { $type: 'number', $value: '{e}' },
      e: { $type: 'number', $value: '{d}' },
    },
    't',
    errors,
  );
  const out = validateSet(m, 'x').join('\n');
  assert.match(out, /\{missing\.token\} does not resolve/);
  assert.match(out, /\{a\} is color, expected dimension/);
  assert.match(out, /reference cycle/);
});

test('structure errors: token with children, untyped token, bad values', () => {
  const errors = [];
  const m = flatten({ g: { $value: 1, child: { $value: 2 } }, u: { $value: 'x' }, $bogus: 1 }, 't', errors);
  assert.ok(errors.some((e) => /must not contain child/.test(e)));
  assert.ok(errors.some((e) => /unknown reserved key/.test(e)));
  assert.ok(validateSet(m, 'x').some((e) => /no \$type/.test(e)));
  assert.match(checkValue('color', { colorSpace: 'srgb', components: [1, 0, 0], hex: '#00ff00' }), /does not match/);
  assert.match(checkValue('dimension', { value: 4, unit: 'pt' }), /dimension/);
  assert.equal(checkValue('cubicBezier', [0.2, 0.8, 0.2, 1]), null);
});

test('resolve follows nested references inside composites', () => {
  const m = flatten(
    {
      c: { $type: 'color', $value: color('#000000', 0.5) },
      d: { $type: 'dimension', $value: { value: 4, unit: 'px' } },
      s: { $type: 'shadow', $value: { color: '{c}', offsetX: '{d}', offsetY: '{d}', blur: '{d}' } },
    },
    't',
    [],
  );
  assert.equal(resolve(m, 's').color.alpha, 0.5);
  assert.equal(resolve(m, 's').blur.value, 4);
});

test('icon linter accepts the house style and rejects violations', () => {
  const ok = `<!-- ${SPDX_TAG}: ISC -->` + '\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/></svg>\n';
  assert.deepEqual(lintSvg('add', ok), []);
  const bad = ok.replace('viewBox="0 0 24 24"', 'viewBox="0 0 32 32"').replace('<path d', '<path fill="#f00" d').replace('</svg>', '<image href="x.png"/></svg>');
  const errs = lintSvg('Bad_Name', bad).join('\n');
  assert.match(errs, /kebab-case/);
  assert.match(errs, /viewBox/);
  assert.match(errs, /currentColor/);
  assert.match(errs, /<image> is not allowed/);
  assert.match(lintSvg('x', ok.replace('1.75', '1')).join(), /stroke-width/);
});

test('names: kebab-case to camelCase, reserved words suffixed', () => {
  assert.equal(camelCase('skip-forward'), 'skipForward');
  assert.equal(camelCase('repeat'), 'repeatIcon');
  assert.equal(dartName('bg.elevated-2'), 'bgElevated2');
  assert.equal(dartName('media-badge.live.bg'), 'mediaBadgeLiveBg');
});

test('unzipEntry reads deflated entries', () => {
  const name = 'a.txt';
  const data = Buffer.from('hello hello hello');
  const comp = deflateRawSync(data);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(8, 8);
  local.writeUInt32LE(comp.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(name.length, 26);
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(8, 10);
  central.writeUInt32LE(comp.length, 20);
  central.writeUInt32LE(data.length, 24);
  central.writeUInt16LE(name.length, 28);
  central.writeUInt32LE(0, 42);
  const cdStart = 30 + name.length + comp.length;
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(1, 8);
  eocd.writeUInt16LE(1, 10);
  eocd.writeUInt32LE(46 + name.length, 12);
  eocd.writeUInt32LE(cdStart, 16);
  const zip = Buffer.concat([local, Buffer.from(name), comp, central, Buffer.from(name), eocd]);
  assert.equal(unzipEntry(zip, name).toString(), data.toString());
  assert.throws(() => unzipEntry(zip, 'nope'), /not found/);
});
