// SPDX-License-Identifier: MPL-2.0
// TV-001: Home skeleton. Real rows (Up Next, recently added, hero backdrop)
// come with TV-007; here: brand header, core status and focusable placeholder
// rows to exercise the focus engine (`.card` button style).
import SwiftUI

struct HomeView: View {
    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 56) {
                header
                PlaceholderRow(title: "Continue Watching", icon: .history, aspect: 16.0 / 9.0, width: 420)
                PlaceholderRow(title: "Recently Added", icon: .movie, aspect: 2.0 / 3.0, width: 240)
            }
            .padding(.horizontal, Theme.safeHorizontal)
            .padding(.vertical, Theme.safeVertical)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Theme.Color.bgBase.ignoresSafeArea())
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Kinolune")
                .font(.system(size: 76, weight: .bold))
                .foregroundStyle(Theme.Color.textPrimary)
                .accessibilityIdentifier(AppTab.home.screenID)
            Text("Your library appears here after you add a source.")
                .font(.title3)
                .foregroundStyle(Theme.Color.textSecondary)
            HStack(spacing: 12) {
                PaIcon.info.image()
                    .resizable()
                    .frame(width: 28, height: 28)
                    .foregroundStyle(Theme.Color.accentText)
                Text(BuildInfo.summary)
                    .font(.headline)
                    .foregroundStyle(Theme.Color.accentText)
                    .accessibilityIdentifier("core-version")
            }
        }
    }
}

/// A titled horizontal row of empty, focusable cards.
struct PlaceholderRow: View {
    let title: String
    let icon: PaIcon
    let aspect: CGFloat
    let width: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(title)
                .font(.title3.weight(.semibold))
                .foregroundStyle(Theme.Color.textSecondary)
            ScrollView(.horizontal) {
                LazyHStack(spacing: Theme.rowSpacing) {
                    ForEach(0 ..< 6, id: \.self) { _ in
                        Button {} label: {
                            ZStack {
                                Theme.Color.bgElevated2
                                icon.image()
                                    .resizable()
                                    .frame(width: 64, height: 64)
                                    .foregroundStyle(Theme.Color.textTertiary)
                            }
                            .frame(width: width, height: width / aspect)
                        }
                        .buttonStyle(.card)
                    }
                }
                // Room for the focus scale and shadow of the card style.
                .padding(.vertical, 24)
            }
            .scrollClipDisabled()
        }
    }
}

#Preview {
    HomeView()
}
