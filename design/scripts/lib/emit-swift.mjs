// SPDX-License-Identifier: MPL-2.0
// Swift emitter (SwiftUI, Apple clients): PaTokens, PaColorScheme.dark|light,
// PaDensity.tv|desktop|tablet|phone — the same shape as the Dart API.
// Typography maps Inter to the system font (SF): DSN-ADR-0002.

import { camelName, checkNames, fmt, fontFeatures, rgba8, toMs, toPx } from './tokens-model.mjs';

// Swift keywords that can't be used as plain identifiers.
const RESERVED = new Set(`associatedtype class deinit enum extension fileprivate func import init inout internal let open
operator private precedencegroup protocol public rethrows static struct subscript typealias var break case catch continue
default defer do else fallthrough for guard if in repeat return throw switch where while Any as await false is nil self
Self super throws true try`.split(/\s+/));

const WEIGHTS = { 100: 'ultraLight', 200: 'thin', 300: 'light', 400: 'regular', 500: 'medium', 600: 'semibold', 700: 'bold', 800: 'heavy', 900: 'black' };

const color = (c) => {
  const [r, g, b, a] = rgba8(c);
  const hex = `0x${[r, g, b].map((x) => x.toString(16).padStart(2, '0')).join('').toUpperCase()}`;
  return a === 255 ? `Color(paRGB: ${hex})` : `Color(paRGB: ${hex}, opacity: ${fmt(c.alpha)})`;
};
const sec = (d) => fmt(toMs(d) / 1000);
const px = (d) => fmt(toPx(d));
const curve = (v) => `PaCurve(x1: ${fmt(v[0])}, y1: ${fmt(v[1])}, x2: ${fmt(v[2])}, y2: ${fmt(v[3])})`;
const weight = (w) => `.${WEIGHTS[Math.round(w / 100) * 100]}`;
const shadow1 = (v) => {
  if (Array.isArray(v) && v.length > 1) throw new Error('multi-layer shadows are not supported by the generators yet');
  return Array.isArray(v) ? v[0] : v;
};

const TYPE = {
  color: 'Color', dimension: 'CGFloat', number: 'Double', duration: 'TimeInterval', cubicBezier: 'PaCurve',
  shadow: 'PaShadow', gradient: 'PaGradient', transition: 'PaTransition', typography: 'PaTypography',
  fontWeight: 'Font.Weight', fontFamily: 'String',
};

function value(type, v, ext) {
  switch (type) {
    case 'color': return color(v);
    case 'dimension': return px(v);
    case 'number': return fmt(v);
    case 'duration': return sec(v);
    case 'cubicBezier': return curve(v);
    case 'fontWeight': return weight(v);
    case 'fontFamily': return JSON.stringify(Array.isArray(v) ? v[0] : v);
    case 'shadow': {
      const s = shadow1(v);
      return `PaShadow(color: ${color(s.color)}, x: ${px(s.offsetX)}, y: ${px(s.offsetY)}, blur: ${px(s.blur)}, spread: ${px(s.spread ?? { value: 0, unit: 'px' })})`;
    }
    case 'gradient':
      return `PaGradient(stops: [${v.map((s) => `PaGradientStop(color: ${color(s.color)}, location: ${fmt(s.position)})`).join(', ')}])`;
    case 'transition':
      return `PaTransition(duration: ${sec(v.duration)}, delay: ${sec(v.delay)}, curve: ${curve(v.timingFunction)})`;
    case 'typography':
      return `PaTypography(size: ${px(v.fontSize)}, lineHeight: ${fmt(v.lineHeight)}, weight: ${weight(v.fontWeight)}, tracking: ${px(v.letterSpacing)}, tabularDigits: ${fontFeatures(ext).includes('tnum')})`;
    default:
      throw new Error(`no Swift mapping for $type ${type}`);
  }
}

