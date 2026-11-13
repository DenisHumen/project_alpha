// swift-tools-version:5.9
// SPDX-License-Identifier: MPL-2.0
//
// AlphaPlayerKit: Apple playback engines behind the core `PlayerEngine`
// contract (PLAY-APL, ADR-0003, ADR-0017). Used by the Flutter plugin
// pa_player_apple (PLAY-APL-004) and the tvOS app (TV-*).
//
// MPVKit: only the LGPL product `MPVKit`, never `MPVKit-GPL`.
// scripts/check-license.sh enforces this in CI (manifest and linked binary).
import PackageDescription

let package = Package(
    name: "AlphaPlayerKit",
    // Same minimums as PACore and MPVKit.
    platforms: [.iOS(.v15), .tvOS(.v15), .macOS(.v12)],
    products: [
        .library(name: "AlphaPlayerKit", targets: ["AlphaPlayerKit"]),
    ],
    dependencies: [
        // Rust core over UniFFI; run tools/bindings/build-xcframework.sh first.
        .package(path: "../../../tools/bindings/PACore"),
        .package(url: "https://github.com/mpvkit/MPVKit.git", exact: "1.1.0-n9.0.2"),
    ],
    targets: [
        .target(
            name: "AlphaPlayerKit",
            dependencies: [
                .product(name: "PACore", package: "PACore"),
                .product(name: "MPVKit", package: "MPVKit"),
            ]
        ),
        .testTarget(
            name: "AlphaPlayerKitTests",
            dependencies: [
                "AlphaPlayerKit",
                .product(name: "PACore", package: "PACore"),
            ]
        ),
    ]
)
