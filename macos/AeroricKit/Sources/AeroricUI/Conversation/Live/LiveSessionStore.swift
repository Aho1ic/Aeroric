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
    /// A6: up to `pages` host pages of history, whether older ones are left,
    /// and what of the transcript did not read.
    func history(taskID: String, pages: Int) async throws -> LiveHistory
    /// R1-2b: queued; `afterTurn` waits for the running turn to end. E12:
    /// `references` are tasks of the project picked with "@".
    func prompt(taskID: String, text: String, afterTurn: Bool, references: [String]) async throws -> LivePromptReceipt
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
    /// Review 2026-11-22: the task's latest check, for a page opened after it ran.
    func lastAutoCheck(taskID: String) async throws -> LiveSessionUpdate?
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
    /// B4′: settle one of the task's criteria by hand (`passed` / `failed`).
    func setObligation(taskID: String, id: String, state: String) async throws
    /// CU F2: this task's delegations, to put back cards still waiting on the user.
    func delegationCards(taskID: String) async throws -> [LiveDelegationCard]
    /// R1-2: this session's delivery queue, and acting on one item
    /// (`delivery.resume` / `.discard` / `.resend`).
    func deliveries(taskID: String) async throws -> [LiveDelivery]
    /// R1-3a: the capability table of `family`.
    func capabilities(family: String) async throws -> LiveCapabilities
    /// B2-4: the table for this task's place (an SSH project gets the
    /// host's remote table). Defaults to the local one.
    func capabilities(taskID: String, family: String) async throws -> LiveCapabilities
    /// R1-3a: this task's login, when the agent refused it.
    func expiredLogin(taskID: String) async throws -> ExpiredLogin?
    /// R2-1: the turns that changed files; plan or apply an undo; accept one.
    func turns(taskID: String) async throws -> [LiveTurnRecord]
    func turnPlan(taskID: String, turnID: String, action: String, apply: Bool) async throws -> LiveTurnPlan
    func acceptTurn(taskID: String, turnID: String) async throws
    func deliveryAction(_ method: String, id: String) async throws
    /// B5-1: `/记住 …` in this task's project; the item as kept.
    func remember(taskID: String, text: String, kind: String) async throws -> LiveMemoryItem
    /// B5-1: the "已记住" card's 改为全局 and 撤销.
    func makeMemoryGlobal(id: String) async throws
    func undoMemory(id: String) async throws
    /// Show another task (a reviewer, a parent).
    func openTask(_ taskID: String)
    /// R5-2: run a turn again with other configurations; the reruns so far;
    /// bring one run's copy back whole; drop a run's copy.
    func startRerun(taskID: String, turnID: String, message: String, original: String?, brief: Bool,
                    choices: [LiveRerunChoice]) async throws -> LiveRerunStarted
    func reruns(taskID: String) async throws -> [LiveRerunGroup]
    func bringAll(taskID: String) async throws -> (applied: Int, conflicts: Int)
    func removeCopy(taskID: String) async throws -> Int
    /// R5-2: read the task's project configurations again (the rerun sheet
    /// opens with what can start now, not what could when the App started).
    func refreshConfigurations(taskID: String) async
}

