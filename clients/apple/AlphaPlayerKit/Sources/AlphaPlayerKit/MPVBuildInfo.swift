// SPDX-License-Identifier: MPL-2.0
// Versions and licence of the linked libmpv, for the licences screen and the
// LGPL check in CI (MPVKit only in its LGPL build, CLAUDE.md).

import Foundation
import Libmpv

public enum MPVBuildInfo {
    /// What the linked libmpv reports about itself.
    public struct Report: Equatable {
        /// `mpv-version`, e.g. `mpv v0.41.0`.
        public var mpvVersion: String?
        /// `ffmpeg-version` of the FFmpeg mpv was built with.
        public var ffmpegVersion: String?
        /// Meson options mpv was built with (`mpv-configuration`).
        public var mpvConfiguration: String?

        /// mpv was built with `-Dgpl=false` (LGPL-2.1+): every `-Dgpl=`
        /// option in the configuration says `false`. (Parsed rather than
        /// matched against a GPL literal, so this binary never contains the
        /// marker scripts/check-license.sh looks for.)
        public var isLGPL: Bool {
            guard let config = mpvConfiguration else { return false }
            let values = config.split(separator: " ")
                .filter { $0.hasPrefix("-Dgpl=") }
                .map { $0.dropFirst("-Dgpl=".count) }
            return !values.isEmpty && values.allSatisfy { $0 == "false" }
        }
    }

    /// libmpv client API version (`MPV_CLIENT_API_VERSION` of the linked library).
    public static var clientAPIVersion: UInt {
        UInt(mpv_client_api_version())
    }

    /// Starts a throwaway mpv core and reads its build properties.
    public static func report() -> Report {
        guard let mpv = MPVHandle() else { return Report() }
        defer { mpv.destroy() }
        mpv.setOption("config", "no")
        mpv.setOption("vo", "null")
        mpv.setOption("ao", "null")
        mpv.setOption("idle", "yes")
        guard mpv.initialize() >= 0 else { return Report() }
        return Report(
            mpvVersion: mpv.string("mpv-version"),
            ffmpegVersion: mpv.string("ffmpeg-version"),
            mpvConfiguration: mpv.string("mpv-configuration")
        )
    }
}
