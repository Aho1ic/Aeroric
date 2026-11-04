import AeroricProtocol
@testable import AeroricUI
import XCTest

/// R1-2: the delivery bar says nothing while things arrive, and one line for
/// the most pressing group otherwise.
final class DeliveryTests: XCTestCase {
    private func item(_ id: String, _ state: String, kind: String = "delegation", reason: String? = nil,
                      at: UInt64 = 1) -> LiveDelivery {
        LiveDelivery(id: id, taskId: "t", kind: kind, label: "委派 · 结果", state: state, reason: reason,
                     createdMs: at, updatedMs: at)
    }

    func testNothingIsSaidWhileEverythingArrives() {
        XCTAssertNil(DeliveryText.notice([item("a", "delivered"), item("b", "sending"), item("c", "waiting")]))
    }

    func testFailedComesFirstThenUnknownThenPaused() {
        let failed = DeliveryText.notice([item("p", "paused"), item("f", "failed", reason: "Claude 未登录")])
        XCTAssertEqual(failed?.text, "没送达：Claude 未登录")
        XCTAssertEqual(failed?.ids, ["f"])
        XCTAssertEqual(failed?.actions.map(\.title), ["重试", "丢弃"])
        let unknown = DeliveryText.notice([item("p", "paused"), item("u", "unknown", reason: "会话记录里没找到")])
        XCTAssertEqual(unknown?.text, "委派 · 结果 不确定是否送达 · 会话记录里没找到")
        let paused = DeliveryText.notice([item("p", "paused"), item("q", "paused", at: 2)])
        XCTAssertEqual(paused?.text, "2 条未发出（你按了停止）")
        XCTAssertEqual(paused?.ids, ["p", "q"])
        XCTAssertEqual(DeliveryText.method(.resume), "delivery.resume")
    }

    /// R1-3a: the slash menu and the permission picker read the table.
    @MainActor func testTheCapabilityTableGreysWhatIsNotSupported() {
        let claude = LiveCapabilities([
            LiveCapabilityEntry(feature: "review", state: "no", note: "Claude 没有内置审查"),
            LiveCapabilityEntry(feature: "compact", state: "yes"),
            LiveCapabilityEntry(feature: "permAsk", state: "yes", note: "由 Claude 把关 · 无沙盒"),
        ])
        let review = SlashCommand.suggestions(for: "/re", family: "claude", capabilities: claude).first
        XCTAssertEqual(review?.enabled, false)
        XCTAssertEqual(review?.detail, "Claude 没有内置审查")
        let codexReview = SlashCommand.suggestions(for: "/re", family: "codex").first
        XCTAssertEqual(codexReview?.enabled, true, "no table: the old answer")
        let ask = LivePickerOptions.permissions(current: "ask", capabilities: claude).sections.first?.options.first
        XCTAssertEqual(ask?.trailing, "由 Claude 把关 · 无沙盒")
        XCTAssertTrue(ask?.isEnabled ?? false)
        let plan = SlashCommand.newSessionSuggestions(
            for: "/pl", harness: "omp",
            capabilities: LiveCapabilities([LiveCapabilityEntry(feature: "planMode", state: "unknown")])).first
        XCTAssertEqual(plan?.detail, "以只读规划开始：先出方案再动手 · 未确认")
    }

    /// R1-3a: the login list decodes what the host sends.
    func testAnExpiredLoginMatchesOnlyItsOwnConfiguration() {
        let gateway = ExpiredLogin(id: "claude|local|cfg-x", family: "claude", label: "x", reason: "401", atMs: 1,
                                   configId: "cfg-x")
        let own = ExpiredLogin(id: "claude|local|own", family: "claude", label: "Claude（本机）", reason: "401",
                               atMs: 1, command: "claude")
        XCTAssertNil(NewSessionLayout.expiredLogin([gateway], harness: "claude", configID: "claude"))
        XCTAssertEqual(NewSessionLayout.expiredLogin([gateway], harness: "claude", configID: "agent-config:cfg-x")?.id,
                       gateway.id)
        XCTAssertEqual(NewSessionLayout.expiredLogin([gateway, own], harness: "claude", configID: nil)?.id, own.id)
        XCTAssertNil(NewSessionLayout.expiredLogin([own], harness: "codex", configID: nil))
    }

    func testATurnRecordMatchesTheTranscriptTurnItStartedIn() {
        let records = [LiveTurnRecord(turnId: "u1", startedMs: 10_000, endedMs: 40_000, files: ["a"]),
                       LiveTurnRecord(turnId: "u2", startedMs: 100_000, endedMs: 120_000, files: ["b"], state: "undone")]
        let first = SessionTranscriptStats.TurnStat(startedAt: 11_000, endedAt: 39_000, tokens: 0)
        let far = SessionTranscriptStats.TurnStat(startedAt: 60_000, endedAt: 70_000, tokens: 0)
        XCTAssertEqual(TurnLedgerText.record(for: first, in: records)?.turnId, "u1")
        XCTAssertNil(TurnLedgerText.record(for: far, in: records), "no guess when nothing fits")
        XCTAssertEqual(TurnLedgerText.pending(records), 1)
        XCTAssertEqual(TurnLedgerText.badge("undone"), "已撤销")
        XCTAssertEqual(TurnLedgerText.summary(LiveTurnRecord(turnId: "x", startedMs: 0, endedMs: 0, files: ["a", "b"],
                                                             additions: 18, deletions: 4)), "2 个文件 +18 −4")
    }

