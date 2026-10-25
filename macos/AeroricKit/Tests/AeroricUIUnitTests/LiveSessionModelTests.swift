import AeroricProtocol
@testable import AeroricUI
import XCTest

/// N4: live session reduction, approval cards, slash commands, usage line and
/// notification gating — pure logic, no host or UI.
final class LiveSessionModelTests: XCTestCase {
    private func state(seq: UInt64 = 0, pending: [LivePendingPermission] = [], turnActive: Bool = false) -> LiveSessionState {
        LiveSessionState(taskId: "t", live: true, structured: true, family: "codex", sessionId: "th", turnActive: turnActive,
                         pending: pending, plan: [], model: "gpt-5", permission: "ask", effort: nil,
                         contextUsed: nil, contextLimit: nil, inputTokens: nil, outputTokens: nil, latestSeq: seq)
    }

    private func batch(_ kinds: [LiveUpdateKind], from first: UInt64 = 1) -> LiveUpdatesBatch {
        let updates = kinds.enumerated().map { offset, kind in
            LiveSessionUpdate(taskId: "t", seq: first + UInt64(offset), atMs: 0, kind: kind)
        }
        return LiveUpdatesBatch(updates: updates, latestSeq: first + UInt64(kinds.count) - 1)
    }

    func testATurnStreamsTextThoughtsToolsAndEnds() {
        var model = LiveSessionModel(state: state())
        let outcome = model.apply(batch([
            .turnStarted(turnId: "u1"),
            .thoughtChunk(id: "r", text: "先看", summary: true),
            .thoughtChunk(id: "r", text: "目录", summary: true),
            .messageChunk(messageId: "m", role: "assistant", text: "我"),
            .messageChunk(messageId: "m", role: "assistant", text: "来"),
            .toolCall(id: "c", kind: .command, title: "ls", input: .null),
            .toolCallUpdate(id: "c", status: .running, outputDelta: "a\n", result: nil),
            .toolCallUpdate(id: "c", status: .completed, outputDelta: "b\n", result: .object(["exitCode": .number("0")])),
        ]))
        XCTAssertEqual(outcome, .applied(turnEnded: false))
        XCTAssertTrue(model.turnActive)
        XCTAssertEqual(model.cursor, 8)
        XCTAssertEqual(model.items.count, 3)
        XCTAssertEqual(model.items[0], .thought(id: "r", text: "先看目录", summary: true))
        XCTAssertEqual(model.items[1], .message(id: "m", text: "我来"))
        guard case let .tool(tool) = model.items[2] else { return XCTFail("tool") }
        XCTAssertEqual(tool.output, "a\nb\n")
        XCTAssertEqual(tool.status, .completed)

        let ended = model.apply(batch([.messageCompleted(messageId: "m", text: "我来看看"),
                                       .turnEnded(turnId: "u1", reason: .completed, error: nil)], from: 9))
        XCTAssertEqual(ended, .applied(turnEnded: true))
        XCTAssertFalse(model.turnActive)
        XCTAssertEqual(model.items[1], .message(id: "m", text: "我来看看"))
    }

    func testDuplicatesAreDroppedAndAHoleAsksForResync() {
        var model = LiveSessionModel(state: state(seq: 2))
        XCTAssertEqual(model.apply(batch([.turnStarted(turnId: "old"), .turnStarted(turnId: "old")])), .applied(turnEnded: false))
        XCTAssertEqual(model.cursor, 2, "seq ≤ cursor is a duplicate")
        XCTAssertFalse(model.turnActive)
        XCTAssertEqual(model.apply(batch([.turnStarted(turnId: "u")], from: 3)), .applied(turnEnded: false))
        XCTAssertEqual(model.apply(batch([.messageChunk(messageId: "m", role: "assistant", text: "x")], from: 5)), .needsResync)
        XCTAssertEqual(model.cursor, 3, "nothing after the hole applies")
        XCTAssertEqual(model.apply(LiveUpdatesBatch(updates: [], latestSeq: 900, gapOldestAvailable: 400)), .needsResync)
        model.echo("hi")
        model.resync(state(seq: 900, turnActive: true))
        XCTAssertEqual(model.cursor, 900)
        XCTAssertTrue(model.items.isEmpty)
        XCTAssertTrue(model.turnActive)
    }

    func testApprovalsComeAndGoAndATurnEndClearsThem() {
        var model = LiveSessionModel(state: state())
        _ = model.apply(batch([
            .turnStarted(turnId: "u"),
            .permissionRequest(id: "7", toolCallId: "c", kind: "command", summary: "rm", options: []),
            .permissionRequest(id: "8", toolCallId: nil, kind: "Write", summary: "a.swift", options: ["allowOnce", "deny"]),
            .permissionResolved(id: "7", decision: "deny"),
        ]))
        XCTAssertEqual(model.pending.map(\.id), ["8"])
        XCTAssertEqual(model.pending.first?.options, ["allowOnce", "deny"], "no session-wide allowance offered")
        _ = model.apply(batch([.turnEnded(turnId: "u", reason: .interrupted, error: nil)], from: 5))
        XCTAssertTrue(model.pending.isEmpty)
        XCTAssertEqual(model.items.last, .notice(id: "seq5", message: "已中断", recoverable: true))
    }

