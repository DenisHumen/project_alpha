// SPDX-License-Identifier: MPL-2.0
// Interim Dart export of design tokens for the release 0.1 Flutter app
// (docs/dev/release-0.1-contract.md). Writes
//   design/flutter/pa_design/lib/src/tokens.g.dart
// DSN-003 (Style Dictionary) may replace this generator; the Dart API should stay.
//
// Every semantic/component/density token is resolved for every theme × density.
// Tokens that differ by theme go to PaColorScheme (dark/light), tokens that
// differ by density go to PaDensity (tv/desktop/tablet/phone), the rest are
// constants on PaTokens. Primitives are not exported (platform code must use roles).
// `--check` fails if the file on disk is stale.

import { readFile, readdir, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { flatten, resolve } from './lib/dtcg.mjs';
import { DESIGN_DIR, REPO_DIR } from './lib/icons.mjs';

const T = path.join(DESIGN_DIR, 'tokens');
export const DART_TOKENS = path.join(DESIGN_DIR, 'flutter', 'pa_design', 'lib', 'src', 'tokens.g.dart');
const PACKAGE = 'pa_design';

const load = async (rel) => {
  const errors = [];
  const m = flatten(JSON.parse(await readFile(path.join(T, rel), 'utf8')), rel, errors);
  if (errors.length) throw new Error(errors.join('\n'));
  return m;
};
const files = async (sub) => (await readdir(path.join(T, sub))).filter((f) => f.endsWith('.tokens.json')).sort();

export function dartName(p) {
  const id = p
    .split(/[.-]/)
    .map((s, i) => (i === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)))
    .join('');
  return /^[0-9]/.test(id) ? `t${id}` : id;
}

const dbl = (n) => {
  const s = String(Math.round(n * 10000) / 10000);
  return s.includes('.') || s.includes('e') ? s : `${s}.0`;
};
const color = (c) => {
  const a = Math.round((c.alpha ?? 1) * 255);
  const [r, g, b] = c.components.map((x) => Math.round(x * 255));
  return `Color(0x${[a, r, g, b].map((x) => x.toString(16).padStart(2, '0')).join('').toUpperCase()})`;
};
const ms = (d) => `Duration(milliseconds: ${Math.round(d.unit === 's' ? d.value * 1000 : d.value)})`;
const px = (d) => dbl(d.unit === 'rem' ? d.value * 16 : d.value);

const DART_TYPE = {
  color: 'Color', dimension: 'double', number: 'double', duration: 'Duration', cubicBezier: 'Cubic',
  shadow: 'BoxShadow', gradient: 'LinearGradient', transition: 'PaTransition', typography: 'TextStyle',
  fontWeight: 'FontWeight', fontFamily: 'String',
};

function dartValue(type, v, ext) {
  switch (type) {
    case 'color': return color(v);
    case 'dimension': return px(v);
    case 'number': return dbl(v);
    case 'duration': return ms(v);
    case 'cubicBezier': return `Cubic(${v.map(dbl).join(', ')})`;
    case 'fontWeight': return `FontWeight.w${Math.round(v / 100) * 100}`;
    case 'fontFamily': return `'${Array.isArray(v) ? v[0] : v}'`;
    case 'shadow': {
      const s = Array.isArray(v) ? v[0] : v;
      return `BoxShadow(color: ${color(s.color)}, offset: Offset(${px(s.offsetX)}, ${px(s.offsetY)}), blurRadius: ${px(s.blur)}, spreadRadius: ${px(s.spread ?? { value: 0, unit: 'px' })})`;
    }
    case 'gradient':
      // Position 0 is the edge that carries text (bottom of a backdrop).
      return `LinearGradient(begin: Alignment.bottomCenter, end: Alignment.topCenter, colors: [${v.map((s) => color(s.color)).join(', ')}], stops: [${v.map((s) => dbl(s.position)).join(', ')}])`;
    case 'transition':
      return `PaTransition(duration: ${ms(v.duration)}, delay: ${ms(v.delay)}, curve: Cubic(${v.timingFunction.map(dbl).join(', ')}))`;
    case 'typography': {
      const size = v.fontSize.unit === 'rem' ? v.fontSize.value * 16 : v.fontSize.value;
      const family = Array.isArray(v.fontFamily) ? v.fontFamily[0] : v.fontFamily;
      const feats = ext?.['org.project-alpha']?.fontFeatures ?? [];
      const ff = feats.length ? `, fontFeatures: [${feats.map((f) => (f === 'tnum' ? 'FontFeature.tabularFigures()' : `FontFeature('${f}')`)).join(', ')}]` : '';
      // Inter has an optical-size axis 14–32: follow the font size.
      const opsz = Math.min(32, Math.max(14, size));
      return `TextStyle(fontFamily: '${family}', package: '${PACKAGE}', fontSize: ${dbl(size)}, fontWeight: FontWeight.w${Math.round(v.fontWeight / 100) * 100}, letterSpacing: ${px(v.letterSpacing)}, height: ${dbl(v.lineHeight)}${ff}, fontVariations: [FontVariation('opsz', ${dbl(opsz)})])`;
    }
    default:
      throw new Error(`no Dart mapping for $type ${type}`);
  }
}

