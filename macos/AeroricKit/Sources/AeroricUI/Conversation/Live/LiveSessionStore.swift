import os
import Foundation
import Observation
import AeroricProtocol

/// What the live session page may ask of the host. The App implements it on
/// top of `SessionsClient`; the page never holds a connection (INV-02).
@MainActor
public protocol LiveSessionActions {
    func state(taskID: String) async throws -> LiveSessionState
    func updates(taskID: String, after: UInt64) async throws -> LiveUpdatesBatch
    func history(taskID: String) async throws -> [SessionMessage]
    /// History plus the transcript's 统计栏 figures (2026-10-29).
    func historyWithStats(taskID: String) async throws -> (messages: [SessionMessage], stats: SessionTranscriptStats?)
    func prompt(taskID: String, text: String) async throws
    func interrupt(taskID: String) async throws
    func respond(taskID: String, requestID: String, decision: LiveDecision, reason: String?) async throws
    /// Answer a question card: exactly one of `value`, `confirmed`, `cancelled`.
    func answer(taskID: String, questionID: String, value: String?, confirmed: Bool?, cancelled: Bool) async throws
    func revokeGrant(taskID: String, subject: String) async throws
    func configure(taskID: String, model: String?, permission: LivePermission?, effort: String?) async throws
    func command(taskID: String, command: LiveCommand) async throws
    /// Continue under another model source (`configID` nil = the harness's own
    /// login). Returns `native` or `handoff` (2026-10-29).
    func moveSource(taskID: String, configID: String?, model: String) async throws -> String
    /// How a change to `model` (offered under `configID`) will happen.
    func planSwitch(taskID: String, model: String, configID: String?) async throws -> LiveSwitchPlan
    func close(taskID: String) async throws
    func export(taskID: String) async throws
    /// `/clear` (N4 D6): open the new-session page prefilled like this task.
    func startNewSession(like taskID: String)
    /// A05: the task's project's auto-check choice; nil when the host has none.
    func autoCheck(taskID: String) async throws -> LiveAutoCheckSelection?
    func setAutoCheck(taskID: String, _ selection: LiveAutoCheckSelection) async throws
}

extension LiveSessionActions {
    public func moveSource(taskID: String, configID: String?, model: String) async throws -> String {
        throw LiveSessionError.unsupported("这个宿主不支持换模型来源")
    }
    /// Hosts without the planner keep the 2026-10-29 behaviour: always move, and ask.
    public func planSwitch(taskID: String, model: String, configID: String?) async throws -> LiveSwitchPlan {
        LiveSwitchPlan(inPlace: false, configID: configID, handoff: false, confirm: true)
    }
    public func historyWithStats(taskID: String) async throws
        -> (messages: [SessionMessage], stats: SessionTranscriptStats?) {
        (try await history(taskID: taskID), nil)
    }
    public func answer(taskID: String, questionID: String, value: String?, confirmed: Bool?, cancelled: Bool) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持问答卡")
    }
    public func revokeGrant(taskID: String, subject: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持撤销授权")
    }
    public func autoCheck(taskID: String) async throws -> LiveAutoCheckSelection? { nil }
    public func setAutoCheck(taskID: String, _ selection: LiveAutoCheckSelection) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持自动检查")
    }
}

/// A05: which checks the task's project runs after a turn that changed files.
public struct LiveAutoCheckSelection: Equatable, Sendable {
    public var tests: Bool
    public var diagnostics: Bool

    public init(tests: Bool, diagnostics: Bool) {
        self.tests = tests
        self.diagnostics = diagnostics
    }

    public var isOn: Bool { tests || diagnostics }
}

/// What the composer should do after `send` when the input was a command.
public enum LiveSendResult: Equatable, Sendable {
    case sent
    case ranCommand
    case openModelPicker
    case openPermissionPicker
    case clearedToNewSession
}

/// One visible session: history + the live rows on top, polled while the page
/// is on screen (N4 D2: 50ms while a turn runs or an approval waits, 500ms idle).
@Observable
@MainActor
public final class LiveSessionStore {
    public let taskID: String
    public private(set) var history: [SessionMessage] = []
    /// The whole session's usage and timing from its transcript.
    public private(set) var transcriptStats: SessionTranscriptStats?
    public private(set) var model: LiveSessionModel?
    public private(set) var state: LiveSessionState?
    public private(set) var approvals = LiveApprovalMachine()
    public private(set) var loading = false
    /// Polling trouble, shown as a thin banner; cleared by the next good poll.
    public private(set) var connectionProblem: String?
    @ObservationIgnored private var pollFailures = 0
    public var errorText: String?
    /// A05: nil until read, or when the host does not offer auto checks.
    public private(set) var autoCheck: LiveAutoCheckSelection?

