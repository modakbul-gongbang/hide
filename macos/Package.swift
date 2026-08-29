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
        .package(
            url: "https://github.com/migueldeicaza/SwiftTerm.git",
            exact: "1.20.0"
        ),
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
            linkerSettings: [
                .unsafeFlags([
                    "-L", "../target/release",
                    "-lherdr_core",
                ]),
            ]
        ),
    ]
)