export async function generate() {
  const primitive = await load('primitive.tokens.json');
  const semantic = await load('semantic.tokens.json');
  const component = await load('component.tokens.json');
  const themes = { dark: new Map() };
  for (const f of await files('themes')) themes[f.replace('.tokens.json', '')] = await load(`themes/${f}`);
  const densities = {};
  for (const f of await files('density')) densities[f.replace('.tokens.json', '')] = await load(`density/${f}`);
  const dn = Object.keys(densities);
  const tn = Object.keys(themes);

  const sets = {};
  for (const t of tn) for (const d of dn) sets[`${t}/${d}`] = new Map([...primitive, ...semantic, ...themes[t], ...densities[d], ...component]);
  const any = sets[`${tn[0]}/${dn[0]}`];
  const paths = [...new Set([...semantic.keys(), ...densities[dn[0]].keys(), ...component.keys()])];

  const constant = [], byTheme = [], byDensity = [];
  const names = new Set();
  for (const p of paths) {
    const tok = any.get(p);
    if (!DART_TYPE[tok.type]) throw new Error(`${p}: unsupported type ${tok.type}`);
    const name = dartName(p);
    if (names.has(name)) throw new Error(`Dart name clash: ${name}`);
    names.add(name);
    const val = (t, d) => {
      const s = sets[`${t}/${d}`];
      return dartValue(tok.type, resolve(s, p), s.get(p).extensions);
    };
    const themeVar = tn.some((t) => val(t, dn[0]) !== val(tn[0], dn[0]));
    const densVar = dn.some((d) => val(tn[0], d) !== val(tn[0], dn[0]));
    if (themeVar && densVar) throw new Error(`${p} depends on both theme and density; split the token`);
    const desc = tok.file && (semantic.get(p) ?? component.get(p) ?? densities[dn[0]].get(p)) ? p : p;
    const entry = { p, name, type: DART_TYPE[tok.type], desc };
    if (themeVar) byTheme.push({ ...entry, values: Object.fromEntries(tn.map((t) => [t, val(t, dn[0])])) });
    else if (densVar) byDensity.push({ ...entry, values: Object.fromEntries(dn.map((d) => [d, val(tn[0], d)])) });
    else constant.push({ ...entry, value: val(tn[0], dn[0]) });
  }

  const cls = (className, doc, list, variants) => {
    const fields = list.map((e) => `  /// \`${e.p}\`\n  final ${e.type} ${e.name};`).join('\n');
    const ctor = `  const ${className}._({\n${list.map((e) => `    required this.${e.name},`).join('\n')}\n  });`;
    const consts = variants
      .map((v) => `  static const ${v} = ${className}._(\n${list.map((e) => `    ${e.name}: ${e.values[v]},`).join('\n')}\n  );`)
      .join('\n\n');
    return `/// ${doc}\nfinal class ${className} {\n${ctor}\n\n${consts}\n\n  static const values = <String, ${className}>{${variants.map((v) => `'${v}': ${v}`).join(', ')}};\n\n${fields}\n}\n`;
  };

  return `// SPDX-License-Identifier: MPL-2.0
// GENERATED by design/scripts/tokens-dart.mjs from design/tokens (W3C DTCG). Do not edit;
// change the JSON and run \`just design tokens\`.

// ignore_for_file: constant_identifier_names, lines_longer_than_80_chars

import 'dart:ui' show FontFeature, FontVariation;

import 'package:flutter/widgets.dart';

/// Transition token (DTCG \`transition\`).
final class PaTransition {
  const PaTransition({required this.duration, required this.delay, required this.curve});
  final Duration duration;
  final Duration delay;
  final Curve curve;
}

/// Theme-independent, density-independent tokens.
abstract final class PaTokens {
${constant.map((e) => `  /// \`${e.p}\`\n  static const ${e.type} ${e.name} = ${e.value};`).join('\n')}
}

${cls('PaColorScheme', 'Tokens that depend on the colour theme. Dark is the default (01 §4.1); TV is always dark.', byTheme, tn)}
${cls('PaDensity', 'Tokens that depend on the density profile: tv (10-foot UI, 1920×1080 logical px), desktop, tablet, phone.', byDensity, dn)}`;
}

async function main() {
  const out = await generate();
  const cur = await readFile(DART_TOKENS, 'utf8').catch(() => null);
  if (process.argv.includes('--check')) {
    if (cur !== out) {
      console.error(`stale: ${path.relative(REPO_DIR, DART_TOKENS)}\ntokens:check failed: run \`just design tokens\` and commit the result`);
      process.exit(1);
    }
    console.log('tokens:check OK: Dart tokens up to date');
    return;
  }
  await mkdir(path.dirname(DART_TOKENS), { recursive: true });
  await writeFile(DART_TOKENS, out);
  console.log(`wrote ${path.relative(REPO_DIR, DART_TOKENS)}`);
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((e) => { console.error(e.message); process.exit(1); });
