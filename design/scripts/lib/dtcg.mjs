// SPDX-License-Identifier: MPL-2.0
// Minimal W3C DTCG (Design Tokens Format Module 2025.10) validator and resolver.
// No dependencies on purpose: CI reads tokens without installing anything.

export const TYPES = new Set([
  'color', 'dimension', 'fontFamily', 'fontWeight', 'duration', 'cubicBezier', 'number',
  'strokeStyle', 'border', 'transition', 'shadow', 'gradient', 'typography',
]);
const RESERVED = new Set(['$value', '$type', '$description', '$extensions', '$deprecated', '$schema']);
const REF_RE = /^\{([^{}]+)\}$/;
const FONT_WEIGHT_NAMES = new Set([
  'thin', 'hairline', 'extra-light', 'ultra-light', 'light', 'normal', 'regular', 'book', 'medium',
  'semi-bold', 'demi-bold', 'bold', 'extra-bold', 'ultra-bold', 'black', 'heavy', 'extra-black', 'ultra-black',
]);

export const isRef = (v) => typeof v === 'string' && REF_RE.test(v);
export const refPath = (v) => v.match(REF_RE)[1];
const isObj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);

/**
 * Flattens a token tree into Map<path, {type, value, file}> and collects structural errors.
 * `$type` is inherited from the closest ancestor group (DTCG §6.3).
 */
export function flatten(tree, file, errors, into = new Map()) {
  const walk = (node, path, inheritedType) => {
    for (const k of Object.keys(node)) {
      if (k.startsWith('$') && !RESERVED.has(k)) errors.push(`${file}: ${[...path, k].join('.')}: unknown reserved key`);
      if (!k.startsWith('$') && /[{}.]/.test(k)) errors.push(`${file}: ${[...path, k].join('.')}: names must not contain { } or .`);
    }
    const type = node.$type ?? inheritedType;
    if (node.$type !== undefined && !TYPES.has(node.$type)) errors.push(`${file}: ${path.join('.') || '<root>'}: unknown $type "${node.$type}"`);
    if ('$value' in node) {
      const p = path.join('.');
      const children = Object.keys(node).filter((k) => !k.startsWith('$'));
      if (children.length) errors.push(`${file}: ${p}: a token must not contain child tokens/groups (${children.join(', ')})`);
      into.set(p, { type, value: node.$value, file, declaredType: node.$type, extensions: node.$extensions });
      return;
    }
    for (const [k, v] of Object.entries(node)) {
      if (k.startsWith('$')) continue;
      if (!isObj(v)) {
        errors.push(`${file}: ${[...path, k].join('.')}: expected a token or group object`);
        continue;
      }
      walk(v, [...path, k], type);
    }
  };
  walk(tree, [], undefined);
  return into;
}

const num = (v) => typeof v === 'number' && Number.isFinite(v);

