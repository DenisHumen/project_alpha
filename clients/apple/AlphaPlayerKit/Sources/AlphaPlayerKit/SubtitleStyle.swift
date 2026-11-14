// SPDX-License-Identifier: MPL-2.0
// PLAY-007: the subtitle look on Apple platforms — the user's settings or the
// system caption style (MediaAccessibility) — as mpv subtitle options.

import Foundation

/// How text subtitles look. Mirrors `SubtitleAppearance` of the Flutter
/// player (pa_player): the same fields and defaults, so a setting looks the
/// same on every platform. Colors are ARGB (`0xAARRGGBB`).
public struct SubtitleStyle: Equatable, Sendable {
    public enum Font: String, CaseIterable, Sendable {
        case sansSerif, serif, monospace, casual, cursive, condensed, smallCaps
    }

    /// Edge around the glyphs.
    public enum Edge: String, CaseIterable, Sendable {
        case none, outline, dropShadow, raised, depressed
    }

    /// Size relative to the default (`textSizeFraction` of the height).
    public var fontScale: Double
    public var font: Font
    /// A concrete font family (from the system style); wins over `font`.
    public var fontName: String?
    public var textColor: UInt32
    /// Box behind each line; transparent: none.
    public var backgroundColor: UInt32
    /// Box behind the whole cue (mpv has one box kind: used when there is
    /// no line box).
    public var windowColor: UInt32
    public var edge: Edge
    public var edgeColor: UInt32
    /// Distance of the last line from the bottom, fraction of the height.
    public var bottomMargin: Double

    /// Text height at scale 1 as a fraction of the picture height (Android's
    /// caption default, also used by pa_player).
    public static let textSizeFraction = 0.0533
    public static let defaultBottomMargin = 0.08

    public init(
        fontScale: Double = 1,
        font: Font = .sansSerif,
        fontName: String? = nil,
        textColor: UInt32 = 0xFFFF_FFFF,
        backgroundColor: UInt32 = 0x0000_0000,
        windowColor: UInt32 = 0x0000_0000,
        edge: Edge = .outline,
        edgeColor: UInt32 = 0xFF00_0000,
        bottomMargin: Double = SubtitleStyle.defaultBottomMargin
    ) {
        self.fontScale = fontScale
        self.font = font
        self.fontName = fontName
        self.textColor = textColor
        self.backgroundColor = backgroundColor
        self.windowColor = windowColor
        self.edge = edge
        self.edgeColor = edgeColor
        self.bottomMargin = bottomMargin
    }

    /// White outlined sans-serif text near the bottom.
    public static let standard = SubtitleStyle()

    /// mpv scales subtitle sizes and margins to a 720-pixel-high window.
    static let mpvReferenceHeight = 720.0

    /// Font family for mpv/libass on Apple platforms.
    var mpvFont: String {
        if let fontName, !fontName.isEmpty { return fontName }
        switch font {
        case .sansSerif, .smallCaps: return "Helvetica Neue"
        case .serif: return "Georgia"
        case .monospace: return "Menlo"
        case .casual: return "Chalkboard SE"
        case .cursive: return "Snell Roundhand"
        case .condensed: return "Avenir Next Condensed"
        }
    }

    /// mpv color `#AARRGGBB`.
    static func mpvColor(_ argb: UInt32) -> String {
        String(format: "#%08X", argb)
    }

    static func alpha(_ argb: UInt32) -> UInt32 { argb >> 24 }

    /// The mpv options of this style, in the order to set them. Text
    /// subtitles (SRT, VTT…) take them; ASS keeps its own styles.
    public func mpvOptions() -> [(name: String, value: String)] {
        let size = Self.textSizeFraction * Self.mpvReferenceHeight * min(max(fontScale, 0.25), 4)
        let box = Self.alpha(backgroundColor) > 0 ? backgroundColor
            : Self.alpha(windowColor) > 0 ? windowColor : 0
        var options: [(String, String)] = [
            ("sub-font", mpvFont),
            ("sub-font-size", String(format: "%.1f", size)),
            ("sub-color", Self.mpvColor(textColor)),
            ("sub-margin-y", String(Int((min(max(bottomMargin, 0), 0.5) * Self.mpvReferenceHeight).rounded()))),
            ("sub-align-y", "bottom"),
        ]
        let border = max(1, size * 0.08)
        let shadow = max(1, size * 0.05)
        if box != 0 {
            options += [
                ("sub-border-style", "background-box"),
                ("sub-back-color", Self.mpvColor(box)),
                ("sub-border-size", "0"),
                ("sub-shadow-offset", "0"),
            ]
            return options
        }
        options.append(("sub-border-style", "outline-and-shadow"))
        options.append(("sub-back-color", Self.mpvColor(0)))
        switch edge {
        case .none:
            options += [("sub-border-size", "0"), ("sub-shadow-offset", "0")]
        case .outline:
            options += [
                ("sub-border-size", String(format: "%.1f", border)),
                ("sub-border-color", Self.mpvColor(edgeColor)),
                ("sub-shadow-offset", "0"),
            ]
        case .dropShadow, .raised, .depressed:
            // libass has one shadow below-right: raised and depressed are
            // approximated by it (depressed with a thin dark border).
            options += [
                ("sub-border-size", edge == .depressed ? "1" : "0"),
                ("sub-border-color", Self.mpvColor(edgeColor)),
                ("sub-shadow-offset", String(format: "%.1f", shadow)),
                ("sub-shadow-color", Self.mpvColor(edgeColor)),
            ]
        }
        return options
    }
}

