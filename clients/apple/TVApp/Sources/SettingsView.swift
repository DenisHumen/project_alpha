// SPDX-License-Identifier: MPL-2.0
// TV-001: Settings skeleton, a classic tvOS list (01 §2.2) with the "About" facts.
import SwiftUI

struct SettingsView: View {
    var body: some View {
        HStack(alignment: .top, spacing: 80) {
            PaIcon.settings.image()
                .resizable()
                .frame(width: 240, height: 240)
                .foregroundStyle(Theme.Color.textTertiary)
                .frame(maxWidth: .infinity)
            List {
                Section("About") {
                    row("Core version", BuildInfo.core, id: "settings-core-version")
                    row("Build flavour", BuildInfo.flavor.rawValue, id: "settings-flavor")
                    row("App version", appVersion, id: "settings-app-version")
                }
            }
            .frame(maxWidth: .infinity)
            .accessibilityIdentifier(AppTab.settings.screenID)
        }
        .padding(.horizontal, Theme.safeHorizontal)
        .padding(.vertical, Theme.safeVertical)
        .background(Theme.Color.bgBase.ignoresSafeArea())
    }

    private var appVersion: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(version) (\(build))"
    }

    private func row(_ title: String, _ value: String, id: String) -> some View {
        // A focusable no-op button so the list rows take focus like system Settings.
        Button {} label: {
            LabeledContent(title, value: value)
        }
        .accessibilityIdentifier(id)
    }
}

#Preview {
    SettingsView()
}
