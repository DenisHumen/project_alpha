// SPDX-License-Identifier: MPL-2.0
// TV-001: empty screen for a section that has no implementation yet.
import SwiftUI

struct SectionPlaceholderView: View {
    let tab: AppTab

    var body: some View {
        VStack(spacing: 28) {
            tab.icon.image()
                .resizable()
                .frame(width: 120, height: 120)
                .foregroundStyle(Theme.Color.textTertiary)
            Text(tab.title)
                .font(.title2.weight(.semibold))
                .foregroundStyle(Theme.Color.textPrimary)
                .accessibilityIdentifier(tab.screenID)
            Text("Coming soon")
                .font(.body)
                .foregroundStyle(Theme.Color.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.Color.bgBase.ignoresSafeArea())
    }
}

#Preview {
    SectionPlaceholderView(tab: .movies)
}
