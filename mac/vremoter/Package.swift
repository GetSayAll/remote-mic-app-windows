// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "vRemote",
    platforms: [.macOS(.v12)],
    dependencies: [
        .package(
            url: "https://github.com/TelemetryDeck/SwiftSDK.git",
            exact: "2.9.10"
        )
    ],
    targets: [
        .executableTarget(
            name: "vRemote",
            dependencies: [
                .product(name: "TelemetryDeck", package: "SwiftSDK")
            ],
            path: "Sources/vRemote",
            swiftSettings: [
                .define("DEBUG", .when(configuration: .debug))
            ]
        )
    ]
)
