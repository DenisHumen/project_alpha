// SPDX-License-Identifier: MPL-2.0
// Downloads pinned fonts from fonts/fonts.lock.json, verifies SHA-256 of the
// archive and every extracted file, writes them to fonts/files/ (git-ignored).
//
// Usage:
//   node scripts/fonts-fetch.mjs                  # download + verify into fonts/files/ and flutter/pa_design/fonts/
//   node scripts/fonts-fetch.mjs --archive <zip>  # use an already downloaded archive (offline)
//   node scripts/fonts-fetch.mjs --dest <dir>     # additionally copy the files to <dir>
//                                                 #   (e.g. clients/flutter/assets/fonts)
// No dependencies: ZIP is read with node:zlib.

import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { inflateRawSync } from 'node:zlib';
import { DESIGN_DIR } from './lib/icons.mjs';

const OUT_DIR = path.join(DESIGN_DIR, 'fonts', 'files');
const PKG_FONTS = path.join(DESIGN_DIR, 'flutter', 'pa_design', 'fonts');
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

/** Extracts one entry from a ZIP buffer (stored or deflated). */
export function unzipEntry(zip, name) {
  // End of central directory record: search backwards for its signature.
  let eocd = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 65557); i--) {
    if (zip.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error('not a ZIP archive');
  const count = zip.readUInt16LE(eocd + 10);
  let p = zip.readUInt32LE(eocd + 16);
  for (let n = 0; n < count; n++) {
    if (zip.readUInt32LE(p) !== 0x02014b50) throw new Error('corrupt central directory');
    const method = zip.readUInt16LE(p + 10);
    const csize = zip.readUInt32LE(p + 20);
    const nameLen = zip.readUInt16LE(p + 28);
    const extraLen = zip.readUInt16LE(p + 30);
    const commentLen = zip.readUInt16LE(p + 32);
    const local = zip.readUInt32LE(p + 42);
    const entry = zip.toString('utf8', p + 46, p + 46 + nameLen);
    if (entry === name) {
      const dataStart = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
      const data = zip.subarray(dataStart, dataStart + csize);
      if (method === 0) return Buffer.from(data);
      if (method === 8) return inflateRawSync(data);
      throw new Error(`${name}: unsupported compression method ${method}`);
    }
    p += 46 + nameLen + extraLen + commentLen;
  }
  throw new Error(`${name}: not found in archive`);
}

async function main() {
  const args = process.argv.slice(2);
  const opt = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : undefined);
  const lock = JSON.parse(await readFile(path.join(DESIGN_DIR, 'fonts', 'fonts.lock.json'), 'utf8'));
  await mkdir(OUT_DIR, { recursive: true });
  await mkdir(PKG_FONTS, { recursive: true });
  const dest = opt('--dest') && path.resolve(opt('--dest'));
  if (dest) await mkdir(dest, { recursive: true });

  for (const font of lock.fonts) {
    let zip;
    if (opt('--archive')) zip = await readFile(opt('--archive'));
    else {
      console.log(`downloading ${font.archive.url}`);
      const res = await fetch(font.archive.url, { redirect: 'follow' });
      if (!res.ok) throw new Error(`${font.archive.url}: HTTP ${res.status}`);
      zip = Buffer.from(await res.arrayBuffer());
    }
    const got = sha256(zip);
    if (got !== font.archive.sha256) throw new Error(`${font.family}: archive SHA-256 ${got} != ${font.archive.sha256}`);

    for (const f of font.files) {
      const data = unzipEntry(zip, f.from);
      const h = sha256(data);
      if (h !== f.sha256) throw new Error(`${f.from}: SHA-256 ${h} != ${f.sha256}`);
      await writeFile(path.join(OUT_DIR, f.to), data);
      if (dest) await copyFile(path.join(OUT_DIR, f.to), path.join(dest, f.to));
      // The Flutter package bundles the TTFs (+ OFL text) from its own fonts/ dir.
      if (!f.to.endsWith('.woff2')) await copyFile(path.join(OUT_DIR, f.to), path.join(PKG_FONTS, f.to));
      console.log(`ok  ${f.to}  ${h.slice(0, 12)}…`);
    }
  }
  console.log(`fonts in ${path.relative(process.cwd(), OUT_DIR) || OUT_DIR}${dest ? ` and ${dest}` : ''}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(`fonts:fetch failed: ${e.message}`);
    process.exit(1);
  });
}