    @ObservationIgnored private let actions: any LiveSessionActions
    @ObservationIgnored private var baseline = 0

    public static let fastInterval: Duration = .milliseconds(50)
    public static let idleInterval: Duration = .milliseconds(500)
    public static let retryInterval: Duration = .seconds(1)

    public init(taskID: String, actions: any LiveSessionActions) {
        self.taskID = taskID
        self.actions = actions
    }

    public var family: String? { state?.family }
    public var turnActive: Bool { model?.turnActive ?? false }
    /// A14: quiet time counts from here until the first update arrives.
    public let openedAtMs = UInt64(Date().timeIntervalSince1970 * 1000)
    public var pending: [LivePendingPermission] { model?.pending ?? [] }

    private var wantsFastPolling: Bool { turnActive || !pending.isEmpty }

    /// The page's lifetime loop (`.task`): open, then poll until cancelled.
    public func run() async {
        await open()
        while !Task.isCancelled {
            let more = await poll()
            if more { continue }
            let interval = connectionProblem != nil ? Self.retryInterval
                : wantsFastPolling ? Self.fastInterval : Self.idleInterval
            try? await Task.sleep(for: interval)
        }
    }

    /// Snapshot + history; the cursor starts at the snapshot's seq.
    public func open() async {
        loading = true
        defer { loading = false }
        do {
            let state = try await actions.state(taskID: taskID)
            self.state = state
            model = LiveSessionModel(state: state)
            approvals.retain(pending: state.pending)
            await reloadHistory()
            baseline = LiveHistoryMerge.assistantCount(history)
        } catch {
            errorText = "读取会话失败：\(error)"
        }
        autoCheck = try? await actions.autoCheck(taskID: taskID)
    }

    public func setAutoCheck(_ selection: LiveAutoCheckSelection) async {
        do {
            try await actions.setAutoCheck(taskID: taskID, selection)
            autoCheck = selection
        } catch {
            errorText = "保存自动检查设置失败：\(error)"
        }
    }

    /// One `session.updates` round. Returns true when more are already waiting.
    @discardableResult
    public func poll() async -> Bool {
        guard var current = model else { return false }
        do {
            let batch = try await actions.updates(taskID: taskID, after: current.cursor)
            connectionProblem = nil
            pollFailures = 0
            let wasActive = current.turnActive
            switch current.apply(batch) {
            case .needsResync:
                await resync()
                return false
            case let .applied(turnEnded):
                if !wasActive && current.turnActive { baseline = LiveHistoryMerge.assistantCount(history) }
                model = current
                approvals.retain(pending: current.pending)
                if turnEnded { await settleTurn() }
                return batch.latestSeq > current.cursor
            }
        } catch {
            noteConnectionFailure(error)
            return false
        }
    }

    /// One failed round is a blip; the banner shows from the second in a row,
    /// with the reason, which also goes to the log (2026-10-28).
    private func noteConnectionFailure(_ error: Error) {
        pollFailures += 1
        Logger(subsystem: "com.aeroric.native", category: "live")
            .error("session \(self.taskID, privacy: .public) poll failed (\(self.pollFailures, privacy: .public)): \(String(describing: error), privacy: .public)")
        if pollFailures >= 2 {
            connectionProblem = "连接中断，正在重试…（\(String(describing: error).prefix(120))）"
        }
    }

    private func resync() async {
        do {
            let state = try await actions.state(taskID: taskID)
            self.state = state
            var fresh = model ?? LiveSessionModel(state: state)
            fresh.resync(state)
            model = fresh
            approvals.retain(pending: state.pending)
            await reloadHistory()
            baseline = LiveHistoryMerge.assistantCount(history)
        } catch {
            noteConnectionFailure(error)
        }
    }