    func testPlanUsageConfigErrorsAndUnknownAreKept() {
        var model = LiveSessionModel(state: state())
        _ = model.apply(batch([
            .plan([LivePlanEntry(text: "读", status: "completed")]),
            .usage(contextUsed: 170_000, contextLimit: 200_000, inputTokens: 12_345, outputTokens: 1_100),
            .configChanged(model: "gpt-x", permission: nil, effort: "high"),
            .error(message: "上下文已压缩", recoverable: true),
            .unknown(kind: "item/future", payload: .null),
            .subagent(id: "s", title: "检索", status: "running"),
            .subagent(id: "s", title: "检索", status: "completed"),
        ]))
        XCTAssertEqual(model.plan, [LivePlanEntry(text: "读", status: "completed")])
        XCTAssertEqual(model.config, LiveConfig(model: "gpt-x", permission: "ask", effort: "high"))
        XCTAssertEqual(model.subagents, [LiveSubagent(id: "s", title: "检索", status: "completed")])
        XCTAssertEqual(model.items, [.notice(id: "seq4", message: "上下文已压缩", recoverable: true),
                                     .unknown(id: "seq5", kind: "item/future")])
        XCTAssertTrue(model.usage.isHigh)
        XCTAssertEqual(model.usage.text, "上下文 85% · 输入 12.3k · 输出 1.1k")
    }

    /// A05: the turn's changes survive history replacing the live rows.
    func testTurnChangesAreKeptApartFromLiveRows() {
        var model = LiveSessionModel(state: state())
        let files = [LiveFileChange(path: "src/a.rs", additions: 3, deletions: 1),
                     LiveFileChange(path: "img.png", additions: nil, deletions: nil)]
        _ = model.apply(batch([
            .turnStarted(turnId: "u1"),
            .messageChunk(messageId: "m", role: "assistant", text: "改好了"),
            .turnEnded(turnId: "u1", reason: .completed, error: nil),
            .turnChanges(turnId: "u1", files: files, additions: 3, deletions: 1, truncated: false),
        ]))
        model.clearItems()
        XCTAssertTrue(model.items.isEmpty)
        XCTAssertEqual(model.lastTurnChanges?.files, files)
        XCTAssertEqual(model.lastTurnChanges?.summary, "+3 −1 · 2 个文件")
    }

