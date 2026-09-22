// swift-tools-version:6.0
import PackageDescription

// Local Swift package for the native macOS client. Dependency direction is
// one-way: AeroricUI -> AeroricClient -> AeroricProtocol (INV-02).
// No external package dependencies yet, so Package.resolved is deliberately
// absent until P5-01 introduces the first one.
let package = Package(
    name: "AeroricKit",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "AeroricProtocol", targets: ["AeroricProtocol"]),
        .library(name: "AeroricClient", targets: ["AeroricClient"]),
        .library(name: "AeroricUI", targets: ["AeroricUI"]),
    ],
    targets: [
        .target(name: "AeroricProtocol"),
        .target(name: "AeroricClient", dependencies: ["AeroricProtocol"]),
        .target(name: "AeroricUI", dependencies: ["AeroricProtocol", "AeroricClient"]),
        .testTarget(name: "AeroricProtocolTests", dependencies: ["AeroricProtocol"]),
        .testTarget(name: "AeroricClientTests", dependencies: ["AeroricClient", "AeroricProtocol"]),
        .testTarget(name: "AeroricUIUnitTests", dependencies: ["AeroricUI", "AeroricClient", "AeroricProtocol"]),
    ]
)
