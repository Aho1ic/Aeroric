@testable import AeroricUI
import AeroricProtocol
import XCTest

/// R5-2: the comparison read back from `rerun.list`, and its words.
final class RerunTests: XCTestCase {
    private func groups() -> [LiveRerunGroup] {
        LiveRerunGroup.decodeList(.array([.object([
            "group": .object(["id": .string("g1"), "turnId": .string("turn-3"), "message": .string("修导出"),
                              "withBrief": .bool(false), "createdMs": .number("1000")]),
            "original": .object(["outcome": .string("completed"), "files": .number("2"), "additions": .number("18"),
                                 "deletions": .number("4"), "inputTokens": .number("12300"),
                                 "outputTokens": .number("845"), "durationMs": .number("54000"),
                                 "tools": .number("6"),
                                 "check": .object(["passed": .bool(false), "summary": .string("测试 1 项失败")])]),
            "runs": .array([
                .object(["run": .object(["taskId": .string("rerun-a"), "label": .string("Codex · gpt-6.1-sol")]),
                         "result": .object(["outcome": .string("running"), "files": .number("0")])]),
                .object(["run": .object(["taskId": .string("rerun-b"), "label": .string("Claude Code · opus")]),
                         "result": .object(["outcome": .string("notRun"),
                                            "notRun": .object(["reason": .string("quota"), "status": .number("429"),
                                                               "message": .string("rate limited")])])]),
                .object(["run": .object(["taskId": .string("rerun-c"), "label": .string("gone")]),
                         "gone": .bool(true)]),
                .object(["run": .object(["taskId": .string("rerun-d"), "label": .string("dropped")]), "copy": .bool(false),
                         "result": .object(["outcome": .string("completed")])]),
            ]),
        ])]))
    }

    func testTheComparisonDecodes() throws {
        let group = try XCTUnwrap(groups().first)
        XCTAssertEqual(group.turnId, "turn-3")
        XCTAssertFalse(group.withBrief)
        XCTAssertEqual(group.runs.map(\.taskId), ["rerun-a", "rerun-b", "rerun-c", "rerun-d"])
        XCTAssertEqual(group.original.checkPassed, false)
        XCTAssertEqual(group.runs[1].result.notRun?.status, 429)
        XCTAssertTrue(group.runs[2].gone)
        XCTAssertTrue(group.runs[0].copy, "a run without the field still has its copy")
        XCTAssertFalse(group.runs[3].copy)
        XCTAssertEqual(RerunText.outcome(LiveRerunColumn(outcome: "replaced")), "重跑后已撤销")
        XCTAssertTrue(RerunText.anyRunning([group]))
    }

    func testTheColumnsAreWorded() throws {
        let group = try XCTUnwrap(groups().first)
        let original = group.original
        XCTAssertEqual(RerunText.outcome(original), "完成")
        XCTAssertEqual(RerunText.changes(original), "2 个文件 +18 −4")
        XCTAssertEqual(RerunText.check(original), "检查没过：测试 1 项失败")
        XCTAssertEqual(RerunText.usage(original), "输入 12.3K · 输出 845 tok")
        XCTAssertEqual(RerunText.effort(original), "用时 54 秒 · 6 步工具")
        let notRun = group.runs[1].result
        XCTAssertEqual(RerunText.outcome(notRun), "没跑成 · 额度或限流")
        XCTAssertEqual(RerunText.tone(notRun), .danger)
        XCTAssertEqual(RerunText.changes(notRun), "没改文件")
        XCTAssertEqual(RerunText.check(notRun), "没跑检查")
        XCTAssertEqual(RerunText.outcome(group.runs[0].result), "运行中")
        XCTAssertEqual(RerunText.started(LiveRerunStarted(runs: 2, refused: ["Claude Code：未登录"])),
                       "已开始 2 个重跑，各在自己的独立副本里；没开成：Claude Code：未登录")
        XCTAssertEqual(RerunText.broughtBack((3, 1), label: "Codex"),
                       "已带回 Codex 的 3 处，1 处冲突，去这个重跑的改动页处理")
    }

    /// 目检 2026-11-12: every harness has a 「全部第三方模型」; the sheet listed
    /// four rows of the same name. The session's own harness comes first.
    func testRepeatedNamesNameTheirHarness() {
        let targets = [
            RerunTarget(id: "claude", title: "Claude Code", agent: "claude", models: ["opus"]),
            RerunTarget(id: "agent-config:via-codex-all", title: "全部第三方模型", agent: "codex", models: []),
            RerunTarget(id: "agent-config:via-claude-all", title: "全部第三方模型", agent: "claude", models: []),
            RerunTarget(id: "codex", title: "Codex", agent: "codex", models: ["gpt-6.1-sol"]),
        ]
        let ordered = RerunText.ordered(targets, agent: "codex")
        XCTAssertEqual(ordered.map(\.id), ["agent-config:via-codex-all", "codex", "claude", "agent-config:via-claude-all"])
        XCTAssertEqual(ordered.map { RerunText.title($0, among: ordered) },
                       ["全部第三方模型 · Codex", "Codex", "Claude Code", "全部第三方模型 · Claude Code"])
    }
}
