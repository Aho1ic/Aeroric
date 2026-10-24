// swift-tools-version:6.0
import PackageDescription

// Local Swift package for the native macOS client. Dependency direction is
// one-way: AeroricUI -> AeroricClient -> AeroricProtocol (INV-02).
// P5-01 adds the FIRST external dependency: SwiftTerm (the terminal renderer).
// The version is pinned exactly (and locked in Package.resolved) so a minor
// bump can never silently change the delegate surface the coordinator codes
// against — the spike in Terminal/SwiftTermSpike.swift asserts the real API.
let package = Package(
    name: "AeroricKit",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "AeroricProtocol", targets: ["AeroricProtocol"]),
        .library(name: "AeroricClient", targets: ["AeroricClient"]),
        .library(name: "AeroricUI", targets: ["AeroricUI"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/migueldeicaza/SwiftTerm.git",
            .exact("1.2.5")
        ),
        // Redesign 01 (Q23): Markdown rendering for the conversation view.
        // MIT, pinned exactly like SwiftTerm.
        .package(
            url: "https://github.com/gonzalezreal/swift-markdown-ui.git",
            .exact("2.4.1")
        ),
    ],
    targets: [
        .target(name: "AeroricProtocol"),
        .target(name: "AeroricClient", dependencies: ["AeroricProtocol"]),
        .target(
            name: "AeroricUI",
            dependencies: [
                "AeroricProtocol", "AeroricClient",
                .product(name: "SwiftTerm", package: "SwiftTerm"),
                .product(name: "MarkdownUI", package: "swift-markdown-ui"),
            ]
        ),
        .testTarget(name: "AeroricProtocolTests", dependencies: ["AeroricProtocol"]),
        .testTarget(name: "AeroricClientTests", dependencies: ["AeroricClient", "AeroricProtocol"]),
        .testTarget(name: "AeroricUIUnitTests", dependencies: ["AeroricUI", "AeroricClient", "AeroricProtocol"]),
    ]
)
