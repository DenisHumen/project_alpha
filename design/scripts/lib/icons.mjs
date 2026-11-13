// SPDX-License-Identifier: MPL-2.0
// Shared helpers for the icon pipeline (DSN-004).

import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const DESIGN_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const REPO_DIR = path.resolve(DESIGN_DIR, '..');
/** "SPDX-License-Identifier", split so REUSE does not read code as a header. */
export const SPDX_TAG = ['SPDX', 'License', 'Identifier'].join('-');
export const NAME_RE =/^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;

export async function loadManifest() {
  return JSON.parse(await readFile(path.join(DESIGN_DIR, 'icons', 'manifest.json'), 'utf8'));
}

/** `skip-forward` -> `skipForward`; Dart/Swift reserved words get a suffix. */
export function camelCase(name) {
  const id = name.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
  const reserved = new Set(['default', 'switch', 'case', 'repeat', 'import', 'in', 'is', 'class', 'enum']);
  return reserved.has(id) ? `${id}Icon` : id;
}