const PRELUDE = `import SwiftUI

extension Color {
    /// sRGB colour from a 0xRRGGBB literal (generated tokens only).
    fileprivate init(paRGB hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }
}

/// Cubic Bézier easing (DTCG \`cubicBezier\`).
public struct PaCurve {
    public let x1: Double
    public let y1: Double
    public let x2: Double
    public let y2: Double

    public func animation(duration: TimeInterval) -> Animation {
        .timingCurve(x1, y1, x2, y2, duration: duration)
    }
}

/// Transition (DTCG \`transition\`); seconds.
public struct PaTransition {
    public let duration: TimeInterval
    public let delay: TimeInterval
    public let curve: PaCurve

    public var animation: Animation { curve.animation(duration: duration).delay(delay) }
}

/// Drop shadow (DTCG \`shadow\`); \`blur\` is the CSS blur radius, SwiftUI's \`radius\` is about half of it.
public struct PaShadow {
    public let color: Color
    public let x: CGFloat
    public let y: CGFloat
    public let blur: CGFloat
    public let spread: CGFloat
}

/// Gradient stop; location 0 is the edge that carries text (bottom of a backdrop).
public struct PaGradientStop {
    public let color: Color
    public let location: CGFloat
}

/// Linear gradient (DTCG \`gradient\`), drawn bottom → top.
public struct PaGradient {
    public let stops: [PaGradientStop]

    public var gradient: Gradient { Gradient(stops: stops.map { Gradient.Stop(color: $0.color, location: $0.location) }) }
    public var linear: LinearGradient { LinearGradient(gradient: gradient, startPoint: .bottom, endPoint: .top) }
}

/// Text style (DTCG \`typography\`). Apple platforms use the system font (SF) instead of Inter;
/// \`lineHeight\` is a multiple of \`size\`, \`tracking\` is in points.
public struct PaTypography {
    public let size: CGFloat
    public let lineHeight: Double
    public let weight: Font.Weight
    public let tracking: CGFloat
    public let tabularDigits: Bool

    public var font: Font {
        let font = Font.system(size: size, weight: weight)
        return tabularDigits ? font.monospacedDigit() : font
    }

    /// Extra space between lines for \`.lineSpacing(_:)\`.
    public var lineSpacing: CGFloat { size * (CGFloat(lineHeight) - 1) }
}
`;

/** @param {import('./tokens-model.mjs').TokenModel} model */
export function emitSwift(model) {
  const all = [...model.constant, ...model.byTheme, ...model.byDensity];
  for (const e of all) if (!TYPE[e.type]) throw new Error(`${e.p}: unsupported type ${e.type}`);
  checkNames(all.map((e) => [e.p, camelName(e.p)]), RESERVED, 'Swift name');

  const cls = (name, doc, list, variants, extOf) => {
    const consts = variants
      .map((v) => `    public static let ${v} = ${name}(\n${list.map((e) => `        ${camelName(e.p)}: ${value(e.type, e.values[v], extOf(e, v))}`).join(',\n')}\n    )`)
      .join('\n\n');
    const fields = list.map((e) => `    /// \`${e.p}\`\n    public let ${camelName(e.p)}: ${TYPE[e.type]}`).join('\n');
    return `/// ${doc}\npublic struct ${name} {\n${consts}\n\n    public static let values: [String: ${name}] = [${variants.map((v) => `"${v}": .${v}`).join(', ')}]\n\n${fields}\n}\n`;
  };

  return `// SPDX-License-Identifier: MPL-2.0
// GENERATED by design/scripts/tokens-build.mjs from design/tokens (W3C DTCG). Do not edit;
// change the JSON and run \`just design tokens\`.

${PRELUDE}
/// Theme-independent, density-independent tokens.
public enum PaTokens {
${model.constant.map((e) => `    /// \`${e.p}\`\n    public static let ${camelName(e.p)}: ${TYPE[e.type]} = ${value(e.type, e.value, e.ext)}`).join('\n')}
}

${cls('PaColorScheme', 'Tokens that depend on the colour theme. Dark is the default; TV is always dark.', model.byTheme, model.themes, (e) => e.ext)}
${cls('PaDensity', 'Tokens that depend on the density profile: tv (10-foot UI, 1920×1080 pt), desktop, tablet, phone.', model.byDensity, model.densities, (e, d) => e.exts[d])}`;
}
