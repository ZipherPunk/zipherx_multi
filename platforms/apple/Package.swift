// swift-tools-version: 5.9
// ZipherX Swift Package — Phase 10a: Apple platform wrapper.
//
// Provides:
//   - ZipherXSwift library: thin Swift wrappers around UniFFI-generated
//     bindings from the zipherx-ffi Rust crate.
//
// UniFFI bindings (ZipherXFFI module) are generated separately by the
// build pipeline and are not listed as a dependency here.  All call
// sites in this package are guarded with `#if canImport(ZipherXFFI)`.

import PackageDescription

let package = Package(
    name: "ZipherXSwift",
    platforms: [
        .iOS(.v16),
        .macOS(.v13),
    ],
    products: [
        .library(
            name: "ZipherXSwift",
            targets: ["ZipherXSwift"]
        ),
    ],
    targets: [
        .target(
            name: "ZipherXSwift",
            path: "Sources/ZipherXSwift"
        ),
    ]
)
