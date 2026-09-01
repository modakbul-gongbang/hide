// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "HerdrMacOS",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "HerdrMacOS", targets: ["HerdrMacOS"]),
    ],
    dependencies: [
        .package(path: "Vendor/SwiftTerm"),
        .package(
            url: "https://github.com/raspu/Highlightr.git",
            exact: "2.3.0"
        ),
        .package(
            url: "https://github.com/gonzalezreal/swift-markdown-ui.git",
            exact: "2.4.1"
        ),
    ],
    targets: [
        .systemLibrary(
            name: "CHerdrCore",
            path: "CHerdrCore"
        ),
        .executableTarget(
            name: "HerdrMacOS",
            dependencies: [
                "CHerdrCore",
                .product(name: "SwiftTerm", package: "SwiftTerm"),
                .product(name: "Highlightr", package: "Highlightr"),
                .product(name: "MarkdownUI", package: "swift-markdown-ui"),
            ],
            path: "Sources/HerdrMacOS",
            resources: [
                .process("Resources"),
            ],
            linkerSettings: [
                .linkedFramework("SystemConfiguration"),
                .unsafeFlags([
                    "-L", "../target/release",
                    "-lherdr_core",
                ]),
            ]
        ),
        .testTarget(
            name: "HerdrMacOSTests",
            dependencies: [
                "HerdrMacOS",
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            path: "Tests/HerdrMacOSTests"
        ),
    ]
)
