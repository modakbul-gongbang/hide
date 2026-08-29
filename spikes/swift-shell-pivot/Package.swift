// swift-tools-version: 5.10

import PackageDescription

let package = Package(
    name: "SwiftShellPivotSpike",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "SwiftShellSpike", targets: ["SwiftShellSpike"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/migueldeicaza/SwiftTerm.git",
            exact: "1.20.0"
        ),
    ],
    targets: [
        .systemLibrary(
            name: "CHerdrCore",
            path: "include"
        ),
        .target(
            name: "SpikeComposition",
            path: "swift/Sources/SpikeComposition"
        ),
        .executableTarget(
            name: "SwiftShellSpike",
            dependencies: [
                "CHerdrCore",
                "SpikeComposition",
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            path: "swift/Sources/SwiftShellSpike",
            linkerSettings: [
                .unsafeFlags([
                    "-L", "rust-core/target/release",
                    "-lherdr_core_spike",
                ]),
            ]
        ),
        .testTarget(
            name: "SpikeCompositionTests",
            dependencies: ["SpikeComposition"],
            path: "swift/Tests/SpikeCompositionTests"
        ),
    ]
)
