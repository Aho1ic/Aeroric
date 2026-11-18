@testable import AeroricProtocol
import XCTest

/// B2-6: a delegate bound to an agent configuration keeps the binding through
/// a decode and an encode (editing a role must not drop it).
final class LiveDelegateAgentTests: XCTestCase {
    func testTheAgentConfigurationSurvivesARoundTrip() {
        let wire: JSONValue = .object(["agent": .string("codex"), "agentConfigId": .string("deepseek"),
                                       "model": .string("deepseek-v4")])
        let agent = LiveDelegateAgent.decode(wire)
        XCTAssertEqual(agent, .named(agent: "codex", agentConfigId: "deepseek", model: "deepseek-v4"))
        XCTAssertEqual(agent.json, wire)
        XCTAssertEqual(LiveDelegateAgent.decode(.object(["agent": .string("claude")])).json,
                       .object(["agent": .string("claude")]))
    }

    /// B2-6: a receipt's reason, a turn's check and its state before undo,
    /// and a question's asked time from the update all reach the client.
    func testFieldsTheHostSendsAreKept() throws {
        let receipt = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("1"), "atMs": .number("5"), "type": .string("deliveryReceipt"),
            "id": .string("dv"), "outcome": .string("failed"), "reason": .string("Agent 已退出"),
        ]))
        XCTAssertEqual(receipt.kind, .deliveryReceipt(id: "dv", outcome: "failed", reason: "Agent 已退出"))
        let question = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("2"), "atMs": .number("1234"), "type": .string("question"),
            "id": .string("q1"), "style": .string("confirm"), "title": .string("继续？"),
        ]))
        guard case let .question(asked) = question.kind else { return XCTFail("\(question.kind)") }
        XCTAssertEqual(asked.askedAtMs, 1234)
        let record = LiveTurnRecord.decode(.object([
            "turnId": .string("u1"), "state": .string("undone"), "stateBeforeUndo": .string("accepted"),
            "check": .object(["passed": .bool(false), "summary": .string("测试 1 项失败")]),
        ]))
        XCTAssertEqual(record?.stateBeforeUndo, "accepted")
        XCTAssertEqual(record?.check, LiveTurnCheck(passed: false, summary: "测试 1 项失败"))
    }

    /// B2-4: an SSH project reads the host's remote tables; a local one the Mac's.
    func testTheProjectsPlacePicksTheCapabilityTables() {
        let local = ["dsh": LiveCapabilities()]
        let remote: [String: LiveCapabilities] = [:]
        XCTAssertEqual(CapabilityPlace.tables(projectPath: "ssh://dev/home/me/app", local: local, remote: remote), remote)
        XCTAssertEqual(CapabilityPlace.tables(projectPath: "/Users/me/app", local: local, remote: remote), local)
        XCTAssertEqual(CapabilityPlace.tables(projectPath: nil, local: local, remote: remote), local)
    }
}