    func testATurnPlanDecodes() {
        let plan = LiveTurnPlan.decode(.object([
            "files": .array([.object(["path": .string("a.rs")]), .object(["path": .string("b.rs"), "delete": .bool(true)])]),
            "skipped": .array([.object(["path": .string("c.rs"), "reason": .string("这一轮之后又改过")])]),
        ]))
        XCTAssertEqual(plan.files, [.init(path: "a.rs"), .init(path: "b.rs", delete: true)])
        XCTAssertEqual(plan.skipped.first?.reason, "这一轮之后又改过")
        XCTAssertNil(plan.blocked)
    }

    func testExpiredLoginsDecode() {
        let list = ExpiredLogin.decodeList(.array([.object([
            "id": .string("claude|local|own"), "family": .string("claude"), "label": .string("Claude（本机）"),
            "reason": .string("401"), "atMs": .number("5"), "command": .string("claude"),
        ])]))
        XCTAssertEqual(list.first?.label, "Claude（本机）")
        XCTAssertEqual(list.first?.command, "claude")
        XCTAssertNil(list.first?.configId)
    }

    /// R1-2b Q3: what waits for the turn shows on its own bubble, not the bar.
    func testAUserMessageWaitingForTheTurnIsOnItsBubble() {
        var waiting = item("w", "waiting", kind: "user")
        waiting.mode = "afterTurn"
        XCTAssertNil(DeliveryText.notice([waiting]))
        XCTAssertEqual(DeliveryText.userState(waiting), "等这轮结束后发送")
        XCTAssertNil(DeliveryText.userState(item("d", "delivered", kind: "user")))
    }

    /// R1-2b (Q-S7): after a restart the user's message waits for them.
    func testAfterARestartTheUsersMessageIsUnsent() {
        let unsent = DeliveryText.notice([item("u", "paused", kind: "user", reason: DeliveryText.restarted)])
        XCTAssertEqual(unsent?.text, "1 条消息未发送（Aeroric 重启了）")
        XCTAssertEqual(unsent?.actions.map(\.title), ["发送", "修改", "丢弃"])
        XCTAssertEqual(DeliveryText.userState(item("u", "paused", kind: "user", reason: DeliveryText.restarted)),
                       "未发送")
        let failed = DeliveryText.notice([item("f", "failed", kind: "user", reason: "Claude 未登录")])
        XCTAssertEqual(failed?.actions.map(\.title), ["重试", "修改"])
    }

    /// R1-2b: the echo is bound to its delivery, a queued one survives
    /// history taking over, and a withdrawn one goes.
    func testAQueuedMessageStaysUntilItLandsAndGoesWhenWithdrawn() {
        var model = LiveSessionModel(state: LiveSessionState(
            taskId: "t", live: true, structured: true, family: "codex", sessionId: "th", turnActive: true,
            pending: [], plan: [], model: "gpt-5", permission: "ask", effort: nil, contextUsed: nil,
            contextLimit: nil, inputTokens: nil, outputTokens: nil, latestSeq: 0))
        var queued = item("dv-1", "waiting", kind: "user", at: 1)
        queued.mode = "afterTurn"
        queued.text = "稍后"
        model.restoreDeliveries([queued])
        XCTAssertEqual(model.items.count, 1, "adopted from the queue")
        model.echo("稍后", afterTurn: true, delivery: "dv-1")
        XCTAssertEqual(model.items.count, 1, "not echoed twice")
        model.clearItems()
        XCTAssertEqual(model.items.count, 1, "still queued: kept")
        queued.state = "withdrawn"
        queued.updatedMs = 2
        model.restoreDeliveries([queued])
        XCTAssertTrue(model.items.isEmpty)
    }

    func testDeliveryUpdatesDecodeAndTheLatestWins() throws {
        let update = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("3"), "atMs": .number("5"), "type": .string("delivery"),
            "delivery": .object(["id": .string("dv-1"), "taskId": .string("t"), "state": .string("delivered"),
                                 "evidence": .string("echo"), "label": .string("审查 · 意见"),
                                 "updatedMs": .number("9")]),
        ]))
        guard case let .delivery(delivery) = update.kind else { return XCTFail("\(update.kind)") }
        XCTAssertEqual(DeliveryText.evidence(delivery), "Claude 回显")
        XCTAssertEqual(DeliveryText.state(delivery), "已送达")
        let receipt = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("4"), "atMs": .number("6"), "type": .string("deliveryReceipt"),
            "id": .string("dv-1"), "outcome": .string("delivered"),
        ]))
        guard case .deliveryReceipt = receipt.kind else { return XCTFail("\(receipt.kind)") }
    }
}
