// SPDX-License-Identifier: MPL-2.0
// Swift smoke test of the generated UniFFI bindings on the host (Linux or macOS).
// Compiled together with target/bindings/swift/PACore.swift by `just core smoke-swift`.

let version = coreVersion()
print("coreVersion() = \(version)")
let parts = version.split(separator: ".")
precondition(parts.count == 3 && parts.allSatisfy { UInt($0) != nil }, "unexpected core version: \(version)")
