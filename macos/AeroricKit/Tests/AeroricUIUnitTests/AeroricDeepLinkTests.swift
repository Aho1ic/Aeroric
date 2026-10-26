@testable import AeroricUI
import XCTest

/// A16: deep links parse strictly; anything else is ignored.
final class AeroricDeepLinkTests: XCTestCase {
    func testTaskAndAttentionLinksRoundTrip() {
        let id = "465ebe83-f057-4f5b-b759-cc2a870e7009"
        XCTAssertEqual(AeroricDeepLink.target(URL(string: "aeroric://task/\(id)")!), .task(id))
        XCTAssertEqual(AeroricDeepLink.target(AeroricDeepLink.url(for: .task(id))), .task(id))
        XCTAssertEqual(AeroricDeepLink.target(URL(string: "aeroric://attention")!), .attention)
        XCTAssertEqual(AeroricDeepLink.target(URL(string: "aeroric://section/agentConfigs")!), .section("agentConfigs"))
        XCTAssertNil(AeroricDeepLink.target(URL(string: "aeroric://section/a-b")!))
    }

    func testUnknownOrUnsafeLinksAreIgnored() {
        for raw in ["https://task/x", "aeroric://task/", "aeroric://task/..%2Fetc", "aeroric://settings", "aeroric://task/a b"] {
            if let url = URL(string: raw) {
                XCTAssertNil(AeroricDeepLink.target(url), raw)
            }
        }
    }
}
