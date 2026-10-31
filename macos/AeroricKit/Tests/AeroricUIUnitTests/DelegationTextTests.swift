import XCTest
@testable import AeroricProtocol
@testable import AeroricUI

/// B4-2 (2026-10-31): the delegation card's, the `#` prefix's and the
/// progress page's wording and parsing.
final class DelegationTextTests: XCTestCase {
    private let roles = [
        LiveDelegationRole(id: "researcher", name: "调研员", description: "只读调研", contextMode: "isolated"),
        LiveDelegationRole(id: "implementer", name: "实现者", permission: "auto_edit", workspace: "shared"),
    ]

    func testTheHashPrefixNamesARoleOrAHarness() {
        XCTAssertEqual(DelegationText.hashCommand("#调研员 查 reqwest 超时")?.target, "调研员")
        XCTAssertEqual(DelegationText.hashCommand("#codex  改一下\n第二行")?.instruction, "改一下\n第二行")
        XCTAssertNil(DelegationText.hashCommand("#调研员"))
        XCTAssertNil(DelegationText.hashCommand("## 标题 文字"))
        XCTAssertNil(DelegationText.hashCommand("普通消息 #调研员 x"))
        XCTAssertEqual(DelegationText.agent(for: "Claude-Code"), .named(agent: "claude"))
        XCTAssertNil(DelegationText.agent(for: "调研员"))
        XCTAssertEqual(DelegationText.suggestions("#", roles: roles), ["调研员", "实现者", "claude-code", "codex"])
        XCTAssertEqual(DelegationText.suggestions("#co", roles: roles), ["codex"])
        XCTAssertEqual(DelegationText.suggestions("#调研员 x", roles: roles), [])
    }

    func testACardSaysWhoWhereAndHowItEnded() {
        var card = LiveDelegationCard(
            delegationId: "dg-1", childTaskId: "task-4", origin: "session", roleName: "调研员",
            harnessLabel: "Codex",
            contract: LiveDelegationContract(instruction: "查", contextMode: "isolated", permission: "plan",
                                             workspace: "readOnly"),
            state: "reported", result: "结论\n第二行",
            acceptance: [LiveDelegationAcceptance(id: "a1", text: "给出示例", state: "passed")])
        XCTAssertEqual(DelegationText.title(card), "委派 · 调研员")
        XCTAssertEqual(DelegationText.contract(card), "Codex · 隔离 · 只读 · 会话发起")
        XCTAssertEqual(DelegationText.state(card).0, "已回报")
        XCTAssertEqual(DelegationText.summary(card), "委派 · 调研员 · 已回报 · 验收 1/1")
        XCTAssertEqual(DelegationText.firstLine(card.result ?? ""), "结论")
        XCTAssertEqual(DelegationText.footnote(card), "没有可核对的证据 · 没有验收条款，建议继续拆分")
        XCTAssertTrue(card.canSend)
        card.state = "overBudget"
        card.error = "超过 30 分钟"
        XCTAssertTrue(card.needsUser)
        XCTAssertEqual(DelegationText.state(card).0, "超出预算")
        XCTAssertTrue(DelegationText.footnote(card)?.hasPrefix("超过 30 分钟") == true)
        card.state = "running"
        XCTAssertTrue(card.isOpen)
        XCTAssertFalse(card.canSend)
        XCTAssertEqual(DelegationText.workspace("shared", permission: "auto_edit"), "可写 · 自动编辑")
    }

    func testTrackRecordsAndAuditReadAsOneLine() {
        let row = LiveReputationRow(agent: "codex", model: "gpt-5", role: "researcher", total: 5, reported: 4,
                                    failed: 1, accepted: 2, withAcceptance: 3, reworks: 1)
        XCTAssertEqual(DelegationText.reputation(row, roles: roles),
                       "Codex · gpt-5 · 调研员：5 次，回报 4，验收 2/3，退回 1，失败 1")
        let model = DelegationPanelModel(stats: [row, LiveReputationRow(agent: "claude", total: 9)])
        XCTAssertEqual(model.reputation(role: "researcher", agent: nil).count, 1)
        XCTAssertEqual(model.reputation(role: nil, agent: nil).first?.agent, "claude")
        XCTAssertEqual(DelegationText.audit(LiveDelegationAudit(atMs: 1, actor: "user", action: "refuse",
                                                                permission: "full_access", detail: "权限过高")),
                       "你被拒绝 · 完全访问 · 权限过高")
        XCTAssertEqual(RoleText.line(roles[0]), "同族 · 隔离 · 只读 · 只读调研")
    }

