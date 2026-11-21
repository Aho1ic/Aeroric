import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private class FakeLiveActions: LiveSessionActions {
    var stateValue: LiveSessionState
    var batches: [LiveUpdatesBatch] = []
    var historyValue: [AeroricUI.SessionMessage] = []
    var calls: [String] = []
    var failRespond = false

    init(state: LiveSessionState) { stateValue = state }

    func state(taskID: String) async throws -> LiveSessionState { calls.append("state"); return stateValue }
    func updates(taskID: String, after: UInt64) async throws -> LiveUpdatesBatch {
        calls.append("updates \(after)")
        return batches.isEmpty ? LiveUpdatesBatch(updates: [], latestSeq: after) : batches.removeFirst()
    }
    var failHistory = false
    func history(taskID: String) async throws -> [AeroricUI.SessionMessage] {
        calls.append("history")
        if failHistory { throw LiveSessionError.unsupported("帧过大") }
        return historyValue
    }
    /// A6: when set, history comes in pages; the argument is how many.
    var paged: ((Int) -> LiveHistory)?
    func history(taskID: String, pages: Int) async throws -> LiveHistory {
        guard let paged else { return LiveHistory(messages: try await history(taskID: taskID)) }
        calls.append("history pages \(pages)")
        return paged(pages)
    }
    var writers: [LiveSharedWriter] = []
    func prompt(taskID: String, text: String, afterTurn: Bool) async throws -> LivePromptReceipt {
        calls.append(afterTurn ? "prompt after \(text)" : "prompt \(text)")
        return LivePromptReceipt(deliveryId: "dv-\(calls.count)", sharedWriters: writers)
    }
    func interrupt(taskID: String) async throws { calls.append("interrupt") }
    func respond(taskID: String, requestID: String, decision: LiveDecision, reason: String?) async throws {
        calls.append("respond \(requestID) \(decision.rawValue) \(reason ?? "-")")
        if failRespond { throw LiveSessionError.unsupported("断线") }
    }
    func configure(taskID: String, model: String?, permission: LivePermission?, effort: String?) async throws {
        calls.append("configure \(model ?? "-") \(permission?.rawValue ?? "-") \(effort ?? "-")")
    }
    func command(taskID: String, command: LiveCommand) async throws { calls.append("command \(command.rawValue)") }
    func close(taskID: String) async throws { calls.append("close") }
    func export(taskID: String) async throws { calls.append("export") }
    func startNewSession(like taskID: String) { calls.append("new-session") }
    var roles: [LiveDelegationRole] = []
    func delegate(taskID: String, contract: LiveDelegationContract) async throws {
        calls.append("delegate \(contract.role ?? "-") \(contract.instruction)")
    }
    func delegationRoles() async throws -> [LiveDelegationRole] { roles }
    var cards: [LiveDelegationCard] = []
    func delegationCards(taskID: String) async throws -> [LiveDelegationCard] { cards }
    func remember(taskID: String, text: String, kind: String) async throws -> LiveMemoryItem {
        calls.append("remember \(kind) \(text)")
        return LiveMemoryItem(id: "m1", projectId: "p", kind: kind, text: text)
    }
    func makeMemoryGlobal(id: String) async throws { calls.append("global \(id)") }
    func undoMemory(id: String) async throws { calls.append("undo \(id)") }
    func answer(taskID: String, questionID: String, value: String?, confirmed: Bool?, cancelled: Bool) async throws {
        calls.append("answer \(questionID)")
    }
    func turnPlan(taskID: String, turnID: String, action: String, apply: Bool) async throws -> LiveTurnPlan {
        calls.append("turn \(action) \(turnID)")
        return LiveTurnPlan(files: [.init(path: "a7-one.txt")], turns: [turnID])
    }
}

