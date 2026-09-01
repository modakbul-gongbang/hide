// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "SwiftTerm",
    platforms: [
        .iOS(.v14),
        .macOS(.v11),
        .tvOS(.v13),
        .visionOS(.v1),
    ],
    products: [
        .library(name: "SwiftTerm", targets: ["SwiftTerm"]),
    ],
    targets: [
        .target(
            name: "SwiftTerm",
            path: "Sources/SwiftTerm",
            exclude: ["Mac/README.md"],
            resources: [
                .process("Apple/Metal/Shaders.metal"),
            ],
            plugins: [
                .plugin(name: "SwiftTermBuildInfoPlugin"),
            ]
        ),
        .executableTarget(
            name: "SwiftTermBuildInfoGenerator",
            path: "Sources/SwiftTermBuildInfoGenerator"
        ),
        .plugin(
            name: "SwiftTermBuildInfoPlugin",
            capability: .buildTool(),
            dependencies: ["SwiftTermBuildInfoGenerator"]
        ),
    ],
    swiftLanguageModes: [.v5]
)