    /// N4 D3: after a turn, history replaces the live rows once it has them.
    private func settleTurn() async {
        await reloadHistory()
        if LiveHistoryMerge.historyCaughtUp(baseline: baseline, history: history) {
            model?.clearItems()
            baseline = LiveHistoryMerge.assistantCount(history)
        }
        if let state = try? await actions.state(taskID: taskID) { self.state = state }
    }

    private func reloadHistory() async {
        if let page = try? await actions.historyWithStats(taskID: taskID) {
            history = page.messages
            if let stats = page.stats { transcriptStats = stats }
        }
    }

    // MARK: - User actions

    /// Sends the composer text, or runs it when it is one of the five commands.
    public func send(_ text: String) async throws -> LiveSendResult {
        if let command = SlashCommand.parse(text) {
            switch command {
            case .compact:
                try await actions.command(taskID: taskID, command: .compact)
                return .ranCommand
            case .review:
                guard family == "codex" else { throw LiveSessionError.unsupported("/review 仅 Codex 支持") }
                try await actions.command(taskID: taskID, command: .review)
                return .ranCommand
            case let .model(name?):
                try await configure(model: name)
                return .ranCommand
            case .model(nil):
                return .openModelPicker
            case .permissions:
                return .openPermissionPicker
            case .clear:
                actions.startNewSession(like: taskID)
                return .clearedToNewSession
            }
        }
        if !turnActive { baseline = LiveHistoryMerge.assistantCount(history) }
        model?.echo(text)
        try await actions.prompt(taskID: taskID, text: text)
        return .sent
    }

    public func interrupt() async {
        do { try await actions.interrupt(taskID: taskID) } catch { errorText = "停止失败：\(error)" }
    }

    public func endSession() async {
        do { try await actions.close(taskID: taskID) } catch { errorText = "结束会话失败：\(error)" }
    }

    public func planSwitch(model: String, configID: String?) async throws -> LiveSwitchPlan {
        try await actions.planSwitch(taskID: taskID, model: model, configID: configID)
    }

    /// Moves the session to another source and model; the next message runs there.
    public func moveSource(configID: String?, model newModel: String) async throws -> String {
        let mode = try await actions.moveSource(taskID: taskID, configID: configID, model: newModel)
        model?.setConfig(LiveConfig(model: newModel, permission: nil, effort: nil))
        if let state = try? await actions.state(taskID: taskID) { self.state = state }
        return mode
    }

    public func configure(model newModel: String? = nil, permission: LivePermission? = nil,
                          effort: String? = nil) async throws {
        try await actions.configure(taskID: taskID, model: newModel, permission: permission, effort: effort)
        model?.setConfig(LiveConfig(model: newModel, permission: permission?.rawValue, effort: effort))
    }

    public func beginDeny(_ id: String) { approvals.beginDeny(id) }
    public func editReason(_ id: String, _ text: String) { approvals.editReason(id, text) }
    public func cancelDeny(_ id: String) { approvals.cancelDeny(id) }

    public func respond(_ id: String, _ decision: LiveDecision) async {
        guard let submission = approvals.submit(id, decision) else { return }
        do {
            try await actions.respond(taskID: taskID, requestID: id, decision: submission.decision,
                                      reason: submission.reason)
        } catch {
            approvals.failed(id, message: "\(error)")
        }
    }

    /// Question cards being sent (disabled until the host resolves them).
    public private(set) var answering: Set<String> = []

    public func answer(_ id: String, value: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) async {
        answering.insert(id)
        do {
            try await actions.answer(taskID: taskID, questionID: id, value: value, confirmed: confirmed, cancelled: cancelled)
        } catch {
            answering.remove(id)
            errorText = "回答没有送达：\(error)"
        }
    }

    public func revokeGrant(_ subject: String) async {
        do {
            try await actions.revokeGrant(taskID: taskID, subject: subject)
            revokedGrants.insert(subject)
        } catch {
            errorText = "撤销授权失败：\(error)"
        }
    }

    /// Grants revoked from this page (the row says so instead of offering it again).
    public private(set) var revokedGrants: Set<String> = []

    public func export() async {
        do { try await actions.export(taskID: taskID) } catch { errorText = "导出失败：\(error)" }
    }
}

public enum LiveSessionError: Error, Equatable, CustomStringConvertible {
    case unsupported(String)

    public var description: String {
        switch self {
        case let .unsupported(text): text
        }
    }
}