@MainActor
final class LiveSessionStoreTests: XCTestCase {
    private func state(seq: UInt64, family: String = "codex") -> LiveSessionState {
        LiveSessionState(taskId: "t", live: true, structured: true, family: family, sessionId: "th", turnActive: false,
                         pending: [], plan: [], model: "gpt-5", permission: "ask", effort: nil, contextUsed: nil,
                         contextLimit: nil, inputTokens: nil, outputTokens: nil, latestSeq: seq)
    }

    private func update(_ seq: UInt64, _ kind: LiveUpdateKind) -> LiveSessionUpdate {
        LiveSessionUpdate(taskId: "t", seq: seq, atMs: 0, kind: kind)
    }

    private let user = AeroricUI.SessionMessage(role: "user", content: [.text(text: "hi")])
    private let reply = AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "好")])

    func testATurnStreamsThenHistoryTakesOverOnceItHasTheReply() async {
        let actions = FakeLiveActions(state: state(seq: 4))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        XCTAssertEqual(store.model?.cursor, 4)
        let sent = try? await store.send("hi")
        XCTAssertEqual(sent, .sent)
        actions.batches = [LiveUpdatesBatch(updates: [update(5, .turnStarted(turnId: "u")),
                                                      update(6, .messageChunk(messageId: "m", role: "assistant", text: "好"))],
                                            latestSeq: 6)]
        await store.poll()
        XCTAssertEqual(store.model?.items.map(\.id), ["user:local-1", "message:m"])
        // History lags behind: the live rows stay after the turn ends.
        actions.batches = [LiveUpdatesBatch(updates: [update(7, .turnEnded(turnId: "u", reason: .completed, error: nil))],
                                            latestSeq: 7)]
        await store.poll()
        XCTAssertEqual(store.model?.items.count, 2)
        // A later turn's end, with history caught up, hands over.
        actions.historyValue = [user, reply]
        actions.batches = [LiveUpdatesBatch(updates: [update(8, .turnStarted(turnId: "v")),
                                                      update(9, .turnEnded(turnId: "v", reason: .completed, error: nil))],
                                            latestSeq: 9)]
        await store.poll()
        XCTAssertEqual(store.model?.items, [])
        XCTAssertEqual(store.history, [user, reply])
        XCTAssertTrue(actions.calls.contains("prompt hi"))
    }

    /// E11: after 停止 the page says 正在停止 until the turn really ends.
    func testStopSaysStoppingUntilTheTurnEnds() async {
        let actions = FakeLiveActions(state: state(seq: 4))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        actions.batches = [LiveUpdatesBatch(updates: [update(5, .turnStarted(turnId: "u"))], latestSeq: 5)]
        await store.poll()
        XCTAssertTrue(store.turnActive)
        XCTAssertFalse(store.stopping)
        await store.interrupt()
        XCTAssertTrue(actions.calls.contains("interrupt"))
        XCTAssertTrue(store.stopping, "asked, not stopped yet")
        await store.poll()
        XCTAssertTrue(store.stopping, "still running")
        actions.batches = [LiveUpdatesBatch(updates: [update(6, .turnEnded(turnId: "u", reason: .interrupted, error: nil))],
                                            latestSeq: 6)]
        await store.poll()
        XCTAssertFalse(store.turnActive)
        XCTAssertFalse(store.stopping)
    }

    /// 2026-11-12 (perf): the page reads turns the store grouped once per
    /// history read, not ones it rebuilds per body.
    func testHistoryIsGroupedIntoTurnsOncePerRead() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        actions.historyValue = [user, reply]
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let items = TranscriptBuilder.items(from: [user, reply], taskRunning: false)
        XCTAssertEqual(store.historyItems, items)
        XCTAssertEqual(store.historyTurns, TranscriptTurns.group(items))
        XCTAssertEqual(store.historyTurns.count, 1)
    }

    /// 2026-11-12 (perf): a poll that brings nothing writes nothing the page
    /// observes — @Observable notifies on every assignment, equal or not, and
    /// the page polls every 50–500ms.
    func testAQuietPollNotifiesNoObserver() async {
        let actions = FakeLiveActions(state: state(seq: 3))
        actions.historyValue = [user, reply]
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        final class Fired: @unchecked Sendable { var value = false }
        let fired = Fired()
        withObservationTracking {
            _ = store.model
            _ = store.approvals
            _ = store.connectionProblem
            _ = store.history
            _ = store.historyTurns
            _ = store.historyError
        } onChange: { fired.value = true }
        await store.poll()
        await store.poll()
        XCTAssertFalse(fired.value)
        XCTAssertEqual(store.model?.cursor, 3)
    }

    /// B4-1: a failed history read is shown with 重试, never a silent blank page.
    func testAFailedHistoryReadIsShownAndCanBeRetried() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        actions.failHistory = true
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        XCTAssertNotNil(store.historyError)
        XCTAssertTrue(store.history.isEmpty)
        actions.failHistory = false
        actions.historyValue = [user, reply]
        await store.retryHistory()
        XCTAssertNil(store.historyError)
        XCTAssertEqual(store.history, [user, reply])
    }

    /// A6: a long session offers its older messages; once loaded, later reads
    /// keep that depth. Lines that did not read are said.
    func testEarlierHistoryIsOfferedAndTheDepthIsKept() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        let oldest = AeroricUI.SessionMessage(role: "user", content: [.text(text: "最早")])
        let unreadable = SessionUnreadable(count: 2, firstLine: 17)
        let (first, second) = (user, reply)
        actions.paged = { pages in
            pages > LiveHistory.pageStep
                ? LiveHistory(messages: [oldest, first, second], unreadable: unreadable)
                : LiveHistory(messages: [first, second], hasEarlier: true, unreadable: unreadable)
        }
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        XCTAssertTrue(store.hasEarlierHistory)
        XCTAssertEqual(store.history, [user, reply])
        XCTAssertEqual(store.historyUnreadable?.text, "历史文件里有 2 行读不出来（第一处在第 17 行）")
        await store.loadEarlierHistory()
        XCTAssertEqual(store.history, [oldest, user, reply])
        XCTAssertFalse(store.hasEarlierHistory)
        // Nothing older left: asking again reads nothing.
        await store.loadEarlierHistory()
        await store.retryHistory()
        XCTAssertEqual(actions.calls.filter { $0.hasPrefix("history pages") },
                       ["history pages 24", "history pages 48", "history pages 48"])
    }

    /// B4-2: "#角色 …" delegates instead of prompting; an unknown name is a message.
    func testAHashPrefixDelegatesToARole() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        actions.roles = [LiveDelegationRole(id: "researcher", name: "调研员")]
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let delegated = try? await store.send("#调研员 查超时")
        XCTAssertEqual(delegated, .ranCommand)
        XCTAssertTrue(actions.calls.contains("delegate researcher 查超时"))
        let sent = try? await store.send("#无此角色 照常发")
        XCTAssertEqual(sent, .sent)
        XCTAssertTrue(actions.calls.contains("prompt #无此角色 照常发"))
    }

    /// CU F2: an unsent delegation card outlives the turn and comes back on reopen;
    /// a finished, sent one does not.
    func testADelegationCardStillWantingTheUserSurvivesHistory() async {
        func card(_ id: String, sent: Bool) -> LiveDelegationCard {
            LiveDelegationCard(delegationId: id, parentTaskId: "t", origin: "user", harnessLabel: "Codex",
                               agent: "codex", contract: LiveDelegationContract(instruction: "查"),
                               state: "reported", createdMs: 0, sent: sent)
        }
        let actions = FakeLiveActions(state: state(seq: 1))
        actions.cards = [card("dg-unsent", sent: false), card("dg-done", sent: true)]
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let ids = { store.model?.items.compactMap { item -> String? in
            if case let .delegation(id, _) = item { return id } else { return nil }
        } ?? [] }
        XCTAssertEqual(ids(), ["dg-unsent"])
        var model = store.model!
        model.echo("下一轮")
        model.clearItems()
        XCTAssertEqual(model.items.map(\.id), ["delegation:dg-unsent"])
    }

    /// B5-1: `/记住` is kept by Aeroric and never reaches the agent.
    func testRememberIsKeptNotSent() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let kept = try? await store.send("/记住 回复一律用简体中文")
        XCTAssertEqual(kept, .ranCommand)
        XCTAssertTrue(actions.calls.contains("remember profile 回复一律用简体中文"))
        XCTAssertFalse(actions.calls.contains { $0.hasPrefix("prompt") })
        XCTAssertEqual(store.remembered?.projectId, "p")
        await store.makeRememberedGlobal()
        XCTAssertNil(store.remembered?.projectId)
        await store.undoRemembered()
        XCTAssertNil(store.remembered)
        XCTAssertEqual(actions.calls.suffix(2), ["global m1", "undo m1"])
    }

    func testAGapRereadsStateAndHistory() async {
        let actions = FakeLiveActions(state: state(seq: 2))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        actions.stateValue = state(seq: 40)
        actions.historyValue = [user]
        actions.batches = [LiveUpdatesBatch(updates: [], latestSeq: 40, gapOldestAvailable: 30)]
        actions.calls.removeAll()
        await store.poll()
        XCTAssertEqual(actions.calls, ["updates 2", "state", "history"])
        XCTAssertEqual(store.model?.cursor, 40)
        XCTAssertEqual(store.history, [user])
    }

    func testMoreWaitingIsReportedSoTheLoopDoesNotSleep() async {
        let actions = FakeLiveActions(state: state(seq: 0))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        actions.batches = [LiveUpdatesBatch(updates: [update(1, .turnStarted(turnId: "u"))], latestSeq: 600)]
        let more = await store.poll()
        XCTAssertTrue(more)
    }

    func testSlashCommandsRunInsteadOfBeingSent() async throws {
        let actions = FakeLiveActions(state: state(seq: 0))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let results = [
            try await store.send("/compact"), try await store.send("/review"), try await store.send("/model gpt-x"),
            try await store.send("/model"), try await store.send("/permissions"), try await store.send("/clear"),
        ]
        XCTAssertEqual(results, [.ranCommand, .ranCommand, .ranCommand, .openModelPicker, .openPermissionPicker,
                                 .clearedToNewSession])
        XCTAssertEqual(actions.calls.filter { !$0.hasPrefix("state") && !$0.hasPrefix("history") },
                       ["command compact", "command review", "configure gpt-x - -", "new-session"])
        XCTAssertEqual(store.model?.config.model, "gpt-x")
        let passthrough = try await store.send("/init")
        XCTAssertEqual(passthrough, .sent, "anything else goes to the Agent")
    }

    func testReviewIsRefusedForANonCodexSession() async {
        let actions = FakeLiveActions(state: state(seq: 0, family: "claude"))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        do {
            _ = try await store.send("/review")
            XCTFail("refused")
        } catch {
            XCTAssertFalse(actions.calls.contains("command review"))
        }
    }

    func testAFailedApprovalCanBeRetried() async {
        let actions = FakeLiveActions(state: state(seq: 0))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        actions.batches = [LiveUpdatesBatch(updates: [
            update(1, .permissionRequest(id: "7", toolCallId: nil, kind: "command", summary: "rm", options: [])),
        ], latestSeq: 1)]
        await store.poll()
        actions.failRespond = true
        store.beginDeny("7")
        store.editReason("7", "别删")
        await store.respond("7", .deny)
        XCTAssertEqual(store.approvals.phase("7"), .failed(.deny, message: "断线"))
        actions.failRespond = false
        await store.respond("7", .allowOnce)
        XCTAssertEqual(store.approvals.phase("7"), .submitting(.allowOnce))
        XCTAssertEqual(actions.calls.filter { $0.hasPrefix("respond") }, ["respond 7 deny 别删", "respond 7 allowOnce -"])
    }

    /// B2-2: a resolved card frees its id: a new question that reuses `q1`
    /// can be answered too.
    func testAQuestionIdUsedAgainCanBeAnsweredAgain() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let q1 = LivePendingQuestion(id: "q1", style: "confirm", title: "继续？")
        actions.batches = [LiveUpdatesBatch(updates: [update(2, .question(q1))], latestSeq: 2)]
        await store.poll()
        await store.answer("q1", confirmed: true)
        XCTAssertEqual(store.answering, ["q1"])
        actions.batches = [LiveUpdatesBatch(updates: [update(3, .questionResolved(id: "q1", outcome: "answered")),
                                                     update(4, .question(q1))], latestSeq: 4)]
        await store.poll()
        XCTAssertEqual(store.answering, [], "the first q1 is resolved; the new one is free")
        await store.answer("q1", confirmed: false)
        XCTAssertEqual(actions.calls.filter { $0 == "answer q1" }.count, 2)
    }

    /// B2-2: a poll that was waiting while the page reopened drops what it
    /// read instead of writing the older model over the new one.
    func testAPollThatStartedBeforeAnOpenDoesNotWriteBack() async {
        let actions = SlowUpdates(state: state(seq: 1))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        actions.batches = [LiveUpdatesBatch(updates: [update(2, .notice(level: "info", message: "旧的"))], latestSeq: 2)]
        let poll = Task { await store.poll() }
        await actions.waiting()
        actions.stateValue = state(seq: 9)
        await store.open()
        actions.release()
        _ = await poll.value
        XCTAssertEqual(store.model?.cursor, 9, "the reopened model stays")
    }

    /// B2-2: a failed "load earlier" keeps the depth that was shown.
    func testAFailedEarlierReadKeepsTheDepth() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        actions.paged = { _ in LiveHistory(messages: [], hasEarlier: true) }
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        XCTAssertEqual(store.historyPages, LiveHistory.pageStep)
        actions.paged = nil
        actions.failHistory = true
        await store.loadEarlierHistory()
        XCTAssertEqual(store.historyPages, LiveHistory.pageStep)
        XCTAssertNotNil(store.historyError)
    }


    /// 目检 2026-11-17: undoing or taking back a turn reads history again, so
    /// its footer (产品失败 · 已撤销) follows the ledger instead of staying.
    func testUndoAndRestoreReadHistoryAgain() async {
        let actions = FakeLiveActions(state: state(seq: 1))
        let store = LiveSessionStore(taskID: "t", actions: actions)
        await store.open()
        let before = actions.calls.filter { $0 == "history" }.count
        let undone = await store.applyTurn("u1", action: "undo")
        XCTAssertTrue(undone)
        let restored = await store.applyTurn("u1", action: "restore")
        XCTAssertTrue(restored)
        XCTAssertEqual(actions.calls.filter { $0 == "history" }.count, before + 2)
    }
}


/// B2-2: `updates` that waits until the test lets it go.
private final class SlowUpdates: FakeLiveActions {
    private var gate: CheckedContinuation<Void, Never>?
    private var entered: CheckedContinuation<Void, Never>?
    private var isWaiting = false

    override func updates(taskID: String, after: UInt64) async throws -> LiveUpdatesBatch {
        let batch = try await super.updates(taskID: taskID, after: after)
        await withCheckedContinuation { gate in
            self.gate = gate
            isWaiting = true
            entered?.resume()
            entered = nil
        }
        return batch
    }

    func waiting() async {
        if isWaiting { return }
        await withCheckedContinuation { entered = $0 }
    }

    func release() {
        gate?.resume()
        gate = nil
    }
}
