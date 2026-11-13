// SPDX-License-Identifier: MPL-2.0
// Design token code generation (DSN-003, design/decisions/NNNN-token-codegen.md):
// design/tokens (W3C DTCG) → Dart, Swift, Kotlin and CSS. No dependencies; the output
// depends only on the JSON and this repository, so it is reproducible byte for byte.
//   node scripts/tokens-build.mjs          write all outputs
//   node scripts/tokens-build.mjs --check  fail if any committed output is stale (CI gate)

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DESIGN_DIR, REPO_DIR } from './lib/icons.mjs';
import { buildModel, TOKENS_DIR } from './lib/tokens-model.mjs';
import { emitDart } from './lib/emit-dart.mjs';
import { emitSwift } from './lib/emit-swift.mjs';
import { emitKotlin } from './lib/emit-kotlin.mjs';
import { emitCss } from './lib/emit-css.mjs';

const GEN = path.join(DESIGN_DIR, 'tokens', 'generated');

/** Output files and their emitters. */
export const TARGETS = [
  { lang: 'Dart', file: path.join(DESIGN_DIR, 'flutter', 'pa_design', 'lib', 'src', 'tokens.g.dart'), emit: emitDart },
  { lang: 'Swift', file: path.join(GEN, 'apple', 'Tokens.swift'), emit: emitSwift },
  { lang: 'Kotlin', file: path.join(GEN, 'android', 'PaTokens.kt'), emit: emitKotlin },
  { lang: 'CSS', file: path.join(GEN, 'web', 'tokens.css'), emit: emitCss },
];

/** Generates every target from `dir`. @returns {Promise<{lang, file, content}[]>} */
export async function generateAll(dir = TOKENS_DIR) {
  const model = await buildModel(dir);
  return TARGETS.map((t) => ({ lang: t.lang, file: t.file, content: t.emit(model) }));
}

/** Targets whose file on disk differs from the generated content. */
export async function staleTargets(outputs) {
  const stale = [];
  for (const o of outputs) {
    const cur = await readFile(o.file, 'utf8').catch(() => null);
    if (cur !== o.content) stale.push(o);
  }
  return stale;
}

async function main() {
  const outputs = await generateAll();
  if (process.argv.includes('--check')) {
    const stale = await staleTargets(outputs);
    if (stale.length) {
      for (const o of stale) console.error(`stale: ${path.relative(REPO_DIR, o.file)}`);
      console.error('tokens:check failed: design/tokens changed without regenerating code; run `just design tokens` and commit the result');
      process.exit(1);
    }
    console.log(`tokens:check OK: ${outputs.map((o) => o.lang).join(', ')} tokens up to date`);
    return;
  }
  for (const o of outputs) {
    await mkdir(path.dirname(o.file), { recursive: true });
    await writeFile(o.file, o.content);
    console.log(`wrote ${path.relative(REPO_DIR, o.file)}`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((e) => { console.error(e.message); process.exit(1); });
