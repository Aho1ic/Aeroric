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
    /// B4′: the task's checkpoint; nil when the host keeps none.
    func checkpoint(taskID: String) async throws -> LiveCheckpoint?
    /// B4′: accept the clarification card's ticked criteria and start the task.
    func acceptClarification(taskID: String, acceptance: [LiveAcceptance], constraints: [String]) async throws
    /// B4-1: whether the task's review loop holds findings for the user.
    func reviewPaused(taskID: String) async throws -> Bool
    func setReviewPaused(taskID: String, _ paused: Bool) async throws
    /// B4-1: review the task's last changed turn now.
    func requestReview(taskID: String) async throws
    /// B4-1: send findings a paused loop held back.
    func sendReview(taskID: String) async throws
    /// B4-1: run a failed or interrupted round again.
    func retryReview(taskID: String) async throws
    /// B4-2: delegate from this task (the `#` prefix, the progress page).
    func delegate(taskID: String, contract: LiveDelegationContract) async throws
    func stopDelegation(taskID: String, delegationID: String) async throws
    func sendDelegation(taskID: String, delegationID: String) async throws
    func redoDelegation(taskID: String, delegationID: String, note: String) async throws
    /// B4-2: the role library (for the `#` suggestions).
    func delegationRoles() async throws -> [LiveDelegationRole]
    /// B4-3: the user saw a delegation's probes.
    func ackDelegationProbes(taskID: String, delegationID: String) async throws
    /// CU F2: this task's delegations, to put back cards still waiting on the user.
    func delegationCards(taskID: String) async throws -> [LiveDelegationCard]
    /// B5-1: `/记住 …` in this task's project; the item as kept.
    func remember(taskID: String, text: String, kind: String) async throws -> LiveMemoryItem
    /// B5-1: the "已记住" card's 改为全局 and 撤销.
    func makeMemoryGlobal(id: String) async throws
    func undoMemory(id: String) async throws
    /// Show another task (a reviewer, a parent).
    func openTask(_ taskID: String)
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
    public func checkpoint(taskID: String) async throws -> LiveCheckpoint? { nil }
    public func acceptClarification(taskID: String, acceptance: [LiveAcceptance], constraints: [String]) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持开工前澄清")
    }
    public func reviewPaused(taskID: String) async throws -> Bool { false }
    public func setReviewPaused(taskID: String, _ paused: Bool) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持审查回环")
    }
    public func requestReview(taskID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持审查回环")
    }
    public func sendReview(taskID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持审查回环")
    }
    public func retryReview(taskID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持审查回环")
    }
    public func delegate(taskID: String, contract: LiveDelegationContract) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func stopDelegation(taskID: String, delegationID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func sendDelegation(taskID: String, delegationID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func redoDelegation(taskID: String, delegationID: String, note: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func delegationRoles() async throws -> [LiveDelegationRole] { [] }
    public func delegationCards(taskID: String) async throws -> [LiveDelegationCard] { [] }
    public func ackDelegationProbes(taskID: String, delegationID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func remember(taskID: String, text: String, kind: String) async throws -> LiveMemoryItem {
        throw LiveSessionError.unsupported("这个宿主不支持记忆")
    }
    public func makeMemoryGlobal(id: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持记忆")
    }
    public func undoMemory(id: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持记忆")
    }
    public func openTask(_ taskID: String) {}
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
    /// B4-1: findings wait for the user before going to the writer.
    public private(set) var reviewPaused = false
    /// B4-2: the role library, for the composer's `#` suggestions.
    public private(set) var delegationRoles: [LiveDelegationRole] = []
    /// B5-1: what the last `/记住` kept (the "已记住" card), until dismissed.
    public private(set) var remembered: LiveMemoryItem?

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
            await restoreDelegations()
        } catch {
            errorText = "读取会话失败：\(error)"
        }
        autoCheck = try? await actions.autoCheck(taskID: taskID)
        reviewPaused = (try? await actions.reviewPaused(taskID: taskID)) ?? false
        delegationRoles = (try? await actions.delegationRoles()) ?? []
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
            await restoreDelegations()
        } catch {
            noteConnectionFailure(error)
        }
    }

    /// CU F2: cards of this task's delegations that still want the user.
    private func restoreDelegations() async {
        guard let cards = try? await actions.delegationCards(taskID: taskID) else { return }
        for card in cards where card.parentTaskId == taskID && card.wantsUser {
            model?.restoreDelegation(card)
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
        if let body = MemoryText.rememberCommand(text) {
            remembered = try await actions.remember(taskID: taskID, text: body, kind: MemoryText.guessKind(body))
            return .ranCommand
        }
        if let (target, instruction) = DelegationText.hashCommand(text),
           let contract = delegationContract(target: target, instruction: instruction) {
            try await actions.delegate(taskID: taskID, contract: contract)
            return .ranCommand
        }
        if !turnActive { baseline = LiveHistoryMerge.assistantCount(history) }
        model?.echo(text)
        try await actions.prompt(taskID: taskID, text: text)
        return .sent
    }

    /// B4-2: "#调研员 …" or "#codex …" as a contract; nil when `target`
    /// names no role or harness (the text then goes to the agent as typed).
    func delegationContract(target: String, instruction: String) -> LiveDelegationContract? {
        if let agent = DelegationText.agent(for: target) {
            return LiveDelegationContract(instruction: instruction, agent: agent)
        }
        let key = target.lowercased()
        guard let role = delegationRoles.first(where: { $0.id.lowercased() == key || $0.name.lowercased() == key })
        else { return nil }
        return LiveDelegationContract(instruction: instruction, role: role.id)
    }

    /// B5-1: the "已记住" card.
    public func makeRememberedGlobal() async {
        guard let item = remembered else { return }
        do {
            try await actions.makeMemoryGlobal(id: item.id)
            remembered?.projectId = nil
        } catch {
            errorText = "改为全局失败：\(error)"
        }
    }

    public func undoRemembered() async {
        guard let item = remembered else { return }
        do {
            try await actions.undoMemory(id: item.id)
            remembered = nil
        } catch {
            errorText = "撤销失败：\(error)"
        }
    }

    public func dismissRemembered() { remembered = nil }

    /// B4-2: the delegation card's actions.
    public func stopDelegation(_ id: String) async {
        do { try await actions.stopDelegation(taskID: taskID, delegationID: id) } catch { errorText = "停止委派失败：\(error)" }
    }

    public func sendDelegation(_ id: String) async {
        do { try await actions.sendDelegation(taskID: taskID, delegationID: id) } catch { errorText = "发出委派结果失败：\(error)" }
    }

    public func ackDelegationProbes(_ id: String) async {
        do {
            try await actions.ackDelegationProbes(taskID: taskID, delegationID: id)
        } catch {
            errorText = "确认探针失败：\(error)"
        }
    }

    public func redoDelegation(_ id: String, note: String) async {
        do {
            try await actions.redoDelegation(taskID: taskID, delegationID: id, note: note)
        } catch {
            errorText = "退回重做失败：\(error)"
        }
    }

    /// B4-1: the review card's and the turn-change row's actions.
    public func requestReview() async {
        do { try await actions.requestReview(taskID: taskID) } catch { errorText = "请人审查失败：\(error)" }
    }

    public func sendReview() async {
        do { try await actions.sendReview(taskID: taskID) } catch { errorText = "发出审查意见失败：\(error)" }
    }

    public func retryReview() async {
        do { try await actions.retryReview(taskID: taskID) } catch { errorText = "重新审查失败：\(error)" }
    }

    public func setReviewPaused(_ paused: Bool) async {
        do {
            try await actions.setReviewPaused(taskID: taskID, paused)
            reviewPaused = paused
        } catch {
            errorText = "保存审查设置失败：\(error)"
        }
    }

    public func openTask(_ id: String) {
        actions.openTask(id)
    }

    /// B4′: start the task with the clarification card's ticked criteria.
    public func acceptClarification(_ acceptance: [LiveAcceptance], constraints: [String]) async {
        do {
            try await actions.acceptClarification(taskID: taskID, acceptance: acceptance, constraints: constraints)
        } catch {
            errorText = "开工失败：\(error)"
        }
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
