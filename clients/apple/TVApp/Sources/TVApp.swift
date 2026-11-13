// SPDX-License-Identifier: MPL-2.0
// TV-001: entry point of the tvOS app (SwiftUI, ADR-0002).
import SwiftUI

@main
struct TVApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
                .preferredColorScheme(.dark)
        }
    }
}
