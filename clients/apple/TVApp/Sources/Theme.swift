// SPDX-License-Identifier: MPL-2.0
// The TV app's view of the design tokens. Values come from the generated
// design/tokens/generated/apple/Tokens.swift (DSN-003, `just design tokens`);
// nothing is copied by hand here. TV is always dark and uses the tv density.
import SwiftUI

enum Theme {
    /// Colour roles: the TV app is always dark (01 §4.1).
    static let colors = PaColorScheme.dark
    /// Typography, grid and sizes of the 10-foot UI.
    static let density = PaDensity.tv

    enum Color {
        static let bgBase = Theme.colors.bgBase
        static let bgElevated = Theme.colors.bgElevated
        static let bgElevated2 = Theme.colors.bgElevated2
        static let textPrimary = Theme.colors.textPrimary
        static let textSecondary = Theme.colors.textSecondary
        static let textTertiary = Theme.colors.textTertiary
        static let accent = PaTokens.accentDefault
        static let accentText = Theme.colors.textLink
        static let accentSubtle = Theme.colors.accentSubtle
    }

    /// TV safe area (`layout.safe-side`, `layout.safe-top`: 80 / 60 pt).
    static let safeHorizontal = density.layoutSafeSide
    static let safeVertical = density.layoutSafeTop
    /// Gap between cards in a row (`layout.gutter`).
    static let rowSpacing = density.layoutGutter
}