    func testTurnChangesDecodeFromTheWire() throws {
        let update = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("4"), "atMs": .number("1"), "type": .string("turnChanges"),
            "turnId": .string("u1"), "additions": .number("3"), "deletions": .number("1"), "truncated": .bool(true),
            "files": .array([.object(["path": .string("src/a.rs"), "additions": .number("3"), "deletions": .number("1")]),
                             .object(["path": .string("img.png")])]),
        ]))
        XCTAssertEqual(update.kind, .turnChanges(turnId: "u1", files: [
            LiveFileChange(path: "src/a.rs", additions: 3, deletions: 1),
            LiveFileChange(path: "img.png", additions: nil, deletions: nil),
        ], additions: 3, deletions: 1, truncated: true))
    }

    func testAFailedTurnShowsItsReason() {
        var model = LiveSessionModel(state: state())
        _ = model.apply(batch([.turnStarted(turnId: "u"), .turnEnded(turnId: "u", reason: .failed, error: "quota")]))
        XCTAssertEqual(model.items, [.notice(id: "seq2", message: "quota", recoverable: false)])
    }

    func testHistoryReplacesLiveRowsOnlyOnceItCaughtUp() {
        let before = [AeroricUI.SessionMessage(role: "user", content: [.text(text: "hi")]),
                      AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "a")])]
        XCTAssertFalse(LiveHistoryMerge.historyCaughtUp(baseline: 1, history: before))
        let after = before + [AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "b")])]
        XCTAssertTrue(LiveHistoryMerge.historyCaughtUp(baseline: 1, history: after))
    }

    func testUsageFormatting() {
        XCTAssertEqual(UsageSummary.compact(999), "999")
        XCTAssertEqual(UsageSummary.compact(1000), "1k")
        XCTAssertEqual(UsageSummary.compact(12_345), "12.3k")
        XCTAssertEqual(UsageSummary.compact(2_000_000), "2M")
        let unknown = UsageSummary(contextUsed: nil, contextLimit: nil, inputTokens: nil, outputTokens: nil)
        XCTAssertTrue(unknown.isEmpty)
        XCTAssertFalse(unknown.isHigh)
        XCTAssertEqual(UsageSummary(contextUsed: 80, contextLimit: 100, inputTokens: nil, outputTokens: nil).isHigh, false)
    }

    func testApprovalMachine() {
        var machine = LiveApprovalMachine()
        XCTAssertTrue(machine.canAct("7"))
        machine.beginDeny("7")
        machine.editReason("7", "  别删  ")
        let submitted = machine.submit("7", .deny)
        XCTAssertEqual(submitted?.decision, .deny)
        XCTAssertEqual(submitted?.reason, "别删")
        XCTAssertNil(machine.submit("7", .allowOnce), "no double submit")
        machine.failed("7", message: "断线")
        XCTAssertEqual(machine.phase("7"), .failed(.deny, message: "断线"))
        XCTAssertEqual(machine.submit("7", .allowOnce)?.reason, nil, "retry after failure")
        machine.beginDeny("9")
        XCTAssertEqual(machine.submit("9", .deny)?.reason, nil, "an empty reason is no reason")
        machine.retain(pending: [LivePendingPermission(id: "9", kind: "command", summary: "", toolCallId: nil)])
        XCTAssertEqual(machine.phase("7"), .pending, "resolved cards are forgotten")
        XCTAssertEqual(machine.phase("9"), .submitting(.deny))
    }

    func testSlashCommands() {
        XCTAssertEqual(SlashCommand.parse("/compact"), .compact)
        XCTAssertEqual(SlashCommand.parse(" /review "), .review)
        XCTAssertEqual(SlashCommand.parse("/model"), .model(nil))
        XCTAssertEqual(SlashCommand.parse("/model gpt-5.1"), .model("gpt-5.1"))
        XCTAssertEqual(SlashCommand.parse("/permissions"), .permissions)
        XCTAssertEqual(SlashCommand.parse("/clear"), .clear)
        XCTAssertNil(SlashCommand.parse("/compact now"), "unknown shapes go to the Agent")
        XCTAssertNil(SlashCommand.parse("/init"))
        XCTAssertNil(SlashCommand.parse("hello /compact"))
        XCTAssertEqual(SlashCommand.suggestions(for: "/", family: "codex").map(\.command),
                       ["/compact", "/review", "/model", "/permissions", "/clear"])
        XCTAssertEqual(SlashCommand.suggestions(for: "/re", family: "claude").map(\.enabled), [false])
        XCTAssertTrue(SlashCommand.suggestions(for: "/model x", family: "codex").isEmpty)
    }

    func testNotificationGate() {
        var gate = ApprovalNotificationGate()
        XCTAssertFalse(gate.shouldNotify(taskID: "t", previous: "running", current: "input_required", appActive: true))
        XCTAssertFalse(gate.shouldNotify(taskID: "t", previous: "input_required", current: "input_required", appActive: false))
        XCTAssertTrue(gate.shouldNotify(taskID: "u", previous: "running", current: "input_required", appActive: false))
        XCTAssertFalse(gate.shouldNotify(taskID: "u", previous: "running", current: "input_required", appActive: false), "once")
        XCTAssertFalse(gate.shouldNotify(taskID: "u", previous: "input_required", current: "running", appActive: false))
        XCTAssertTrue(gate.shouldNotify(taskID: "u", previous: "running", current: "input_required", appActive: false), "again after it moved on")
    }
}

@MainActor
final class LivePickerOptionsTests: XCTestCase {
    func testModelsKeepTheCurrentOneAndPermissionsAreTheThreeTiers() {
        let models = LivePickerOptions.models(["a", "b"], current: "custom")
        XCTAssertEqual(models.sections[0].options.map(\.id), ["custom", "a", "b"])
        XCTAssertEqual(models.sections[0].options.first(where: \.selected)?.id, "custom")
        XCTAssertNotNil(LivePickerOptions.models([], current: nil).note)
        let permissions = LivePickerOptions.permissions(current: "auto_edit")
        XCTAssertEqual(permissions.sections[0].options.map(\.id), ["ask", "auto_edit", "full_access"])
        XCTAssertEqual(permissions.sections[0].options.first(where: \.selected)?.id, "auto_edit")
    }

    func testClaudeEffortIsReadOnlyAndAnEmptyCatalogHidesTheCapsule() {
        XCTAssertNil(LivePickerOptions.efforts([], current: nil, editable: true))
        let codex = LivePickerOptions.efforts(["low", "high"], current: "low", editable: true)
        XCTAssertTrue(codex?.sections[0].options.allSatisfy(\.isEnabled) == true)
        let claude = LivePickerOptions.efforts(["low", "high"], current: "low", editable: false)
        XCTAssertEqual(claude?.sections[0].options.map(\.isEnabled), [true, false])
    }

    func testChangesAreLinesNotFiles() {
        XCTAssertEqual(LiveSessionFacts(agent: "codex").changesText, "—")
        XCTAssertEqual(LiveSessionFacts(agent: "codex", additions: 12, deletions: 3).changesText, "+12 −3")
    }
}
