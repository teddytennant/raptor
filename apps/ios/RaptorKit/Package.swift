// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RaptorKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "RaptorKit", targets: ["RaptorKit"]),
    ],
    targets: [
        // Built by scripts/build-apple.sh from crates/raptor-ffi.
        .binaryTarget(name: "RaptorFFI", path: "RaptorFFI.xcframework"),
        .target(
            name: "RaptorKit",
            dependencies: ["RaptorFFI"],
            resources: [.process("Resources")],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [.linkedFramework("SystemConfiguration"), .linkedLibrary("c++")]
        ),
        // macOS-only tool that renders RaptorKit views and terminals to PNGs (no windows).
        .executableTarget(
            name: "raptor-snap",
            dependencies: ["RaptorKit"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
