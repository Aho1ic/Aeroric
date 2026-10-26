import AeroricClient
import AeroricProtocol
import AeroricUI
import AppKit
import Foundation
import UserNotifications

/// N4 宿主接线：原生结构化会话页走 `session.*`（SessionsClient），视图只拿
/// `LiveSessionActions`，不持有连接（INV-02）。
@MainActor
struct HostLiveSessionActions: LiveSessionActions {
    let services: AppServices

    private func client() throws -> SessionsClient {
        guard let client = services.sessionsClient else { throw AppServices.ProjectError.notConnected }
        return client
    }

    func state(taskID: String) async throws -> LiveSessionState {
        let state = try await client().state(taskId: taskID)
        services.liveStructured[taskID] = state.structured
        return state
    }

    func updates(taskID: String, after: UInt64) async throws -> LiveUpdatesBatch {
        try await client().updates(taskId: taskID, after: after)
    }

    func history(taskID: String) async throws -> [AeroricUI.SessionMessage] {
        let page = try await client().historyPage(taskId: taskID, cursor: nil)
        return page.messages.map(SessionBlockMapping.message)
    }

    func prompt(taskID: String, text: String) async throws { try await client().prompt(taskId: taskID, text: text) }
    func interrupt(taskID: String) async throws { try await client().interrupt(taskId: taskID) }

    func respond(taskID: String, requestID: String, decision: LiveDecision, reason: String?) async throws {
        try await client().respond(taskId: taskID, requestId: requestID, decision: decision, reason: reason)
    }

    func configure(taskID: String, model: String?, permission: LivePermission?, effort: String?) async throws {
        try await client().configure(taskId: taskID, model: model, permission: permission, effort: effort)
    }

    func command(taskID: String, command: LiveCommand) async throws {
        try await client().command(taskId: taskID, command: command)
    }

    func close(taskID: String) async throws { try await client().close(taskId: taskID) }

    func export(taskID: String) async throws {
        guard let out = services.chooseSessionExportPath(taskID: taskID) else { return }
        try await client().exportSession(taskId: taskID, out: out)
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: out)])
    }

    func startNewSession(like taskID: String) { services.requestedNewSessionLike = taskID }

    func autoCheck(taskID: String) async throws -> LiveAutoCheckSelection? {
        let choice = try await client().autoCheck(taskId: taskID)
        return LiveAutoCheckSelection(tests: choice.tests, diagnostics: choice.diagnostics)
    }

    func setAutoCheck(taskID: String, _ selection: LiveAutoCheckSelection) async throws {
        try await client().setAutoCheck(taskId: taskID, tests: selection.tests, diagnostics: selection.diagnostics)
    }
}

extension AppServices {
    /// Where to save a session's Markdown export; nil when the user cancels.
    /// The export used to land in a temporary directory nobody was told about.
    func chooseSessionExportPath(taskID: String) -> String? {
        let title = taskStore.tasks.first { $0.id == taskID }?.title ?? "session"
        let safe = title.components(separatedBy: CharacterSet(charactersIn: "/:\\\n\r\t")).joined(separator: " ")
            .trimmingCharacters(in: .whitespaces)
        let panel = NSSavePanel()
        panel.nameFieldStringValue = String((safe.isEmpty ? "session" : safe).prefix(60)) + ".md"
        panel.canCreateDirectories = true
        panel.isExtensionHidden = false
        guard panel.runModal() == .OK, let url = panel.url else { return nil }
        return url.pathExtension.lowercased() == "md" ? url.path : url.path + ".md"
    }

    /// Asks the host whether this task runs as a structured session.
    /// A failed read is retried before the page falls back to the terminal: a
    /// task the App sees right after submitting may not be committed yet, and
    /// the connection may still be attaching. One failure used to pin the
    /// terminal page for the App's lifetime (N6a).
    func resolveLive(taskID: String) async {
        guard liveCapability, liveStructured[taskID] == nil else { return }
        for attempt in 0..<5 {
            if let client = sessionsClient, let state = try? await client.state(taskId: taskID) {
                if liveStructured[taskID] == nil { liveStructured[taskID] = state.structured }
                return
            }
            try? await Task.sleep(nanoseconds: 300_000_000 << UInt64(attempt))
            if Task.isCancelled || liveStructured[taskID] != nil { return }
        }
        liveStructured[taskID] = false
    }

    func liveStore(for taskID: String) -> LiveSessionStore {
        if let store = liveStores[taskID] { return store }
        let store = LiveSessionStore(taskID: taskID, actions: HostLiveSessionActions(services: self))
        liveStores[taskID] = store
        return store
    }

    func liveFacts(for task: TaskStore.TaskEntry) -> LiveSessionFacts {
        let configurations = sessionConfigurations(projectID: task.projectID).filter { $0.agent == task.agent }
        var models: [String] = []
        var efforts: [String] = []
        for configuration in configurations {
            for model in configuration.options.models where !models.contains(model) { models.append(model) }
            for effort in configuration.options.efforts where !efforts.contains(effort) { efforts.append(effort) }
        }
        let projectPath = taskStore.projects.first { $0.id == task.projectID }?.path
        return LiveSessionFacts(
            agent: task.agent, configurationTitle: configurations.first?.title,
            workingDirectory: task.worktreePath ?? projectPath, branch: task.worktreeBranch,
            startedAt: task.createdAtMs.map { Date(timeIntervalSince1970: Double($0) / 1000) },
            additions: task.additions, deletions: task.deletions, models: models, efforts: efforts,
            downgradeBadge: downgrades[task.id].map(AgentConfigText.downgradeBadge),
            downgradeHelp: downgrades[task.id].map(AgentConfigText.downgradeHelp)
        )
    }

