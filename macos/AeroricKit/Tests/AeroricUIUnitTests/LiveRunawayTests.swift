import XCTest
import AeroricProtocol
@testable import AeroricUI

final class LiveRunawayTests: XCTestCase {
    private func turn(_ assistant: [SessionContent]) -> [AeroricUI.SessionMessage] {
        [AeroricUI.SessionMessage(role: "user", content: [.text(text: "继续")]),
         AeroricUI.SessionMessage(role: "assistant", content: assistant)]
    }
    private let waiting: [SessionContent] = [.toolUse(id: "w", name: "wait", input: "{}"), .text(text: "还在跑")]
    private let working: [SessionContent] = [.toolUse(id: "e", name: "Edit", input: "{}"), .text(text: "改好了")]

    func testTurnsAreBusyIdleOrUnknown() {
        let agentWait = SessionContent.toolUse(id: "a", name: "Agent", input: #"{"tool":"wait_agent"}"#)
        let history = turn(working) + turn(waiting) + turn([agentWait]) + turn([]) + turn([.text(text: "好的")])
            + turn([.toolUse(id: "w", name: "wait", input: "{}"), .text(text: String(repeating: "结", count: 300))])
        XCTAssertEqual(LiveRunaway.turns(history), [.busy, .idle, .idle, .unknown, .busy, .busy])
        let toolResult = AeroricUI.SessionMessage(role: "user", content: [.toolResult(id: "w", output: "")])
        XCTAssertEqual(LiveRunaway.turns(turn(waiting) + [toolResult] + [AeroricUI.SessionMessage(
            role: "assistant", content: [.toolUse(id: "e", name: "Edit", input: "{}")])]), [.busy],
                       "a tool result is not the person speaking; the turn goes on")
    }

    func testIdleShareNeedsEnoughJudgedTurns() {
        var limits = LiveRunaway.Limits()
        limits.minTurns = 4
        let idle3busy1 = turn(waiting) + turn(waiting) + turn(waiting) + turn(working)
        let alerts = LiveRunaway.alerts(stats: nil, history: idle3busy1, delegations: [], nowMs: 0, limits: limits)
        XCTAssertEqual(alerts.map(\.id), ["idle"])
        XCTAssertEqual(alerts.first?.text, "最近 4 个回合里有 3 个空转（75%，提醒线 25%）")
        let withUnknowns = turn(waiting) + turn(waiting) + turn(waiting) + turn([]) + turn([])
        XCTAssertTrue(LiveRunaway.alerts(stats: nil, history: withUnknowns, delegations: [], nowMs: 0,
                                         limits: limits).isEmpty, "unknown turns are not counted as judged")
        XCTAssertTrue(LiveRunaway.alerts(stats: nil, history: idle3busy1, delegations: [], nowMs: 0).isEmpty,
                      "20 judged turns by default")
    }

    func testTokensActivityAndDelegationsOverTheirLines() {
        var stats = SessionTranscriptStats(inputTokens: 380_000_000, outputTokens: 30_000_000)
        stats.turns = [.init(startedAt: 1_000, endedAt: 2_000, tokens: 10)]
        let day: UInt64 = 24 * 3_600_000
        let card = LiveDelegationCard(
            delegationId: "dg-1", childTaskId: "t", origin: "session", roleName: "调研员", harnessLabel: "Codex",
            contract: LiveDelegationContract(instruction: "查", contextMode: "isolated", permission: "plan",
                                             workspace: "readOnly"),
            state: "running", tokens: 120_000_000)
        let alerts = LiveRunaway.alerts(stats: stats, history: [], delegations: [card],
                                        nowMs: 1_000 + 3 * day + 2 * 3_600_000)
        XCTAssertEqual(alerts.map(\.text), [
            "这个会话累计用了 4.1 亿 token（提醒线 4 亿）",
            "委派 · 调研员 已用 1.2 亿 token（提醒线 1 亿）",
            "这个会话已连续活跃 3 天 2 小时（提醒线 3 天）",
        ])
        XCTAssertTrue(LiveRunaway.alerts(stats: SessionTranscriptStats(inputTokens: 1_000), history: [],
                                         delegations: [], nowMs: 5_000).isEmpty)
    }

    func testAmountsReadInWanAndYi() {
        XCTAssertEqual(LiveRunaway.amount(900), "900")
        XCTAssertEqual(LiveRunaway.amount(218_600), "21.9 万")
        XCTAssertEqual(LiveRunaway.amount(400_000_000), "4 亿")
    }
}
