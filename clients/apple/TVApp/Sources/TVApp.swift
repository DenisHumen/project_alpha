// SPDX-License-Identifier: MPL-2.0
// TV-001: entry point of the tvOS app (SwiftUI, ADR-0002).
import SwiftUI

@main
struct TVApp: App {
    init() {
        // CORE-015: the core keeps passwords and tokens in the Keychain.
        do {
            try CoreSecrets.open()
        } catch {
            print("Core secret store unavailable: \(error)")
        }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .preferredColorScheme(.dark)
        }
    }
}
