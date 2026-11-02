// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ShrewKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "ShrewKit", targets: ["ShrewKit"]),
    ],
    targets: [
        // Built by scripts/build-apple.sh from crates/shrew-ffi.
        .binaryTarget(name: "ShrewFFI", path: "ShrewFFI.xcframework"),
        .target(
            name: "ShrewKit",
            dependencies: ["ShrewFFI"],
            resources: [.process("Resources")],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [.linkedFramework("SystemConfiguration"), .linkedLibrary("c++")]
        ),
        // macOS-only tool that renders ShrewKit views and terminals to PNGs (no windows).
        .executableTarget(
            name: "shrew-snap",
            dependencies: ["ShrewKit"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