extension LiveSessionActions {
    public func startRerun(taskID: String, turnID: String, message: String, original: String?, brief: Bool,
                           choices: [LiveRerunChoice]) async throws -> LiveRerunStarted {
        throw LiveSessionError.unsupported("这个宿主不支持换模型重跑")
    }
    public func reruns(taskID: String) async throws -> [LiveRerunGroup] { [] }
    public func refreshConfigurations(taskID: String) async {}
    public func bringAll(taskID: String) async throws -> (applied: Int, conflicts: Int) {
        throw LiveSessionError.unsupported("这个宿主不支持带回")
    }
    public func removeCopy(taskID: String) async throws -> Int {
        throw LiveSessionError.unsupported("这个宿主不支持删除副本")
    }
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
    /// Hosts without paging hand everything over at once.
    public func history(taskID: String, pages: Int) async throws -> LiveHistory {
        let page = try await historyWithStats(taskID: taskID)
        return LiveHistory(messages: page.messages, stats: page.stats)
    }
    public func answer(taskID: String, questionID: String, value: String?, confirmed: Bool?, cancelled: Bool) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持问答卡")
    }
    public func revokeGrant(taskID: String, subject: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持撤销授权")
    }
    public func autoCheck(taskID: String) async throws -> LiveAutoCheckSelection? { nil }
    public func lastAutoCheck(taskID: String) async throws -> LiveSessionUpdate? { nil }
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
    public func deliveries(taskID: String) async throws -> [LiveDelivery] { [] }
    public func capabilities(family: String) async throws -> LiveCapabilities { LiveCapabilities() }
    public func capabilities(taskID: String, family: String) async throws -> LiveCapabilities {
        try await capabilities(family: family)
    }
    public func expiredLogin(taskID: String) async throws -> ExpiredLogin? { nil }
    public func turns(taskID: String) async throws -> [LiveTurnRecord] { [] }
    public func turnPlan(taskID: String, turnID: String, action: String, apply: Bool) async throws -> LiveTurnPlan {
        throw LiveSessionError.unsupported("这个宿主不支持撤销")
    }
    public func acceptTurn(taskID: String, turnID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持撤销")
    }
    public func deliveryAction(_ method: String, id: String) async throws {}
    public func ackDelegationProbes(taskID: String, delegationID: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持委派")
    }
    public func setObligation(taskID: String, id: String, state: String) async throws {
        throw LiveSessionError.unsupported("这个宿主不支持检查点")
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

/// A6 (2026-11-17): one read of a session's history.
public struct LiveHistory: Equatable, Sendable {
    public var messages: [SessionMessage]
    public var stats: SessionTranscriptStats?
    /// Older messages were left unread (the page offers 加载更早的消息).
    public var hasEarlier: Bool
    public var unreadable: SessionUnreadable?

    public init(messages: [SessionMessage], stats: SessionTranscriptStats? = nil, hasEarlier: Bool = false,
                unreadable: SessionUnreadable? = nil) {
        self.messages = messages
        self.stats = stats
        self.hasEarlier = hasEarlier
        self.unreadable = unreadable
    }

    /// Host pages a session first reads; each 加载更早的消息 adds as many.
    public static let pageStep = 24
}

/// One visible session: history + the live rows on top, polled while the page
/// is on screen (N4 D2: 50ms while a turn runs or an approval waits, 500ms idle).
@Observable
@MainActor
public final class LiveSessionStore {
    public let taskID: String
    public private(set) var history: [SessionMessage] = []
    /// A6: older messages were left unread; how many pages the reads take,
    /// kept across the reload after each turn; what of the transcript did not read.
    public private(set) var hasEarlierHistory = false
    public private(set) var historyPages = LiveHistory.pageStep
    public private(set) var historyUnreadable: SessionUnreadable?
    public private(set) var loadingEarlier = false
    /// The whole session's usage and timing from its transcript.
    public private(set) var transcriptStats: SessionTranscriptStats?
    /// `history` as transcript items and as turns with their timing, rebuilt
    /// once per history read. 2026-11-12 (perf): the page derived them in its
    /// body — N+3 times per body for N turns, at ~20 bodies a second while a
    /// turn ran.
    public private(set) var historyItems: [TranscriptItem] = []
    public private(set) var historyTurns: [TranscriptTurn] = []
    public private(set) var model: LiveSessionModel?
    public private(set) var state: LiveSessionState?
    public private(set) var approvals = LiveApprovalMachine()
    public private(set) var loading = false
    /// Polling trouble, shown as a thin banner; cleared by the next good poll.
    public private(set) var connectionProblem: String?
    @ObservationIgnored private var pollFailures = 0
    public var errorText: String?
    /// B4-1: why the history could not be read; shown with 重试 instead of
    /// an empty page. Cleared by the next successful read.
    public private(set) var historyError: String?
    /// R1-3a: what this session's family can do; empty until loaded.
    public var capabilities = LiveCapabilities()
    /// R1-3a: this session's login, when the agent refused it.
    public var expiredLogin: ExpiredLogin?
    /// R2-1: this session's turns that changed files.
    public private(set) var turnRecords: [LiveTurnRecord] = []
    /// R5-2: this session's reruns, newest last.
    public private(set) var rerunGroups: [LiveRerunGroup] = []
    /// R5-2: a word after starting, bringing back or dropping a run.
    public var rerunNotice: String?
    /// A05: nil until read, or when the host does not offer auto checks.
    public private(set) var autoCheck: LiveAutoCheckSelection?
    /// B4-1: findings wait for the user before going to the writer.
    public private(set) var reviewPaused = false
    /// B4-2: the role library, for the composer's `#` suggestions.
    public private(set) var delegationRoles: [LiveDelegationRole] = []
    /// B5-1: what the last `/记住` kept (the "已记住" card), until dismissed.
    public private(set) var remembered: LiveMemoryItem?
    /// R1-1: sessions writing in this directory too, as of the last message.
    public private(set) var sharedWriters: [LiveSharedWriter] = []
    private var dismissedWriters: Set<String> = []
    /// One notice per writer; the session itself is never blocked.
    public var sharedWriter: LiveSharedWriter? {
        sharedWriters.first { !dismissedWriters.contains($0.taskId) }
    }
    public func dismissSharedWriter() {
        if let writer = sharedWriter { dismissedWriters.insert(writer.taskId) }
    }

    /// D 批: runaway reminders not yet dismissed, most telling first. A
    /// dismissal is kept per session and per kind, across launches.
    public func runawayAlerts(nowMs: UInt64 = UInt64(Date().timeIntervalSince1970 * 1000)) -> [LiveRunawayAlert] {
        let cards = (model?.items ?? []).compactMap { item -> LiveDelegationCard? in
            if case let .delegation(_, card) = item { return card } else { return nil }
        }
        let key = "\(history.count)|\(history.last?.messageId ?? "")"
        if runawayTurns?.key != key { runawayTurns = (key, LiveRunaway.turns(history)) }
        return LiveRunaway.alerts(stats: transcriptStats, history: history, delegations: cards, nowMs: nowMs,
                                  limits: .current(), judged: runawayTurns?.turns)
            .filter { !dismissedRunaway.contains($0.id) }
    }
    @ObservationIgnored private var runawayTurns: (key: String, turns: [LiveRunaway.Turn])?
    public func dismissRunaway(_ id: String) {
        dismissedRunaway.insert(id)
        UserDefaults.standard.set(Array(dismissedRunaway), forKey: runawayKey)
    }
    public func askForSummary() async {
        do { _ = try await send(LiveRunaway.summaryRequest) } catch { errorText = "发送失败：\(error)" }
    }
    private var runawayKey: String { "aeroric.session.runawayDismissed.\(taskID)" }
    private var dismissedRunaway: Set<String> = []

    @ObservationIgnored private let actions: any LiveSessionActions
    @ObservationIgnored private var baseline = 0

    public static let fastInterval: Duration = .milliseconds(50)
    public static let idleInterval: Duration = .milliseconds(500)
    public static let retryInterval: Duration = .seconds(1)

    public init(taskID: String, actions: any LiveSessionActions) {
        self.taskID = taskID
        self.actions = actions
        dismissedRunaway = Set(UserDefaults.standard.stringArray(forKey: runawayKey) ?? [])
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

    /// B2-2: bumped by `open()` and `resync()`; a poll that started before
    /// one of them drops what it read instead of writing an older model back.
    private var syncGeneration: UInt64 = 0
    /// B2-2: bumped by each history read; only the latest one writes.
    private var historyRead: UInt64 = 0
    /// B2-2: the one loop that follows running reruns.
    private var rerunPollTask: Task<Void, Never>?

    /// Snapshot + history; the cursor starts at the snapshot's seq.
    public func open() async {
        syncGeneration &+= 1
        loading = true
        defer { loading = false }
        do {
            let state = try await actions.state(taskID: taskID)
            self.state = state
            model = LiveSessionModel(state: state)
            retainApprovals(pending: state.pending)
            await reloadHistory()
            baseline = LiveHistoryMerge.assistantCount(history)
            // Review 2026-11-22: a check that ran before the page opened (a
            // commands-only one has no turn record to show it otherwise).
            if let update = try? await actions.lastAutoCheck(taskID: taskID) {
                model?.restoreAutoCheck(update)
            }
            await restoreDelegations()
            await restoreDeliveries()
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
        let generation = syncGeneration
        do {
            let batch = try await actions.updates(taskID: taskID, after: current.cursor)
            // B2-2: an open or resync while this waited holds the newer model.
            guard generation == syncGeneration else { return false }
            // Only real changes are written: @Observable notifies on every
            // assignment, and this runs every 50–500ms.
            if connectionProblem != nil { connectionProblem = nil }
            pollFailures = 0
            let wasActive = current.turnActive
            switch current.apply(batch) {
            case .needsResync:
                await resync()
                return false
            case let .applied(turnEnded):
                if !wasActive && current.turnActive { baseline = LiveHistoryMerge.assistantCount(history) }
                if stopping && !current.turnActive { stopping = false }
                if current != model { model = current }
                retainApprovals(pending: current.pending)
                trimAnswering(current.questions, resolved: batch.updates.compactMap {
                    if case let .questionResolved(id, _) = $0.kind { return id }
                    return nil
                })
                syncSendFailures(current)
                if turnEnded {
                    await settleTurn()
                    await refreshLogin()
                }
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
        syncGeneration &+= 1
        do {
            let state = try await actions.state(taskID: taskID)
            self.state = state
            var fresh = model ?? LiveSessionModel(state: state)
            fresh.resync(state)
            model = fresh
            retainApprovals(pending: state.pending)
            await reloadHistory()
            baseline = LiveHistoryMerge.assistantCount(history)
            // Review 2026-11-22: a check that ran before the page opened (a
            // commands-only one has no turn record to show it otherwise).
            if let update = try? await actions.lastAutoCheck(taskID: taskID) {
                model?.restoreAutoCheck(update)
            }
            await restoreDelegations()
            await restoreDeliveries()
        } catch {
            noteConnectionFailure(error)
        }
    }

    /// R1-2: the host's queue for this session (it survives restarts; the
    /// update log does not).
    private func restoreDeliveries() async {
        await refreshLogin()
        guard let list = try? await actions.deliveries(taskID: taskID) else { return }
        model?.restoreDeliveries(list)
    }

    /// R1-3a: the capability table (once) and whether the login still holds.
    func refreshLogin() async {
        if capabilities.isEmpty, let family,
           let table = try? await actions.capabilities(taskID: taskID, family: family) {
            capabilities = table
        }
        expiredLogin = (try? await actions.expiredLogin(taskID: taskID)) ?? nil
        await refreshTurns()
        Task { await refreshReruns() }
    }

    /// R2-1: the host records a turn once its snapshot is done, a moment
    /// after the turn ends — so a second look follows shortly.
    public func refreshTurns(again: Bool = true) async {
        if let list = try? await actions.turns(taskID: taskID) { turnRecords = list }
        guard again else { return }
        try? await Task.sleep(nanoseconds: 2_000_000_000)
        if let list = try? await actions.turns(taskID: taskID) { turnRecords = list }
    }

    /// R5-2: the sheet asks for a fresh catalog when it opens.
    public func refreshConfigurations() async {
        await actions.refreshConfigurations(taskID: taskID)
    }

    /// R5-2: read the reruns again; while one still runs, again in a while.
    /// B2-2: one loop at a time follows running reruns, however many reads
    /// start it.
    public func refreshReruns() async {
        guard let groups = try? await actions.reruns(taskID: taskID) else { return }
        rerunGroups = groups
        guard RerunText.anyRunning(groups), rerunPollTask == nil else { return }
        rerunPollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 5_000_000_000)
                guard let self, !Task.isCancelled else { return }
                guard let groups = try? await self.actions.reruns(taskID: self.taskID) else { continue }
                self.rerunGroups = groups
                if !RerunText.anyRunning(groups) { break }
            }
            self?.rerunPollTask = nil
        }
    }

    /// B2-2: stops following reruns (the page went away).
    public func stopFollowingReruns() {
        rerunPollTask?.cancel()
        rerunPollTask = nil
    }

    /// R5-2: nil when the runs started (some may have been refused, said in
    /// the notice); the reason when none did.
    public func startRerun(turnID: String, message: String, original: String? = nil, brief: Bool,
                           choices: [LiveRerunChoice]) async -> String? {
        do {
            let started = try await actions.startRerun(taskID: taskID, turnID: turnID, message: message,
                                                       original: original,
                                                       brief: brief, choices: choices)
            rerunNotice = RerunText.started(started)
            Task { await refreshReruns() }
            return nil
        } catch {
            return "重跑没有开始：\(error.localizedDescription)"
        }
    }

    /// R5-2: bring `run` back whole, first undoing the original turn when asked.
    public func bringBackRerun(_ run: LiveRerunRun, undoOriginal turnID: String?) async {
        if let turnID, !(await applyTurn(turnID, action: "undo")) { return }
        do {
            let result = try await actions.bringAll(taskID: run.taskId)
            rerunNotice = RerunText.broughtBack(result, label: run.label)
        } catch {
            errorText = "带回失败：\(error.localizedDescription)"
        }
        await refreshTurns()
    }

    /// R5-2: drop the copies of the runs the user did not keep.
    public func dropReruns(_ runs: [LiveRerunRun]) async {
        var dropped = 0
        for run in runs where !run.gone {
            do {
                _ = try await actions.removeCopy(taskID: run.taskId)
                dropped += 1
            } catch {
                errorText = "删除副本失败：\(error.localizedDescription)"
            }
        }
        if dropped > 0 { rerunNotice = "已丢弃 \(dropped) 个重跑的副本，会话仍在侧栏" }
        await refreshReruns()
    }

    public func planTurn(_ turnID: String, action: String) async -> LiveTurnPlan {
        do {
            return try await actions.turnPlan(taskID: taskID, turnID: turnID, action: action, apply: false)
        } catch {
            return LiveTurnPlan(blocked: error.localizedDescription)
        }
    }

    /// Undo (or take back); true when it went through.
    public func applyTurn(_ turnID: String, action: String) async -> Bool {
        defer { Task { await refreshTurns(again: false) } }
        do {
            let done = try await actions.turnPlan(taskID: taskID, turnID: turnID, action: action, apply: true)
            // A7: a half-done undo answers with what failed; it is not done.
            if let halfDone = TurnLedgerText.halfDone(done, action: action) {
                errorText = halfDone
                return false
            }
            // 目检 2026-11-17: the turn's footer (产品失败 · 已撤销) is graded
            // from the ledger when history is read; read it again, or a
            // turn taken back keeps saying it was undone.
            await reloadHistory()
            return true
        } catch {
            errorText = error.localizedDescription
            return false
        }
    }

    public func acceptTurn(_ turnID: String) async {
        do {
            try await actions.acceptTurn(taskID: taskID, turnID: turnID)
        } catch {
            errorText = error.localizedDescription
        }
        await refreshTurns(again: false)
    }

    /// R1-2: the bar above the composer, when something did not simply arrive.
    public var deliveryNotice: DeliveryNotice? {
        DeliveryText.notice(model.map { Array($0.deliveries.values) } ?? [])
    }

    public func actOnDeliveries(_ action: DeliveryNotice.Action, ids: [String]) async {
        for id in ids {
            do {
                try await actions.deliveryAction(DeliveryText.method(action), id: id)
            } catch {
                errorText = "操作失败：\(error)"
                break
            }
        }
        await restoreDeliveries()
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

    /// B2-2: true when the read landed; a newer read makes an older one's
    /// reply moot.
    @discardableResult
    private func reloadHistory() async -> Bool {
        historyRead &+= 1
        let read = historyRead
        do {
            let page = try await actions.history(taskID: taskID, pages: historyPages)
            guard read == historyRead else { return false }
            history = page.messages
            if hasEarlierHistory != page.hasEarlier { hasEarlierHistory = page.hasEarlier }
            if historyUnreadable != page.unreadable { historyUnreadable = page.unreadable }
            if let stats = page.stats { transcriptStats = stats }
            historyItems = TranscriptBuilder.items(from: history, taskRunning: false)
            historyTurns = TranscriptTurns.attach(transcriptStats?.turns ?? [], to: TranscriptTurns.group(historyItems))
            if historyError != nil { historyError = nil }
            return true
        } catch {
            guard read == historyRead else { return false }
            historyError = "历史载入失败：\(error)"
            return false
        }
    }

    private func retainApprovals(pending: [LivePendingPermission]) {
        var next = approvals
        next.retain(pending: pending)
        if next != approvals { approvals = next }
    }

    public func retryHistory() async {
        await reloadHistory()
        baseline = LiveHistoryMerge.assistantCount(history)
    }

    /// A6: read `pageStep` more host pages, older first; later reloads keep
    /// the depth.
    public func loadEarlierHistory() async {
        guard hasEarlierHistory, !loadingEarlier else { return }
        loadingEarlier = true
        defer { loadingEarlier = false }
        historyPages += LiveHistory.pageStep
        // B2-2: a failed read keeps the depth that was shown.
        guard await reloadHistory() else {
            historyPages -= LiveHistory.pageStep
            return
        }
        baseline = LiveHistoryMerge.assistantCount(history)
    }

    public func dismissHistoryError() { historyError = nil }

    // MARK: - User actions

    /// Sends the composer text, or runs it when it is one of the five commands.
    public func send(_ text: String, afterTurn: Bool = false, references: [String] = []) async throws -> LiveSendResult {
        if let command = SlashCommand.parse(text) {
            switch command {
            case .compact:
                try await actions.command(taskID: taskID, command: .compact)
                return .ranCommand
            case .review:
                let review = capabilities.entry("review")
                guard review.map({ $0.state != "no" }) ?? (family == "codex") else {
                    throw LiveSessionError.unsupported(review?.note ?? "/review 仅 Codex 支持")
                }
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
        let waits = afterTurn && turnActive
        if !turnActive { baseline = LiveHistoryMerge.assistantCount(history) }
        let receipt = try await actions.prompt(taskID: taskID, text: text, afterTurn: waits, references: references)
        model?.echo(text, afterTurn: waits, delivery: receipt.deliveryId)
        sharedWriters = receipt.sharedWriters
        return .sent
    }

    /// R1-2b: 撤回 a message of the user's that has not gone out.
    public func withdraw(_ deliveryID: String) async {
        await actOnDeliveries(.discard, ids: [deliveryID])
    }

    /// R1-2b: 修改 — out of the queue; the caller puts the text back in the
    /// composer. Nil when it could not be taken back.
    public func takeBack(_ deliveryID: String) async -> String? {
        // E12: the person's words, without the referenced tasks in front.
        guard let text = model?.deliveries[deliveryID].map({ AttachedContext.typed($0.text) }) else { return nil }
        do {
            try await actions.deliveryAction(DeliveryText.method(.discard), id: deliveryID)
        } catch {
            errorText = "修改失败：\(error)"
            return nil
        }
        await restoreDeliveries()
        return text
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

    /// Review 2026-11-23 (1123b): settle a delegation's criterion from its
    /// card. The criterion lives on the child's checkpoint; reading the
    /// delegations again has the host recompute the card (and drop the
    /// "还没有自动核对" probe once all pass), and the fresh card replaces
    /// the shown one at once.
    public func settleDelegationAcceptance(_ card: LiveDelegationCard, id: String, state: String) async {
        guard let child = card.childTaskId else { return }
        do {
            try await actions.setObligation(taskID: child, id: id, state: state)
            let cards = try await actions.delegationCards(taskID: taskID)
            if let fresh = cards.first(where: { $0.delegationId == card.delegationId }) {
                model?.updateDelegation(fresh)
            }
        } catch {
            errorText = "标记验收失败：\(error)"
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

    /// E11: stop was asked and the turn has not ended yet (正在停止).
    public private(set) var stopping = false

    public func interrupt() async {
        stopping = true
        do {
            try await actions.interrupt(taskID: taskID)
        } catch {
            stopping = false
            errorText = "停止失败：\(error)"
        }
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
        model?.clearSendFailure(id)
        do {
            try await actions.respond(taskID: taskID, requestID: id, decision: submission.decision,
                                      reason: submission.reason)
        } catch {
            // A2: a timeout is "not known", never resent on its own.
            approvals.submitFailed(id, .classify(error))
        }
    }

    /// A2: what the host said it could not write to the agent goes on the card.
    /// A2: answers the host could not write keep their card, with the reason.
    /// Written back only when it changes, like `retainApprovals`: a poll
    /// runs every 50 ms in a turn and every write redraws the page.
    private func syncSendFailures(_ model: LiveSessionModel) {
        guard !model.sendFailures.isEmpty else { return }
        var next = approvals
        for (id, reason) in model.sendFailures { next.hostFailed(id, message: reason) }
        if next != approvals { approvals = next }
    }

    /// Question cards being sent (disabled until the host resolves them).
    public private(set) var answering: Set<String> = []

    /// B2-2: a card the host resolved, or no longer lists, is done; its id
    /// may come back for a new question (`q1` again, even in the same batch)
    /// and must be answerable.
    private func trimAnswering(_ open: [LivePendingQuestion], resolved: [String]) {
        let kept = answering.subtracting(resolved).intersection(open.map(\.id))
        if kept != answering { answering = kept }
    }

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
