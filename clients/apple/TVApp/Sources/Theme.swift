// SPDX-License-Identifier: MPL-2.0
// TV-001: the few design tokens the skeleton needs, copied from design/tokens
// (semantic.tokens.json, dark theme; TV stays dark). Replace with the generated
// Tokens.swift once DSN-003 adds the Style Dictionary pipeline for Swift.
import SwiftUI

enum Theme {
    enum Color {
        /// bg/base = color/gray/950
        static let bgBase = rgb(0x0B0B0F)
        /// bg/elevated = color/gray/900
        static let bgElevated = rgb(0x16161C)
        /// bg/elevated-2 = color/gray/850
        static let bgElevated2 = rgb(0x1E1E25)
        /// text/primary = white
        static let textPrimary = SwiftUI.Color.white
        /// text/secondary = white @ 64 %
        static let textSecondary = SwiftUI.Color.white.opacity(0.64)
        /// text/tertiary = white @ 38 %
        static let textTertiary = SwiftUI.Color.white.opacity(0.38)
        /// accent/default = color/lune/600
        static let accent = rgb(0x00777D)
        /// text/link = color/lune/400
        static let accentText = rgb(0x49D1D8)
        /// accent/subtle = color/lune/800
        static let accentSubtle = rgb(0x003E41)

        private static func rgb(_ hex: UInt32) -> SwiftUI.Color {
            SwiftUI.Color(
                red: Double((hex >> 16) & 0xFF) / 255,
                green: Double((hex >> 8) & 0xFF) / 255,
                blue: Double(hex & 0xFF) / 255
            )
        }
    }

    /// TV safe area (01 §4.2: 60 pt top/bottom, 80 pt left/right).
    static let safeHorizontal: CGFloat = 80
    static let safeVertical: CGFloat = 60
    /// Gap between cards in a row (01 §4.2).
    static let rowSpacing: CGFloat = 40
}
