import AeroricProtocol
import XCTest
@testable import AeroricClient

/// A14: `power.keepAwake.state` decodes strictly.
final class PowerClientDecodingTests: XCTestCase {
    func testDecodesTheHostState() throws {
        let state = try PowerClient.decode(.object([
            "enabled": .bool(true), "active": .bool(true), "activeTasks": .number("2"),
        ]))
        XCTAssertEqual(state, PowerClient.State(enabled: true, active: true, activeTasks: 2))
    }

    func testRejectsAReplyMissingAField() {
        XCTAssertThrowsError(try PowerClient.decode(.object([
            "enabled": .bool(true), "active": .bool(false),
        ])))
    }
}