    // MARK: - D4 task status pump

    /// Polls `events.since` once a second so background tasks' status reaches the
    /// sidebar and the board, and approvals can notify (N4 D4/D5).
    func startStatusPump() {
        statusPumpTask?.cancel()
        statusCursor = nil
        statusPumpTask = Task { @MainActor [weak self] in
            var tick = 0
            while !Task.isCancelled {
                await self?.pumpStatusOnce()
                // A14: the keep-awake indicator changes on the host's 2s watch; 5s is enough.
                if tick % 5 == 0 { await self?.keepAwakeStore.refresh() }
                // R8: which running tasks the gateway answers with a fallback model.
                if tick % 10 == 0 { await self?.refreshDowngrades() }
                // A05 → A19: failed auto-checks join the attention queue.
                if tick % 5 == 0 { await self?.refreshCheckFailures() }
                tick += 1
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    func refreshCheckFailures() async {
        guard let client = sessionsClient, let list = try? await client.autoCheckFailures() else { return }
        let next = list.mapValues { AttentionQueue.CheckFailure(summary: $0.summary, atMs: $0.atMs) }
        if next != checkFailures { checkFailures = next }
    }

    func refreshDowngrades() async {
        guard taskStore.tasks.contains(where: { $0.status == "running" }),
              let client = agentConnectionsClient,
              let list = try? await client.downgrades() else {
            if !downgrades.isEmpty, !taskStore.tasks.contains(where: { $0.status == "running" }) { downgrades = [:] }
            return
        }
        let next = Dictionary(list.map { ($0.taskId, $0) }, uniquingKeysWith: { _, last in last })
        if next != downgrades { downgrades = next }
    }

    func pumpStatusOnce() async {
        guard let connection = attachedConnection else { return }
        let after = statusCursor ?? 0
        guard let reply = try? await connection.request("events.since", params: .object(["after": .number(String(after))])),
              let batch = ReplayBatch.decode(reply.result) else { return }
        let latest = UInt64(batch.latestSeq.text) ?? after
        // First read only sets the cursor: the snapshot already came from task.list.
        guard statusCursor != nil, !batch.reset else {
            statusCursor = latest
            if batch.reset { await refreshWorkspace() }
            return
        }
        statusCursor = latest
        for event in batch.events where event.event == "task-status" {
            applyStatus(event.data)
        }
    }

    private func applyStatus(_ data: JSONValue) {
        guard let taskID = data["task_id"]?.stringValue, let status = data["status"]?.stringValue else { return }
        guard var task = taskStore.tasks.first(where: { $0.id == taskID }) else {
            Task { await refreshWorkspace() }
            return
        }
        let previous = task.status
        guard previous != status else { return }
        task.status = status
        taskStore.upsertTask(task)
        if notificationGate.shouldNotify(taskID: taskID, previous: previous, current: status,
                                         appActive: NSApp.isActive) {
            notifyApproval(task)
        }
    }

    // MARK: - D5 notifications

    private func notifyApproval(_ task: TaskStore.TaskEntry) {
        Task { @MainActor in
            let summary = try? await sessionsClient?.state(taskId: task.id).pending.first?.summary
            notifier.post(taskID: task.id, title: "\(task.title) 需要你批准", body: summary ?? "Agent 在等待你的回应")
        }
    }

    /// A clicked notification: bring the window forward on that task's approval card.
    func openFromNotification(taskID: String) {
        NSApp.activate(ignoringOtherApps: true)
        requestedSelection = SessionNavigation.task(taskID)
        Task { @MainActor in
            if let id = try? await sessionsClient?.state(taskId: taskID).pending.first?.id {
                approvalFocus[taskID] = id
            }
        }
    }
}

/// UNUserNotificationCenter for "needs your approval" (N4 D5). Authorization is
/// requested the first time one is needed; once declined it never asks again.
final class SessionNotifier: NSObject, UNUserNotificationCenterDelegate, @unchecked Sendable {
    static let taskKey = "taskID"
    @MainActor var onOpen: ((String) -> Void)?

    func install() {
        UNUserNotificationCenter.current().delegate = self
    }

    @MainActor
    func post(taskID: String, title: String, body: String) {
        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { settings in
            let deliver = {
                let content = UNMutableNotificationContent()
                content.title = title
                content.body = body
                content.sound = .default
                content.userInfo = [Self.taskKey: taskID]
                center.add(UNNotificationRequest(identifier: "approval.\(taskID)", content: content, trigger: nil))
            }
            switch settings.authorizationStatus {
            case .authorized, .provisional:
                deliver()
            case .notDetermined:
                center.requestAuthorization(options: [.alert, .sound]) { granted, _ in if granted { deliver() } }
            default:
                break
            }
        }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        let taskID = response.notification.request.content.userInfo[Self.taskKey] as? String
        completionHandler()
        guard let taskID else { return }
        Task { @MainActor in self.onOpen?(taskID) }
    }
}
