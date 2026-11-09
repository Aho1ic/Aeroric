import AeroricClient
@testable import AeroricUI
import AeroricProtocol
import XCTest

/// R3-2: what a turn carried into the model, and an agent retrying with
/// nothing new to go on.
final class TurnContextTests: XCTestCase {
    private func stats() throws -> SessionTranscriptStats {
        try XCTUnwrap(SessionTranscriptStats.decode(.object([
            "turns": .array([.object([
                "startedAt": .number("0"), "tokens": .number("1000"),
                "context": .array([
                    .object(["kind": .string("system"), "tokens": .number("600")]),
                    .object(["kind": .string("instructions"), "tokens": .number("150"),
                             "sources": .array([.string("/repo/CLAUDE.md"), .string("/repo/AGENTS.md")])]),
                    .object(["kind": .string("new"), "tokens": .number("250")]),
                ]),
            ])]),
            "retry": .object(["name": .string("Bash"), "detail": .string("cargo test"),
                              "count": .number("3"), "at": .number("42")]),
        ])))
    }

    func testTheContextDecodesAndReadsAsSharesOfTheCall() throws {
        let turn = try XCTUnwrap(stats().turns.first)
        XCTAssertEqual(turn.context.map(\.kind), ["system", "instructions", "new"])
        XCTAssertEqual(turn.context[1].sources, ["/repo/CLAUDE.md", "/repo/AGENTS.md"])
        XCTAssertEqual(TurnBillText.contextTotal(turn), 1000)
        XCTAssertEqual(TurnBillText.contextShare(turn.context[0], total: 1000), "≈600 tok · 60%")
        XCTAssertEqual(TurnBillText.contextLabel("instructions"), "项目说明")
        XCTAssertEqual(TurnBillText.contextLabel("whatever"), "其他说明")
        // No tool definitions in the transcript: the card says where they went.
        XCTAssertTrue(TurnBillText.contextNote(turn).hasPrefix("会话文件没记工具定义"))
    }

    func testARetryStreakIsTheFirstReminderAndAnAttentionRow() throws {
        let stats = try stats()
        let retry = try XCTUnwrap(stats.retry)
        XCTAssertEqual(retry, .init(name: "Bash", detail: "cargo test", count: 3, at: 42))
        let alerts = LiveRunaway.alerts(stats: stats, history: [], delegations: [], nowMs: 0)
        XCTAssertEqual(alerts.first?.id, "retry:Bash|cargo test")
        XCTAssertEqual(alerts.first?.text, "「cargo test」连续 3 次失败，报错都一样，中间没改过文件")

        let task = TaskStore.TaskEntry(id: "t", projectID: "p", title: "t", status: "running", agent: "codex",
                                       createdAtMs: 1, updatedAtMs: 1, archived: false, starred: false)
        let items = AttentionQueue.items([task], retries: ["t": .init(summary: LiveRunaway.retryText(retry), atMs: 42)],
                                         nowMs: 100)
        XCTAssertEqual(items.map(\.reason), [.retryNeedsYou])
        XCTAssertEqual(items.first?.id, "t#retry")
        XCTAssertEqual(AttentionQueue.Reason.retryNeedsYou.text, "在重试")
    }
}
