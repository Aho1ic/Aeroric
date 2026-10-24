import AeroricProtocol
import XCTest

@testable import AeroricClient

final class LegacyCleanupClientTests: XCTestCase {
    func testSourceIDsFollowTheHostRules() throws {
        XCTAssertEqual(
            try LegacyCleanupClient.validateSourceIDs(["custom:a", "custom:b", "custom:a"]),
            ["custom:a", "custom:b"]
        )
        for bad in [[], ["builtin:claude"], ["custom:"], ["custom:  "], ["a"]] as [[String]] {
            XCTAssertThrowsError(try LegacyCleanupClient.validateSourceIDs(bad), "\(bad)")
        }
        let tooMany = (0...LegacyCleanupContract.maxSourceIDs).map { "custom:\($0)" }
        XCTAssertThrowsError(try LegacyCleanupClient.validateSourceIDs(tooMany))
    }

    func testHandshakeCapabilitiesAreParsedWithoutGuessing() {
        let reply: JSONValue = .object([
            "clientId": .string("c"),
            "capabilities": .array([
                .object(["name": .string("agentConnections.legacyCleanup"), "enabled": .bool(true),
                         "since": .number("1"), "note": .string("n")]),
                .object(["name": .string("task.submissions"), "enabled": .bool(false)]),
                .object(["name": .string("broken")]),
                .object(["enabled": .bool(true)]),
                .string("noise"),
            ]),
        ])
        XCTAssertEqual(HostConnection.parseCapabilities(reply), [
            "agentConnections.legacyCleanup": true, "task.submissions": false,
        ])
        XCTAssertEqual(HostConnection.parseCapabilities(.object(["clientId": .string("c")])), [:])
        XCTAssertEqual(HostConnection.parseCapabilities(.null), [:])
    }

    func testMalformedRepliesSurfaceAsMalformedReply() {
        XCTAssertThrowsError(try LegacyCleanupClient.decode(.object([:]))) { error in
            XCTAssertTrue(error is WorkspaceClient.MalformedReply, "\(error)")
        }
    }
}
