// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "AeroricCompanionKit",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .library(name: "AeroricMobileTransport", targets: ["AeroricMobileTransport"]),
        .library(name: "AeroricMobileUI", targets: ["AeroricMobileUI"]),
    ],
    targets: [
        .target(name: "AeroricMobileTransport"),
        .target(
            name: "AeroricMobileUI",
            dependencies: ["AeroricMobileTransport"]
        ),
        .testTarget(
            name: "AeroricCompanionTests",
            dependencies: ["AeroricMobileTransport", "AeroricMobileUI"]
        ),
    ]
)
