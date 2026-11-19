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

    /// E1: the card says what the round rests on, file by file.
    func testTheCardSaysWhichFilesTheReviewerAccountedFor() throws {
        let decoded = try XCTUnwrap(LiveReviewCard.decode(.object([
            "reviewId": .string("rv-1"), "state": .string("passed"),
            "coverage": .object([
                "state": .string("partial"),
                "files": .array([
                    .object(["path": .string("src/a.rs"), "state": .string("reviewed")]),
                    .object(["path": .string("src/b.rs"), "state": .string("seen")]),
                    .object(["path": .string("src/c.rs"), "state": .string("missed")]),
                    .object(["path": .string("gen.rs"), "state": .string("skipped"), "reason": .string("生成的代码")]),
                    .object(["path": .string(".env"), "state": .string("excluded"),
                             "reason": .string("可能含有凭据，没有发给审查者")]),
                ]),
            ]),
        ])))
        let coverage = try XCTUnwrap(decoded.coverage)
        XCTAssertEqual(coverage.inScope.count, 4)
        XCTAssertEqual(coverage.looked, 2)
        XCTAssertEqual(ReviewText.coverage(coverage)?.0, "审查覆盖 2/4 个文件（结果可能不完整）")
        XCTAssertEqual(coverage.files.map(ReviewText.fileLine), [
            "src/a.rs · 已审",
            "src/b.rs · 看过（审查者没有逐个交代）",
            "src/c.rs · 没看",
            "gen.rs · 跳过：生成的代码",
            ".env · 不在范围：可能含有凭据，没有发给审查者",
        ])
        XCTAssertEqual(ReviewText.summary(decoded), "第 1 轮 · 通过 · 覆盖 2/4")

        let none = LiveReviewCoverage(state: "unreviewed", files: [.init(path: "a", state: "missed")])
        XCTAssertEqual(ReviewText.coverage(none)?.0, "没有审查：1 个改动的文件一个都没看")
        XCTAssertNil(LiveReviewCard.decode(.object(["reviewId": .string("rv-2"), "state": .string("passed")]))?.coverage)
    }

    /// E2: a finding shows its code, where it was moved from, and what it folded.
    func testAFindingSaysHowItsPlaceWasSettled() throws {
        let card = try XCTUnwrap(LiveReviewCard.decode(.object([
            "reviewId": .string("rv-1"), "state": .string("waitingCoder"),
            "findings": .array([.object([
                "id": .string("r1"), "severity": .string("blocker"), "text": .string("重试没有上限"),
                "file": .string("src/a.rs"), "line": .number("4"), "snippet": .string("retry();"),
                "category": .string("bug"), "movedFrom": .number("1"), "merged": .number("2"),
            ])]),
        ])))
        let finding = try XCTUnwrap(card.findings.first)
        XCTAssertEqual(finding.snippet, "retry();")
        XCTAssertEqual(ReviewText.line(finding), "src/a.rs:4 重试没有上限（3 条相同）")
        XCTAssertEqual(ReviewText.notes(finding), ["位置已按代码修正（审查者写的是第 1 行）", "合并了 2 条说同一处的意见"])
        var lost = finding
        lost.movedFrom = nil
        lost.merged = 0
        lost.unlocated = true
        XCTAssertEqual(ReviewText.notes(lost), ["引用的代码在文件里找不到，位置以审查者写的为准"])
    }
}
