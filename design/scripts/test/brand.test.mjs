// SPDX-License-Identifier: MPL-2.0
// Unit tests for the brand asset generator (DSN-006): `npm test` in design/.

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { test } from 'node:test';
import { APP_ID, buildAll, ico, png } from '../brand-build.mjs';

const pngSize = (b) => [b.readUInt32BE(16), b.readUInt32BE(20)];
const find = (out, suffix) => {
  const hit = [...out.keys()].find((p) => p.split(path.sep).join('/').endsWith(suffix));
  assert.ok(hit, `missing ${suffix}`);
  return out.get(hit);
};

const out = await buildAll();

test('Android launcher icons have the right pixel sizes per density', () => {
  for (const [dpi, k] of Object.entries({ mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 })) {
    assert.deepEqual(pngSize(find(out, `mipmap-${dpi}/ic_launcher.png`)), [48 * k, 48 * k]);
    for (const layer of ['foreground', 'background', 'monochrome']) {
      assert.deepEqual(pngSize(find(out, `mipmap-${dpi}/ic_launcher_${layer}.png`)), [108 * k, 108 * k]);
    }
  }
  assert.match(find(out, 'mipmap-anydpi-v26/ic_launcher.xml'), /<monochrome android:drawable="@mipmap\/ic_launcher_monochrome"\/>/);
});

test('Android TV banner is 320×180 and Play Store icon 512', () => {
  assert.deepEqual(pngSize(find(out, 'drawable-xhdpi/tv_banner.png')), [320, 180]);
  assert.deepEqual(pngSize(find(out, 'android/play-store-512.png')), [512, 512]);
});

test('Linux hicolor icons and .desktop use the application id', () => {
  for (const s of [16, 22, 24, 32, 48, 64, 128, 256, 512]) {
    assert.deepEqual(pngSize(find(out, `hicolor/${s}x${s}/apps/${APP_ID}.png`)), [s, s]);
  }
  find(out, `hicolor/scalable/apps/${APP_ID}.svg`);
  const desktop = find(out, `linux/${APP_ID}.desktop`);
  assert.match(desktop, new RegExp(`^Icon=${APP_ID.replace('.', '\\.')}$`, 'm'));
  assert.match(desktop, /^Categories=AudioVideo;/m);
});

test('ICO container lists every embedded PNG', () => {
  const a = png('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1"/></svg>', 16);
  const b = ico([{ size: 16, data: a }, { size: 256, data: a }]);
  assert.equal(b.readUInt16LE(2), 1);
  assert.equal(b.readUInt16LE(4), 2);
  assert.equal(b.readUInt8(6 + 16), 0); // 256 is stored as 0
  assert.equal(b.readUInt32LE(6 + 12), 6 + 32);
  const fav = find(out, 'web/favicon.ico');
  assert.equal(fav.readUInt16LE(4), 3);
});

test('placeholders keep their aspect ratios', () => {
  assert.deepEqual(pngSize(find(out, 'export/poster.png')), [400, 600]);
  assert.deepEqual(pngSize(find(out, 'export/backdrop.png')), [1280, 720]);
  assert.deepEqual(pngSize(find(out, 'export/album.png')), [512, 512]);
});

test('committed exports are up to date (same as brand:check)', async () => {
  for (const [p, data] of out) {
    const cur = await readFile(p);
    assert.ok(cur.equals(Buffer.from(data)), `stale ${p}; run \`just design brand\``);
  }
});
