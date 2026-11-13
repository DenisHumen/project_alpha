// SPDX-License-Identifier: MPL-2.0
// TV-001: top-level sections of the tvOS app, in tab-bar order
// (docs/research/01 §2.3: Home · Movies · TV Shows · Twitch · Music · Search · Settings).

enum AppTab: String, CaseIterable, Identifiable, Hashable {
    case home
    case movies
    case tvShows = "tv-shows"
    case twitch
    case music
    case search
    case settings

    var id: String { rawValue }

    /// Title in the top tab bar.
    var title: String {
        switch self {
        case .home: "Home"
        case .movies: "Movies"
        case .tvShows: "TV Shows"
        case .twitch: "Twitch"
        case .music: "Music"
        case .search: "Search"
        case .settings: "Settings"
        }
    }

    /// Design-system icon (DSN-004) shown on the section screen.
    var icon: PaIcon {
        switch self {
        case .home: .home
        case .movies: .movie
        case .tvShows: .tvShow
        case .twitch: .live
        case .music: .music
        case .search: .search
        case .settings: .settings
        }
    }

    /// Accessibility identifier of the section's root view (used by UI tests).
    var screenID: String { "screen-" + rawValue }
}
