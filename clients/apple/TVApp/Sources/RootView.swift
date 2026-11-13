// SPDX-License-Identifier: MPL-2.0
// TV-001: top tab bar (tvOS style) with one screen per section.
// Navigation is the system focus engine: swipe left/right in the tab bar,
// down into the content, Menu/Back returns to the tab bar.
import SwiftUI

struct RootView: View {
    @State private var selection: AppTab = .home

    var body: some View {
        TabView(selection: $selection) {
            ForEach(AppTab.allCases) { tab in
                screen(for: tab)
                    .tabItem { Text(tab.title) }
                    .tag(tab)
            }
        }
        .background(Theme.Color.bgBase.ignoresSafeArea())
    }

    @ViewBuilder
    private func screen(for tab: AppTab) -> some View {
        switch tab {
        case .home: HomeView()
        case .settings: SettingsView()
        default: SectionPlaceholderView(tab: tab)
        }
    }
}

#Preview {
    RootView()
}
