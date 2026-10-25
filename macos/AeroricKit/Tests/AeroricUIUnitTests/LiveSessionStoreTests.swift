import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeLiveActions: LiveSessionActions {
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
    func history(taskID: String) async throws -> [AeroricUI.SessionMessage] { calls.append("history"); return historyValue }
    func prompt(taskID: String, text: String) async throws { calls.append("prompt \(text)") }
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
}
