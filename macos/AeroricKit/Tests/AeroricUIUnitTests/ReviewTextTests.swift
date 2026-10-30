import XCTest
@testable import AeroricProtocol
@testable import AeroricUI

/// B4-1 (2026-10-30): the review card's and the progress page's wording.
final class ReviewTextTests: XCTestCase {
    private func card(_ state: String, decisions: [String?] = ["reject", nil]) -> LiveReviewCard {
        LiveReviewCard(reviewId: "rv-1", round: 2, state: state, turnId: "u2", reviewerTaskId: "task-9",
                       reviewerLabel: "Claude Code · sonnet",
                       findings: [LiveReviewFinding(id: "r1", severity: "blocker", text: "重试没有上限",
                                                    file: "client.rs", line: 88, decision: decisions[0],
                                                    reason: "调用方已限流"),
                                  LiveReviewFinding(id: "r2", severity: "minor", text: "命名", decision: decisions[1])])
    }

    func testAStateNamesWhatHappensNext() {
        XCTAssertEqual(ReviewText.state(card("waitingCoder")).0, "等写代码的会话答复")
        XCTAssertEqual(ReviewText.state(card("settled")).0, "已结束 · 阻断被驳回，请看一眼")
        XCTAssertEqual(ReviewText.state(card("settled", decisions: ["adopt", nil])).0, "已结束")
        XCTAssertTrue(card("settled").needsUser)
        XCTAssertTrue(card("escalated", decisions: ["adopt", nil]).needsUser)
        XCTAssertFalse(card("passed", decisions: [nil, nil]).needsUser)
        XCTAssertTrue(card("reviewing").isOpen)
    }

    func testAFindingReadsAsOneLine() {
        let finding = card("settled").findings[0]
        XCTAssertEqual(ReviewText.line(finding), "client.rs:88 重试没有上限")
        XCTAssertEqual(ReviewText.severity("blocker").0, "阻断")
        XCTAssertEqual(ReviewText.decision(finding)?.0, "驳回 · 调用方已限流")
        XCTAssertNil(ReviewText.decision(card("settled").findings[1]))
        XCTAssertEqual(ReviewText.summary(card("escalated", decisions: ["ask", "adopt"])),
                       "第 2 轮 · 需要你处理 · 2 条 · 1 条需要你定")
    }

    func testThePickerSaysWhoAutoPicks() {
        let claude = LiveReviewerCandidate(agent: "claude", label: "Claude Code", sameFamily: false)
        var model = ReviewPanelModel(settings: LiveReviewSettings(enabled: true), candidates: [claude], auto: claude)
        XCTAssertEqual(model.reviewerTitle, "自动 · Claude Code")
        model.auto = LiveReviewerCandidate(agent: "codex", label: "Codex", sameFamily: true)
        XCTAssertEqual(model.reviewerTitle, "自动 · Codex（同族）")
        XCTAssertTrue(model.reviewerHint.hasPrefix("本机只有同族可用"))
        model.settings?.reviewer = .named(agent: "claude", model: "opus")
        XCTAssertEqual(model.reviewerTitle, "Claude Code · opus")
        model.auto = nil
        model.settings?.reviewer = .auto
        XCTAssertEqual(model.reviewerTitle, "自动 · 本机没有可用的审查者")
    }

    func testSettingsAndCardsDecodeFromTheWire() {
        let settings = LiveReviewSettings.decode(.object(["enabled": .bool(true),
                                                          "reviewer": .object(["agent": .string("codex")]),
                                                          "paused": .bool(false)]))
        XCTAssertEqual(settings, LiveReviewSettings(enabled: true, reviewer: .named(agent: "codex")))
        XCTAssertEqual(LiveReviewSettings.decode(.object(["reviewer": .string("auto")]))?.reviewer, .auto)
        XCTAssertEqual(LiveReviewerChoice.named(agent: "claude", model: "opus").json,
                       .object(["agent": .string("claude"), "model": .string("opus")]))
        XCTAssertNil(LiveReviewCard.decode(.object(["state": .string("passed")])))
    }
}
