import XCTest

@testable import AeroricProtocol

/// 2026-11-07: an approval a Codex subagent asked for says which subagent,
/// in the update and in the pending snapshot; the session's own leave it nil.
final class SubagentApprovalTests: XCTestCase {
    func testTheSubagentTravelsWithTheRequestAndThePendingSnapshot() throws {
        let update = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("3"), "atMs": .number("1"), "type": .string("permissionRequest"),
            "id": .string("9"), "kind": .string("command"), "summary": .string("npm test"),
            "options": .array([.string("allowOnce"), .string("deny")]), "subagent": .string("worker"),
        ]))
        guard case let .permissionRequest(_, _, _, _, _, subagent) = update.kind else {
            return XCTFail("a permission request")
        }
        XCTAssertEqual(subagent, "worker")

        let own = LiveUpdateKind.decode(type: "permissionRequest", [
            "id": .string("8"), "kind": .string("command"), "summary": .string("ls"), "options": .array([]),
        ])
        XCTAssertEqual(own, .permissionRequest(id: "8", toolCallId: nil, kind: "command", summary: "ls", options: []))

        let state = try LiveSessionState.decode(.object([
            "taskId": .string("t"), "live": .bool(true), "structured": .bool(true), "turnActive": .bool(false),
            "plan": .array([]), "latestSeq": .number("3"),
            "pending": .array([.object(["id": .string("9"), "kind": .string("command"), "summary": .string("npm test"),
                                        "subagent": .string("worker")])]),
        ]))
        XCTAssertEqual(state.pending.first?.subagent, "worker")
    }
}
