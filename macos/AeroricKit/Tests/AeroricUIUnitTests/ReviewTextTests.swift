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

    /// 2026-11-22 (目检 C1 没测成): an Agent configuration can review.
    func testAnAgentConfigurationCanBeTheReviewer() {
        let magpie = LiveReviewerConfiguration.decode(.object([
            "agentConfigId": .string("magpie"), "title": .string("Magpie"), "agent": .string("claude"),
            "models": .array([.string("42/deepseek-flash"), .string("42/deepseek-pro")]), "sameFamily": .bool(true),
        ]))!
        XCTAssertEqual(magpie.models, ["42/deepseek-flash", "42/deepseek-pro"])
        XCTAssertEqual(ReviewPanelModel.configurationTitle(magpie), "Magpie（同族）")
        let choice = ReviewPanelModel.choice(magpie)
        XCTAssertEqual(choice, .named(agent: "claude", configID: "magpie", model: "42/deepseek-flash"))
        XCTAssertEqual(choice.json, .object(["agent": .string("claude"), "agentConfigId": .string("magpie"),
                                             "model": .string("42/deepseek-flash")]))

        var model = ReviewPanelModel(settings: LiveReviewSettings(enabled: true, reviewer: choice))
        model.configurations = [magpie]
        XCTAssertEqual(model.reviewerTitle, "Magpie · 42/deepseek-flash")
        XCTAssertFalse(model.reviewerUnavailable)
        // The configuration is gone (or blocked): the picker and the hint say so.
        model.configurations = []
        XCTAssertEqual(model.reviewerTitle, "配置 magpie · 现在不能用")
        XCTAssertTrue(model.reviewerUnavailable)
        XCTAssertTrue(model.reviewerHint.contains("下一次审查会失败"))
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

    /// E2 (2026-11-26): the fact check's line on the card, and each
    /// finding it took out with its ground; settings read `factCheck`.
    func testTheFactCheckSaysWhatItTookOut() throws {
        let card = try XCTUnwrap(LiveReviewCard.decode(.object([
            "reviewId": .string("rv-1"), "state": .string("passed"),
            "factCheck": .object([
                "state": .string("done"), "model": .string("deepseek-flash"), "guarded": .number("1"),
                "filtered": .array([
                    .object(["finding": .object(["id": .string("r1"), "severity": .string("blocker"),
                                                 "file": .string("src/a.rs"), "line": .number("3"),
                                                 "text": .string("重试没有上限")]),
                             "ground": .string("refuted"), "evidence": .string("if attempt >= MAX_RETRIES {")]),
                    .object(["finding": .object(["id": .string("r2"), "text": .string("吞掉了错误")]),
                             "ground": .string("notInDiff"), "evidence": .string("引用的代码不在这次的改动里")]),
                ]),
            ]),
        ])))
        let check = try XCTUnwrap(card.factCheck)
        XCTAssertEqual(check.filtered.map(\.id), ["r1", "r2"])
        XCTAssertEqual(ReviewText.factCheck(check)?.0, "已过滤 2 条被 diff 直接驳倒的意见")
        XCTAssertEqual(ReviewText.filteredGround(check.filtered[0]), "被这一行驳倒：if attempt >= MAX_RETRIES {")
        XCTAssertEqual(ReviewText.filteredGround(check.filtered[1]), "引用的代码不在这次的改动里")
        XCTAssertTrue(ReviewText.factCheckHelp(check).contains("另有 1 条模型想删"))
        let failed = LiveFactCheck(state: "failed", reason: "核查没跑成：HTTP 401；意见全部保留")
        XCTAssertEqual(ReviewText.factCheck(failed)?.0, "核查没跑成：HTTP 401；意见全部保留")
        XCTAssertEqual(ReviewText.factCheck(LiveFactCheck(state: "done"))?.0, "核查过了，没有过滤意见")
        XCTAssertEqual(LiveReviewSettings.decode(.object(["enabled": .bool(true), "factCheck": .bool(true)]))?.factCheck, true)
        XCTAssertEqual(LiveReviewSettings.decode(.object(["enabled": .bool(true)]))?.factCheck, false)
    }

    /// E3: round 2 says what is new, what persists, what was resolved and
    /// what nobody looked at again.
    func testTheSecondRoundComparesWithTheFirst() throws {
        func finding(_ id: String, _ file: String, _ text: String, repeats: String? = nil) -> JSONValue {
            var fields: [String: JSONValue] = ["id": .string(id), "severity": .string("blocker"),
                                               "file": .string(file), "text": .string(text)]
            if let repeats { fields["repeats"] = .string(repeats) }
            return .object(fields)
        }
        let card = try XCTUnwrap(LiveReviewCard.decode(.object([
            "reviewId": .string("rv-1"), "round": .number("2"), "state": .string("waitingCoder"),
            "findings": .array([finding("r1", "src/a.rs", "重试没有上限", repeats: "r1"),
                                finding("r2", "src/a.rs", "日志太多")]),
            "compare": .object([
                "previousRound": .number("1"),
                "resolved": .array([finding("r3", "src/a.rs", "忙等")]),
                "unchecked": .array([finding("r2", "src/b.rs", "会崩")]),
            ]),
        ])))
        let compare = try XCTUnwrap(card.compare)
        XCTAssertEqual(card.findings.first?.repeats, "r1")
        XCTAssertEqual(ReviewText.compare(card), "新增 1 · 仍在 1 · 已解决 1 · 未复查 1")
        XCTAssertEqual(ReviewText.earlier(compare), ["已解决 · src/a.rs 忙等",
                                                     "未复查（这一轮没人再看这个文件） · src/b.rs 会崩"])
        XCTAssertEqual(ReviewText.notes(card.findings[0]), ["上一轮也提过（当时是 r1），还没改好"])
        XCTAssertEqual(ReviewText.summary(card), "第 2 轮 · 等写代码的会话答复 · 2 条 · 1 条未复查")

        var first = card
        first.compare = nil
        XCTAssertNil(ReviewText.compare(first), "round 1 has nothing to compare")
    }

    /// E4: the progress page lists the rules in force and where they come from.
    func testThePanelListsTheReviewRulesInForce() throws {
        func rule(_ id: String, _ source: String, _ paths: [String], _ severity: String,
                  disabled: Bool = false) -> JSONValue {
            .object(["id": .string(id), "source": .string(source), "paths": .array(paths.map(JSONValue.string)),
                     "severity": .string(severity), "text": .string("说明"), "disabled": .bool(disabled)])
        }
        let rules = try XCTUnwrap(LiveReviewRules.decode(.object([
            "file": .string(".aeroric/review-rules.json"), "state": .string("loaded"),
            "rules": .array([rule("rust", "builtin", ["*.rs"], "major"),
                             rule("shell", "builtin", ["*.{sh,bash}"], "major", disabled: true),
                             rule("money", "project", ["src/billing/**"], "blocker")]),
        ])))
        var model = ReviewPanelModel(settings: LiveReviewSettings(enabled: true))
        XCTAssertNil(model.rulesTitle, "not read yet")
        model.rules = rules
        XCTAssertEqual(model.rulesTitle, "审查规则 · 内置 1 条 · 项目 1 条（关了 1 条内置）")
        XCTAssertEqual(model.rulesHint, "项目规则来自 .aeroric/review-rules.json；审查时按改动前的版本")
        XCTAssertEqual(rules.rules.map(ReviewPanelModel.ruleLine), [
            "rust · 重要 · *.rs",
            "shell · 重要 · *.{sh,bash}（已关闭）",
            "money · 阻断 · src/billing/**（项目）",
        ])
        model.rules?.state = "invalid"
        model.rules?.error = "规则文件读不懂"
        XCTAssertEqual(model.rulesHint, "规则文件读不懂，只用了内置规则：规则文件读不懂")

        var finding = LiveReviewFinding(id: "r1", severity: "blocker", text: "用了浮点")
        finding.rule = "money"
        XCTAssertEqual(ReviewText.notes(finding), ["依据规则：money"])
    }
}