/// The system caption settings as plain values: read from MediaAccessibility
/// on Apple platforms (`SystemCaptions.current()`), testable anywhere.
public struct SystemCaptionValues: Equatable, Sendable {
    /// "Closed Captions + SDH" is on (display type "always on").
    public var enabled: Bool
    public var relativeSize: Double
    public var foreground: UInt32?
    public var background: UInt32?
    public var window: UInt32?
    public var edge: SubtitleStyle.Edge?
    public var fontName: String?
    /// Preferred caption languages (BCP 47), best first.
    public var languages: [String]

    public init(
        enabled: Bool,
        relativeSize: Double = 1,
        foreground: UInt32? = nil,
        background: UInt32? = nil,
        window: UInt32? = nil,
        edge: SubtitleStyle.Edge? = nil,
        fontName: String? = nil,
        languages: [String] = []
    ) {
        self.enabled = enabled
        self.relativeSize = relativeSize
        self.foreground = foreground
        self.background = background
        self.window = window
        self.edge = edge
        self.fontName = fontName
        self.languages = languages
    }

    /// The look of these settings. MediaAccessibility always has a style
    /// (the default one when the user changed nothing), so it applies even
    /// with captions off; `bottomMargin` is the user's (the system has no
    /// position).
    public func style(bottomMargin: Double = SubtitleStyle.defaultBottomMargin) -> SubtitleStyle {
        let base = SubtitleStyle.standard
        return SubtitleStyle(
            fontScale: relativeSize > 0 ? relativeSize : 1,
            font: base.font,
            fontName: fontName,
            textColor: foreground ?? base.textColor,
            backgroundColor: background ?? base.backgroundColor,
            windowColor: window ?? base.windowColor,
            edge: edge ?? base.edge,
            edgeColor: base.edgeColor,
            bottomMargin: bottomMargin
        )
    }

    /// ARGB from RGBA components 0…1 and an opacity 0…1.
    public static func argb(red: Double, green: Double, blue: Double, alpha: Double) -> UInt32 {
        func byte(_ v: Double) -> UInt32 { UInt32((min(max(v, 0), 1) * 255).rounded()) }
        return byte(alpha) << 24 | byte(red) << 16 | byte(green) << 8 | byte(blue)
    }
}

#if canImport(MediaAccessibility)
import CoreGraphics
import CoreText
import MediaAccessibility

/// The user's caption settings (Settings → Accessibility → Subtitles &
/// Captioning on iOS and tvOS). Re-read on
/// `kMACaptionAppearanceSettingsChangedNotification`.
public enum SystemCaptions {
    public static func current() -> SystemCaptionValues {
        let domain = MACaptionAppearanceDomain.user
        func color(_ cg: CGColor, _ opacity: CGFloat) -> UInt32? {
            guard let srgb = CGColorSpace(name: CGColorSpace.sRGB),
                  let c = cg.converted(to: srgb, intent: .defaultIntent, options: nil),
                  let comps = c.components, comps.count >= 3
            else { return nil }
            return SystemCaptionValues.argb(
                red: Double(comps[0]), green: Double(comps[1]), blue: Double(comps[2]),
                alpha: Double(opacity) * Double(comps.count > 3 ? comps[3] : 1)
            )
        }
        let foreground = color(
            MACaptionAppearanceCopyForegroundColor(domain, nil).takeRetainedValue(),
            MACaptionAppearanceGetForegroundOpacity(domain, nil)
        )
        let background = color(
            MACaptionAppearanceCopyBackgroundColor(domain, nil).takeRetainedValue(),
            MACaptionAppearanceGetBackgroundOpacity(domain, nil)
        )
        let window = color(
            MACaptionAppearanceCopyWindowColor(domain, nil).takeRetainedValue(),
            MACaptionAppearanceGetWindowOpacity(domain, nil)
        )
        let edge: SubtitleStyle.Edge?
        switch MACaptionAppearanceGetTextEdgeStyle(domain, nil) {
        case .none: edge = SubtitleStyle.Edge.none
        case .raised: edge = SubtitleStyle.Edge.raised
        case .depressed: edge = SubtitleStyle.Edge.depressed
        case .uniform: edge = SubtitleStyle.Edge.outline
        case .dropShadow: edge = SubtitleStyle.Edge.dropShadow
        default: edge = nil
        }
        let descriptor = MACaptionAppearanceCopyFontDescriptorForStyle(domain, nil, .default)
            .takeRetainedValue()
        let family = CTFontDescriptorCopyAttribute(descriptor, kCTFontFamilyNameAttribute) as? String
        let languages = (MACaptionAppearanceCopySelectedLanguages(domain).takeRetainedValue()
            as? [String]) ?? []
        return SystemCaptionValues(
            enabled: MACaptionAppearanceGetDisplayType(domain) == .alwaysOn,
            relativeSize: Double(MACaptionAppearanceGetRelativeCharacterSize(domain, nil)),
            foreground: foreground,
            background: background,
            window: window,
            edge: edge,
            fontName: family,
            languages: languages
        )
    }
}
#endif
