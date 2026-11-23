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

    /// B3: a criterion says what its verdict rests on; a stale pass reads
    /// "需要重新确认" and does not count as passed.
    func testACriterionSaysWhatItRestsOnAndAStalePassIsNotPassed() throws {
        let card = try XCTUnwrap(LiveDelegationCard.decode(.object([
            "delegationId": .string("dg-1"), "state": .string("reported"),
            "acceptance": .array([
                .object(["id": .string("a1"), "text": .string("cargo test 通过"), "state": .string("passed"),
                         "evidence": .array([.object(["kind": .string("autoCheck"), "seq": .number("7"),
                                                      "turn": .number("2"), "detail": .string("u1")])]),
                         "checkedRevision": .string("0123456789abcdef"), "stale": .bool(true)]),
                .object(["id": .string("a2"), "text": .string("文档写好"), "state": .string("passed"),
                         "evidence": .array([.object(["kind": .string("userConfirmation")])])]),
                .object(["id": .string("a3"), "text": .string("旧数据"), "state": .string("passed")]),
                .object(["id": .string("a4"), "text": .string("还没跑"), "state": .string("open")]),
            ]),
        ])))
        let (a1, a2, a3, a4) = (card.acceptance[0], card.acceptance[1], card.acceptance[2], card.acceptance[3])
        XCTAssertEqual(DelegationText.acceptance(a1).0, "需要重新确认")
        XCTAssertEqual(DelegationText.basis(a1), "依据：自动检查 · 第 2 轮")
        XCTAssertTrue(DelegationText.basisDetail(a1).contains("0123456789"))
        XCTAssertTrue(DelegationText.basisDetail(a1).contains("不再算通过"))
        XCTAssertEqual(DelegationText.acceptance(a2).0, "通过")
        XCTAssertEqual(DelegationText.basis(a2), "依据：你确认的")
        XCTAssertEqual(DelegationText.basis(a3), "依据未记录", "old data: not invented")
        XCTAssertNil(DelegationText.basis(a4))
        XCTAssertEqual(card.passedCount, 2, "the stale pass is not counted")
    }

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

    /// 2026-11-23: a delegation can go to an Agent configuration and one of
    /// its models, as reviewers can; the picker says which, or that it is gone.
    func testTheFormCanSendADelegationToAnAgentConfiguration() {
        var model = DelegationPanelModel()
        let magpie = LiveReviewerConfiguration(agentConfigId: "magpie", title: "Magpie", agent: "codex",
                                               models: ["gpt-6.2-sol", "gpt-6.2-mini"], sameFamily: true)
        model.configurations = [magpie]

        let first = DelegationPanelModel.pick(magpie)
        XCTAssertEqual(first, .named(agent: "codex", agentConfigId: "magpie", model: "gpt-6.2-sol"))
        XCTAssertEqual(first.json, .object(["agent": .string("codex"), "agentConfigId": .string("magpie"),
                                            "model": .string("gpt-6.2-sol")]))
        XCTAssertEqual(model.agentTitle(DelegationPanelModel.pick(magpie, model: "gpt-6.2-mini"), hasRole: false),
                       "Magpie · gpt-6.2-mini")
        XCTAssertEqual(model.agentTitle(.named(agent: "claude", agentConfigId: "gone", model: nil), hasRole: false),
                       "配置 gone（现在不能用）")
        XCTAssertEqual(model.agentTitle(.named(agent: "codex"), hasRole: false), DelegationText.agentLabel("codex"))
        XCTAssertEqual(model.agentTitle(nil, hasRole: true), "按角色")
        XCTAssertEqual(model.agentTitle(nil, hasRole: false), "同族")
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
        // R2-3b added the isolated copy, so the shared one says so.
        XCTAssertEqual(DelegationText.workspace("shared", permission: "auto_edit"), "共享可写 · 自动编辑")
    }

    /// Review 2026-11-23 (1123b): 通过 / 未通过 on the card once the child
    /// reported, for a criterion not passed or whose pass went stale.
    func testTheCardOffersToSettleOpenCriteriaAfterTheReport() {
        var item = LiveDelegationAcceptance(id: "a1", text: "test -f g.md", state: "open")
        var card = LiveDelegationCard(delegationId: "dg-1", childTaskId: "child", origin: "user",
                                      harnessLabel: "Codex", state: "reported", acceptance: [item])
        XCTAssertTrue(DelegationText.canSettle(card, item))
        item.state = "failed"
        XCTAssertTrue(DelegationText.canSettle(card, item), "a failed check can be overruled, as in 手动核销")
        item.state = "passed"
        XCTAssertFalse(DelegationText.canSettle(card, item))
        item.stale = true
        XCTAssertTrue(DelegationText.canSettle(card, item), "a stale pass asks to be confirmed again")
        card.state = "running"
        XCTAssertFalse(DelegationText.canSettle(card, item), "not while the child still works")
        card.state = "reported"
        card.childTaskId = nil
        XCTAssertFalse(DelegationText.canSettle(card, item), "nothing to mark without the child")
    }

    /// A1: the badge follows the delivery; only a delivered one says 已送达.
    func testAResultCardSaysWhereItsResultStands() {
        var card = LiveDelegationCard(delegationId: "dg-1", origin: "user", harnessLabel: "Codex", state: "reported",
                                      sent: true)
        XCTAssertEqual(DelegationText.state(card).0, "已回报 · 已发送（没有投递记录）")
        XCTAssertFalse(card.canSend, "an old card that went is not offered again")
        // 2026-11-17: an expert's answer is taken by `ask_expert`, never queued.
        var expert = LiveDelegationCard(delegationId: "dg-2", origin: "session", roleName: "专家", harnessLabel: "Codex",
                                        contract: LiveDelegationContract(instruction: "问", role: "expert"),
                                        state: "failed", sent: true)
        XCTAssertEqual(DelegationText.state(expert).0, "失败 · 主会话已取走")
        expert.state = "reported"
        XCTAssertEqual(DelegationText.state(expert).0, "已回报 · 主会话已取走")
        expert.sent = false
        XCTAssertEqual(DelegationText.state(expert).0, "已回报")
        let words: [(String, String)] = [
            ("waiting", "已入队"), ("sending", "正在发送"), ("accepted", "对方已收下"), ("delivered", "已送达"),
            ("failed", "没送到"), ("unknown", "结果不明"), ("paused", "已暂停"), ("withdrawn", "已撤回"),
        ]
        for (state, word) in words {
            card.returnDelivery = LiveDelegationReturn(id: "dv-1", state: state, reason: "会话打不开")
            XCTAssertEqual(DelegationText.state(card).0, "已回报 · \(word)", state)
            XCTAssertEqual(card.canSend, ["failed", "paused", "unknown", "withdrawn"].contains(state), state)
            XCTAssertEqual(card.needsUser, state == "unknown", state)
        }
        card.returnDelivery = LiveDelegationReturn(id: "dv-1", state: "failed", reason: "会话打不开")
        XCTAssertTrue(DelegationText.footnote(card)?.hasPrefix("没送到：会话打不开") == true)
        card.returnDelivery = LiveDelegationReturn(id: "dv-1", state: "unknown", reason: "读不到会话记录")
        XCTAssertTrue(DelegationText.footnote(card)?.contains("先去主会话核对") == true)
        card.state = "failed"
        card.returnDelivery = LiveDelegationReturn(id: "dv-1", state: "delivered", evidence: "protocol")
        XCTAssertEqual(DelegationText.state(card).0, "失败 · 已送达")
        card.state = "reported"
        card.sent = false
        card.returnDelivery = nil
        XCTAssertEqual(DelegationText.state(card).0, "已回报", "nothing handed over yet")
        XCTAssertTrue(card.canSend)
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

    /// H5: native subagents sit beside the delegations, background commands do not;
    /// a board line written by a subagent names it.
    func testNativeSubagentsAndTheirBoardWritesAreNamed() {
        let model = DelegationPanelModel().with(native: [
            LiveSubagent(id: "t1", title: "检索", status: "running", detail: "读取 src/main.rs"),
            LiveSubagent(id: "t2", title: "\(LiveSubagent.backgroundCommandPrefix)npm test", status: "running"),
            LiveSubagent(id: "t3", title: "子代理 worker", status: "completed", detail: "最后一步"),
        ])
        XCTAssertEqual(model.native.map(\.id), ["t1", "t3"])
        XCTAssertEqual(DelegationText.native(model.native[0]), "检索 · 进行中 · 读取 src/main.rs")
        XCTAssertEqual(DelegationText.native(model.native[1]), "子代理 worker · 已完成")
        XCTAssertEqual(DelegationText.nativeStatus("cancelled"), "已取消")
        XCTAssertTrue(DelegationText.nativeGroup.contains("Aeroric 只能看"))
        XCTAssertTrue(DelegationText.delegatedGroup.contains("由 Aeroric 管理"))

        let entry = LiveBoardEntry.decode(.object(["id": .string("b1"), "kind": .string("evidence"),
                                                   "text": .string("超时来自 DNS"), "state": .string("open"),
                                                   "author": .string("task-4"),
                                                   "subagent": .string("aeroric-researcher")]))!
        XCTAssertEqual(BoardText.line(entry), "证据 · 子代理 aeroric-researcher：超时来自 DNS")
        XCTAssertEqual(BoardText.author(entry.author, subagent: entry.subagent),
                       "会话 task-4 的子代理 aeroric-researcher 写的")
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