/** Validates a literal (non-reference) value against its type. Returns a message or null. */
export function checkValue(type, v, sub = (t, x) => checkValue(t, x)) {
  const field = (t, x) => (isRef(x) ? null : sub(t, x));
  switch (type) {
    case 'color': {
      if (!isObj(v)) return 'color must be an object {colorSpace, components, alpha?, hex?}';
      if (v.colorSpace !== 'srgb') return `colorSpace "${v.colorSpace}" is not supported here (use srgb)`;
      if (!Array.isArray(v.components) || v.components.length !== 3 || !v.components.every((c) => num(c) && c >= 0 && c <= 1))
        return 'components must be 3 numbers in [0, 1]';
      if (v.alpha !== undefined && !(num(v.alpha) && v.alpha >= 0 && v.alpha <= 1)) return 'alpha must be in [0, 1]';
      if (v.hex !== undefined) {
        if (!/^#[0-9a-f]{6}$/.test(v.hex)) return `hex "${v.hex}" must be #rrggbb lower-case`;
        const fromHex = [1, 3, 5].map((i) => parseInt(v.hex.slice(i, i + 2), 16) / 255);
        if (fromHex.some((c, i) => Math.abs(c - v.components[i]) > 0.003)) return `hex ${v.hex} does not match components`;
      }
      return null;
    }
    case 'dimension':
      return isObj(v) && num(v.value) && (v.unit === 'px' || v.unit === 'rem') ? null : 'dimension must be {value: number, unit: "px"|"rem"}';
    case 'duration':
      return isObj(v) && num(v.value) && v.value >= 0 && (v.unit === 'ms' || v.unit === 's') ? null : 'duration must be {value: number ≥ 0, unit: "ms"|"s"}';
    case 'number':
      return num(v) ? null : 'number expected';
    case 'fontWeight':
      return (num(v) && v >= 1 && v <= 1000) || FONT_WEIGHT_NAMES.has(v) ? null : 'fontWeight must be 1–1000 or a predefined name';
    case 'fontFamily':
      return typeof v === 'string' || (Array.isArray(v) && v.length && v.every((s) => typeof s === 'string')) ? null : 'fontFamily must be a string or string[]';
    case 'cubicBezier':
      return Array.isArray(v) && v.length === 4 && v.every(num) && v[0] >= 0 && v[0] <= 1 && v[2] >= 0 && v[2] <= 1
        ? null
        : 'cubicBezier must be [x1, y1, x2, y2] with x in [0, 1]';
    case 'shadow': {
      const one = (s) => {
        if (!isObj(s)) return 'shadow must be an object or array of objects';
        for (const k of ['color', 'offsetX', 'offsetY', 'blur']) if (!(k in s)) return `shadow is missing ${k}`;
        return field('color', s.color) || field('dimension', s.offsetX) || field('dimension', s.offsetY) || field('dimension', s.blur) ||
          (s.spread !== undefined ? field('dimension', s.spread) : null);
      };
      return Array.isArray(v) ? v.map(one).find(Boolean) ?? null : one(v);
    }
    case 'gradient':
      if (!Array.isArray(v) || v.length < 2) return 'gradient must be an array of ≥ 2 stops';
      for (const s of v) {
        if (!isObj(s) || !('color' in s) || !('position' in s)) return 'gradient stop must be {color, position}';
        const e = field('color', s.color) || (isRef(s.position) ? null : num(s.position) && s.position >= 0 && s.position <= 1 ? null : 'position must be in [0, 1]');
        if (e) return e;
      }
      return null;
    case 'transition':
      if (!isObj(v)) return 'transition must be {duration, delay, timingFunction}';
      for (const k of ['duration', 'delay', 'timingFunction']) if (!(k in v)) return `transition is missing ${k}`;
      return field('duration', v.duration) || field('duration', v.delay) || field('cubicBezier', v.timingFunction);
    case 'typography':
      if (!isObj(v)) return 'typography must be an object';
      for (const k of ['fontFamily', 'fontSize', 'fontWeight', 'letterSpacing', 'lineHeight']) if (!(k in v)) return `typography is missing ${k}`;
      return field('fontFamily', v.fontFamily) || field('dimension', v.fontSize) || field('fontWeight', v.fontWeight) ||
        field('dimension', v.letterSpacing) || field('number', v.lineHeight);
    case 'border':
      if (!isObj(v)) return 'border must be an object';
      return field('color', v.color) || field('dimension', v.width);
    case 'strokeStyle':
      return typeof v === 'string' || isObj(v) ? null : 'strokeStyle must be a string or object';
    default:
      return `unknown type ${type}`;
  }
}

/** Sub-field types of composite values, used to type-check references inside them. */
const FIELD_TYPES = {
  shadow: { color: 'color', offsetX: 'dimension', offsetY: 'dimension', blur: 'dimension', spread: 'dimension' },
  transition: { duration: 'duration', delay: 'duration', timingFunction: 'cubicBezier' },
  typography: { fontFamily: 'fontFamily', fontSize: 'dimension', fontWeight: 'fontWeight', letterSpacing: 'dimension', lineHeight: 'number' },
  border: { color: 'color', width: 'dimension', style: 'strokeStyle' },
  gradient: { color: 'color', position: 'number' },
};

/** Yields [expectedType, refString] for every reference inside a value. */
function* refsIn(type, value) {
  if (isRef(value)) {
    yield [type, value];
    return;
  }
  const fields = FIELD_TYPES[type];
  if (!fields) return;
  const items = Array.isArray(value) ? value : [value];
  for (const item of items) {
    if (!isObj(item)) continue;
    for (const [k, t] of Object.entries(fields)) if (isRef(item[k])) yield [t, item[k]];
  }
}

/**
 * Validates a merged token set: types, literal values, references (exist, typed, acyclic).
 * Returns a list of error strings.
 */
export function validateSet(tokens, label) {
  const errors = [];
  for (const [p, t] of tokens) {
    if (!t.type) {
      errors.push(`${label}: ${p} (${t.file}): no $type (set it on the token or a parent group)`);
      continue;
    }
    if (!TYPES.has(t.type)) continue; // reported by flatten
    if (!isRef(t.value)) {
      const e = checkValue(t.type, t.value);
      if (e) errors.push(`${label}: ${p} (${t.file}): ${e}`);
    }
    for (const [expected, r] of refsIn(t.type, t.value)) {
      const target = tokens.get(refPath(r));
      if (!target) errors.push(`${label}: ${p} (${t.file}): reference ${r} does not resolve`);
      else if (target.type !== expected) errors.push(`${label}: ${p} (${t.file}): ${r} is ${target.type}, expected ${expected}`);
    }
  }
  // Cycles.
  for (const p of tokens.keys()) {
    const seen = [p];
    let cur = tokens.get(p);
    while (cur && isRef(cur.value)) {
      const next = refPath(cur.value);
      if (seen.includes(next)) {
        errors.push(`${label}: reference cycle ${[...seen, next].join(' -> ')}`);
        break;
      }
      seen.push(next);
      cur = tokens.get(next);
    }
  }
  return errors;
}

/** Fully resolves a token value (references replaced recursively). */
export function resolve(tokens, p, depth = 0) {
  if (depth > 32) throw new Error(`reference depth exceeded at ${p}`);
  const t = tokens.get(p);
  if (!t) throw new Error(`unknown token ${p}`);
  const deep = (v) => {
    if (isRef(v)) return resolve(tokens, refPath(v), depth + 1);
    if (Array.isArray(v)) return v.map(deep);
    if (isObj(v)) return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, deep(x)]));
    return v;
  };
  return deep(t.value);
}

// ---- WCAG 2.x contrast ----

const lin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
export const luminance = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);

/** Source-over compositing of colour {components, alpha} onto an opaque rgb triple. */
export function over(color, base) {
  const a = color.alpha ?? 1;
  return color.components.map((c, i) => c * a + base[i] * (1 - a));
}

export function contrast(rgbA, rgbB) {
  const [hi, lo] = [luminance(rgbA), luminance(rgbB)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

export function hexToRgb(hex) {
  return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
}
