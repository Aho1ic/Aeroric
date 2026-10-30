import XCTest
@testable import AeroricProtocol
@testable import AeroricUI

/// B4′ (2026-10-30): the progress page's and the handoff card's wording.
final class CheckpointTextTests: XCTestCase {
    private func checkpoint() -> LiveCheckpoint {
        LiveCheckpoint(
            taskId: "t1", revision: 3, turn: 2, updatedMs: 1_761_350_400_000,
            goal: LiveCheckpointEntry(text: "修复超时", status: "verified"),
            unknown: [LiveEffect(summary: "cargo test", kind: "command", turn: 2, status: "unknown")],
            intents: [LiveIntent(requestId: "p1", summary: "git push", verify: "看退出码", state: "outcomeUnknown", turn: 2),
                      LiveIntent(requestId: "p2", summary: "rm -rf", verify: "看退出码", state: "denied", turn: 2)],
            obligations: [LiveObligation(id: "a1", text: "测试通过", check: "cargo test", state: "open", source: "acceptance"),
                          LiveObligation(id: "a2", text: "文档", state: "passed", source: "acceptance")]
        )
    }

    func testAttentionCountsUnknownsAndOpenWork() {
        XCTAssertEqual(CheckpointText.attention(checkpoint()), 3)
        XCTAssertTrue(checkpoint().needsAttention)
        XCTAssertFalse(LiveCheckpoint(taskId: "t1").needsAttention)
    }

    func testBadgesNameWhereAFactCameFrom() {
        XCTAssertEqual(EpistemicBadge.label("verified"), "已验证")
        XCTAssertEqual(EpistemicBadge.label("unknown"), "未知")
        XCTAssertEqual(EpistemicBadge.label("inferred"), "推断")
        XCTAssertEqual(CheckpointText.intent("denied").0, "已拒绝")
        XCTAssertEqual(CheckpointText.intent("outcomeUnknown").0, "结果未知")
        XCTAssertEqual(CheckpointText.obligation("passed").1, "检查通过")
        XCTAssertEqual(CheckpointText.reason("sourceMove"), "换来源")
    }

    func testHeaderSaysWhenAndHowFar() {
        XCTAssertEqual(CheckpointPanelText.header(nil), "正在读取检查点…")
        XCTAssertEqual(CheckpointPanelText.header(LiveCheckpoint(taskId: "t1")), "检查点 · 尚未开始")
        let header = CheckpointPanelText.header(checkpoint())
        XCTAssertTrue(header.hasPrefix("检查点 · 回合 2 结束 · "), header)
        XCTAssertFalse(header.contains("\n"))
    }

    func testABlankCheckpointShowsTheHintNotEmptySections() {
        XCTAssertTrue(LiveCheckpoint(taskId: "t1", revision: 4, turn: 1, updatedMs: 1).isBlank)
        XCTAssertFalse(checkpoint().isBlank)
    }
}