    /// B4-3: probes, board rows and the attention line.
    func testProbesAndBoardRowsReadAsOneLine() {
        var card = LiveDelegationCard(delegationId: "dg-1", state: "running",
                                      probes: [LiveDelegationProbe(kind: "repeat", detail: "同一个调用重复了 3 次：ls")])
        XCTAssertTrue(card.needsUser)
        XCTAssertEqual(BoardText.probes(card.probes), "同一个调用重复了 3 次：ls")
        card.probes[0].acknowledged = true
        XCTAssertFalse(card.needsUser)
        XCTAssertNil(BoardText.probes(card.probes))
        let entries = [
            LiveBoardEntry(id: "b1", kind: "evidence", text: "日志", state: "open"),
            LiveBoardEntry(id: "b2", kind: "decision", text: "用 connect_timeout", state: "proposed", author: "task-4"),
            LiveBoardEntry(id: "b3", kind: "goal", text: "修超时", state: "open"),
            LiveBoardEntry(id: "b4", kind: "claim", text: "查 hyper", state: "open", claimedBy: "task-5"),
        ]
        XCTAssertEqual(BoardText.ordered(entries).map(\.id), ["b3", "b2", "b1", "b4"])
        XCTAssertEqual(BoardText.line(entries[3]), "认领 · 查 hyper（task-5）")
        XCTAssertEqual(BoardText.next(entries[3]), ["done"])
        XCTAssertNil(BoardText.next(entries[1]))
        XCTAssertEqual(BoardText.state("proposed").0, "提议")
        XCTAssertEqual(BoardText.author("task-4"), "会话 task-4 写的")
        XCTAssertEqual(BoardText.attention(LiveBoardAttention(ownerTaskId: "t1", entry: entries[1], reason: "decision")),
                       "决定待确认：用 connect_timeout")
        let board = LiveBoard.decode(.object(["ownerTaskId": .string("t1"), "revision": .number("3"),
                                              "entries": .array([.object(["id": .string("b1"), "kind": .string("goal"),
                                                                          "text": .string("修超时"), "state": .string("open"),
                                                                          "author": .string("user")])])]))
        XCTAssertEqual(board?.revision, 3)
        XCTAssertEqual(board?.entries.first?.text, "修超时")
    }

    func testADraftBecomesAContract() {
        var draft = DelegationDraft()
        XCTAssertFalse(draft.ready)
        draft.instruction = "  查超时 "
        draft.acceptance = "给出示例"
        draft.check = "cargo test"
        draft.files = "src/a.rs, src/b.rs，src/c.rs"
        draft.minutes = "10"
        let contract = draft.contract
        XCTAssertEqual(contract.instruction, "查超时")
        XCTAssertEqual(contract.contextRefs, ["src/a.rs", "src/b.rs", "src/c.rs"])
        XCTAssertEqual(contract.acceptance, [LiveAcceptanceItem(text: "给出示例", check: "cargo test")])
        XCTAssertEqual(contract.budgetMinutes, 10)
        XCTAssertEqual(contract.json.fields["budget"]?.fields["minutes"]?.numberText, "10")
        XCTAssertNil(contract.json.fields["role"])
    }

    /// CU F3: the form sets a token budget too.
    func testTheDraftCarriesATokenBudget() {
        XCTAssertEqual(DelegationDraft.tokens("200k"), 200_000)
        XCTAssertEqual(DelegationDraft.tokens(" 1.5M "), 1_500_000)
        XCTAssertEqual(DelegationDraft.tokens("120,000"), 120_000)
        XCTAssertNil(DelegationDraft.tokens(""))
        XCTAssertNil(DelegationDraft.tokens("很多"))
        var draft = DelegationDraft()
        draft.instruction = "查"
        draft.tokens = "50k"
        XCTAssertEqual(draft.contract.budgetTokens, 50_000)
        XCTAssertEqual(draft.contract.json.fields["budget"]?.fields["tokens"]?.numberText, "50000")
    }
}
