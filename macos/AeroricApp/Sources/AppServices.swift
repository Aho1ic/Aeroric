import AeroricClient
import AppKit
import AeroricProtocol
import AeroricUI
import Foundation
import Observation

/// App-scoped services. P4-02 adds the single supervisor/client pair:
/// `startSupervisor()` spawns the headless host (`aeroric-cli serve`) from
/// the bundle and wires `ConnectionStore` as the single consumer of both
/// phases and events.
///
/// Data directory: the explicitly injected `AERORIC_DATA_DIR` wins; a plain
/// launch derives the default under `Application Support` (relative to the
/// standard location, never an absolute path baked into source). The host is
/// the only tenant of that directory — the app itself never reads it; it
/// only passes it through.
@MainActor
@Observable
final class AppServices {
    /// What the caller injected for this launch. `injectedExplicitly == false`
    /// means a plain launch (Xcode Run, Finder).
    struct PreviewConfiguration: Equatable, Sendable {
        var environmentLabel: String
        var dataDirectoryPath: String?
        var injectedExplicitly: Bool
    }

    private(set) var configuration: PreviewConfiguration
    /// Wire version this build speaks, straight from AeroricKit (v1).
    let protocolVersion: Int

    let connectionStore = ConnectionStore()
    let applicationStore: ApplicationStore
    let projectStore = ProjectStore()
    /// P5-03：项目/任务镜像（宿主快照 → TaskStore）。
    let taskStore = TaskStore()
    /// AP4: 全业务域 Store 挂载（统一由 AppServices 持有，消除仅存于库目录的断链）。
    let fileTreeStore = FileTreeStore()
    let documentStore = DocumentStore()
    let gitStore = GitStore()
    let diagnosticsStore = DiagnosticsStore()
    let debugSessionStore = DebugSessionStore()
    let breakpointStore = BreakpointStore()
    let sshConnectionStore = AeroricUI.ConnectionStore()
    let infrastructureStore = InfrastructureStore()
    let databaseStore = DatabaseStore()
    let notebookStore = NotebookStore()
    let syncStore = SyncStore()
    let extensionStore = ExtensionStore()
    let remoteStore = RemoteStore(client: nil)
    let maintenanceStore = MaintenanceStore(client: nil)
    /// AP7:会话控制簿记(能力探测结果 + 控制裁决)。
    let agentToolsStore = AgentToolsStore()
    /// B1:旧版自定义 Agent 清理(设置 → Agents)。按握手广告的能力决定可用性。
    let legacyCleanupStore = LegacyCleanupStore()
    /// Imported Claude/Codex sessions (M4); gated on the host's `imports` capability.
    let importBook = ImportBook()
    let agentUpgradeBook = AgentUpgradeBook()
    /// SSH hosts (连接 page, open-remote-project sheet), 2026-10-27.
    let sshHostsStore = SshHostsStore()
    /// The sidebar's "打开远程项目…" asks the workspace to show its sheet.
    var openRemoteRequested = false
    let keepAwakeStore = KeepAwakeStore()
    /// Canvas boards and node layout (M5); gated on the host's `canvas` capability.
    let canvasBook = CanvasBook()
    /// AP7:外部协议只读状态(OAuth/沙箱/工具/媒体/语音)。
    let externalProtocolStore = ExternalProtocolStore()
    /// P5-01/P5-02：模式控制器（迁移默认按旧客户端存在性）。
    /// P5-03：类型化 workspace 调用（连接附着后创建）。
    private(set) var workspaceClient: WorkspaceClient?
    private(set) var taskSnapshotReadState: WorkspaceReadState = .notLoaded
    private(set) var configurationReadStates: [String: WorkspaceReadState] = [:]
    private var projectConfigurations: [String: [SessionConfiguration]] = [:]
    private var configurationReads: [String: UUID] = [:]
    private(set) var configurationEpoch = UUID()
    // MARK: N4 native sessions (state; behaviour in LiveSessions.swift)

    /// The host advertised `session.live`.
    var liveCapability = false
    /// Per task: does it run as a structured session (host's `session.state.structured`).
    var liveStructured: [String: Bool] = [:]
    @ObservationIgnored var liveStores: [String: LiveSessionStore] = [:]
    /// A window should select this (a clicked notification, `/clear`).
    var requestedSelection: String?
    /// Task whose new-session draft `/clear` asked for (prefilled like it).
    var requestedNewSessionLike: String?
    /// Approval card to scroll to, per task (a clicked notification).
    var approvalFocus: [String: String] = [:]
    /// R8: task id → the fallback model the gateway last answered it with.
    var downgrades: [String: AgentConnectionsClient.Downgrade] = [:]
    /// Bumped when connections or Agent configs change; the new-session
    /// picker reloads its catalogue on it.
    var agentCatalogEpoch = UUID()
    /// A05 → A19: task id → its latest auto-check run, when that run failed.
    var checkFailures: [String: AttentionQueue.CheckFailure] = [:]
    @ObservationIgnored var statusPumpTask: Task<Void, Never>?
    @ObservationIgnored var statusCursor: UInt64?
    @ObservationIgnored var notificationGate = ApprovalNotificationGate()
    @ObservationIgnored let notifier = SessionNotifier()

    /// P5-01：选中任务的终端轮询任务（attach/detach 管理）。
    private var terminalPollTask: Task<Void, Never>?
    private(set) var terminalCoordinator: TerminalCoordinator?
    private(set) var terminalPresentationOwner: UUID?
    /// 终端流的续看簿记（与任务镜像分开）。
    let terminalStore = TerminalStore()
    private(set) var supervisor: HostSupervisor?
    /// The live channel; retained so the sink stays wired between attempts.
    private(set) var attachedConnection: HostConnection?
    /// A read-only boundary, not a global resource/selection store.
    var projectResourceReader: ProjectResourcesClient? {
        guard phaseSnapshot == .ready, let attachedConnection else { return nil }
        return ProjectResourcesClient(connection: attachedConnection)
    }
    /// AP3: Persistent security-scoped bookmark store for sandboxed project access.
    private(set) var bookmarkStore: SandboxBookmarkStore?
    /// AP3: Explicit opt-in background host policy (default keepAliveOnUIQuit = false).
    var backgroundPolicy = BackgroundHostPolicy()
    init(configuration: PreviewConfiguration, protocolVersion: Int) {
        self.configuration = configuration
        self.protocolVersion = protocolVersion
        self.applicationStore = ApplicationStore(
            environmentTag: configuration.environmentLabel,
            dataDirPath: configuration.dataDirectoryPath,
            injectedExplicitly: configuration.injectedExplicitly
        )
    }

    /// Reads only the explicitly injected environment keys. No defaults that
    /// touch user data; nothing is created or opened here.
    static func resolvingInjectedPreviewConfiguration(
        environment: ProcessInfo = .processInfo
    ) -> AppServices {
        let dataDirectory = environment.environment["AERORIC_DATA_DIR"]
        let injectedEnvironment = environment.environment["AERORIC_ENV"]
        let configuration = PreviewConfiguration(
            environmentLabel: injectedEnvironment ?? "default",
            dataDirectoryPath: dataDirectory,
            injectedExplicitly: dataDirectory != nil || injectedEnvironment != nil
        )
        return AppServices(configuration: configuration, protocolVersion: ProtocolMarker.version)
    }

    /// The host's tenant directory: injected path or the app-support default.
    /// Deriving the default only at supervisor start keeps plain launches
    /// (no supervisor yet) free of any filesystem touch.
    var dataDirectory: URL {
        SandboxDataRoot.resolve(injectedPath: configuration.dataDirectoryPath)
    }

    /// Build identifier for the bootstrap surface: version-config+build.
    var buildIdentifier: String {
        #if DEBUG
        let configurationName = "debug"
        #else
        let configurationName = "release-local"
        #endif
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown"
        return "\(version)-\(configurationName)+\(build)"
    }

    /// Idempotent: repeated `.task`/`.onChange(.active)` calls are no-ops once
    /// a supervisor exists (its own run loop lives until `stopSupervisor`).
    func startSupervisor() async {
        guard supervisor == nil else { return }
        killStaleTestServesIfRequested()
        let dataDir = dataDirectory
        do {
            try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            // The supervisor degrades on its own if the directory is unusable;
            // surfacing a second failure mode here adds nothing.
        }
        let bookmarks = SandboxBookmarkStore(dataDir: dataDir)
        _ = bookmarks.restoreAllBookmarks()
        self.bookmarkStore = bookmarks
        self.backgroundPolicy = BackgroundHostPolicy.load(from: dataDir)
        let store = connectionStore
        let supervisor = HostSupervisor(
            config: SupervisorConfig(
                dataDir: dataDir,
                socketPath: SandboxDataRoot.socketPath(for: dataDir),
                environment: Self.childEnvironment()
            ),
            onChange: { [weak self] phase in
                Task { @MainActor in
                    self?.connectionStore.applyPhase(phase)
                }
            },
            // Strong capture: this closure lives inside the supervisor we
            // hold; the app services object is process-lifetime anyway.
            configureConnection: { connection in
                await connection.setEventSink { frame in
                    Task { @MainActor in
                        store.ingest(frame)
                    }
                }
                // Runs after start(), so `self.supervisor` is already assigned.
                await MainActor.run {
                    self.attachedConnection = connection
                    self.connectionStore.attach(connection: connection, supervisor: self.supervisor)
                    self.workspaceClient = WorkspaceClient(connection: connection)
                    self.configurationReads.removeAll()
                    self.configurationReadStates.removeAll()
                    self.projectConfigurations.removeAll()
                    self.configurationEpoch = UUID()
                }
            },
            // After the handshake: before it the advertised capabilities are
            // empty and every request fails `notConnected` (N6a: the native
            // session page, imports and the canvas were never enabled).
            connectionReady: { connection in
                let advertised = await connection.advertisedCapabilities
                await MainActor.run {
                    self.legacyCleanupStore.attach(
                        client: LegacyCleanupClient(connection: connection),
                        capabilityEnabled: advertised[LegacyCleanupContract.capability] == true
                    )
                    self.importBook.attach(
                        client: ImportsClient(connection: connection),
                        capabilityEnabled: advertised[ImportsClient.capability] == true
                    )
                    // The import picker may add projects; show them at once.
                    self.importBook.onProjectsChanged = { [weak self] in await self?.refreshWorkspace() }
                    self.sshHostsStore.attach(client: SshClient(connection: connection))
                    self.agentUpgradeBook.attach(
                        client: AgentUpgradeClient(connection: connection),
                        capabilityEnabled: advertised[AgentUpgradeClient.capability] == true
                    )
                    self.canvasBook.attach(
                        client: CanvasClient(connection: connection),
                        capabilityEnabled: advertised[CanvasClient.capability] == true
                    )
                    self.keepAwakeStore.attach(
                        client: PowerClient(connection: connection),
                        capabilityEnabled: advertised[PowerClient.capability] == true
                    )
                    self.liveCapability = advertised[SessionsClient.liveCapability] == true
                    self.liveStructured.removeAll()
                    self.liveStores.removeAll()
                    self.notifier.install()
                    self.notifier.onOpen = { [weak self] taskID in self?.openFromNotification(taskID: taskID) }
                    self.startStatusPump()
                }
                // AP7:能力探测走宿主真值。没有适配器面时 `capabilities()` 抛错,
                // store 保持「未探测」——UI 因此什么都不显示,而不是显示一堆注定
                // 失败的按钮。
                if let report = try? await AdapterClient(connection: connection).capabilities() {
                    await MainActor.run {
                        self.agentToolsStore.attach(report)
                    }
                }
                await self.refreshWorkspace()
            }
        )
        self.supervisor = supervisor
        await supervisor.start()
    }

    /// Synchronous host teardown for `applicationWillTerminate` (P4-04). The
    /// delegate runs on the main thread with no async context; awaiting the
    /// actor from a blocked main thread deadlocks, so the child is killed
    /// through the supervisor's nonisolated sync path instead. The run loop's
    /// own cleanup paths stay primary while the process is alive.
    func terminateHostForProcessExit() {
        let keepAlive = backgroundPolicy.keepAliveOnUIQuit
        supervisor?.terminateChildForProcessExit(keepAliveInBackground: keepAlive)
        if !keepAlive {
            bookmarkStore?.stopAllAccess()
        }
    }

    func setBackgroundKeepAliveEnabled(_ enabled: Bool) {
        backgroundPolicy.keepAliveOnUIQuit = enabled
        try? backgroundPolicy.save(to: dataDirectory)
    }

    @discardableResult
    func stopBackgroundHostService() async -> BackgroundStopImpact {
        let wasKeepAlive = backgroundPolicy.keepAliveOnUIQuit
        backgroundPolicy.keepAliveOnUIQuit = false
        try? backgroundPolicy.save(to: dataDirectory)
        let pid = (await supervisor?.currentReadyInfo?.pid).map(Int32.init)
        let wasRunning = supervisor != nil
        await stopSupervisor()
        bookmarkStore?.stopAllAccess()
        return BackgroundStopImpact(
            wasRunning: wasRunning,
            backgroundKeepAliveWasEnabled: wasKeepAlive,
            terminatedPID: pid
        )
    }

    func stopSupervisor() async {
        guard let supervisor else { return }
        self.supervisor = nil
        await supervisor.stop()
    }

    @discardableResult
    func authorizeAndOpenProject(url: URL) async throws -> ProjectStore.ProjectEntry {
        if let bookmarkStore {
            let state = try bookmarkStore.registerAuthorizedDirectory(url)
            if case .revoked = state {
                throw ProjectError.notConnected
            }
        }
        return try await openProject(at: url.path)
    }

    /// Environment for the child: whitelist, never a blind passthrough. The
    /// host derives everything from `--data-dir`; only test runners need to
    /// see the runner's own markers.
    static func childEnvironment() -> [String: String] {
        var forwarded: [String: String] = [:]
        for (key, value) in ProcessInfo.processInfo.environment where key.hasPrefix("TEST_RUNNER_") {
            forwarded[key] = value
        }
        return forwarded
    }

    enum ProjectError: Error {
        case notConnected
        case malformedReply
    }

    /// UI-test hygiene (G41): XCUITest's `terminate()` hard-kills the app
    /// without running any delegate callback, so the previous test's serve
    /// can still be alive and holding this data dir's write lock — every
    /// supervisor attempt in this launch would then die on lock acquisition
    /// (serve stderr, 2026-10-02). The UI-test RUNNER is sandboxed and
    /// cannot spawn pkill; this app is not, so the sweep runs here, gated by
    /// the same hook env var and scoped to this data dir only.
    private func killStaleTestServesIfRequested() {
        guard ProcessInfo.processInfo.environment["AERORIC_TEST_OPEN_PROJECT"] != nil else { return }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/pkill")
        process.arguments = ["-f", "aeroric-cli --data-dir \(dataDirectory.path)"]
        try? process.run()
        process.waitUntilExit()
        Thread.sleep(forTimeInterval: 0.3)
    }

    // MARK: - P5-03 workspace（项目/任务镜像与刷新）

    /// 连接就绪后读取项目/任务镜像；不把初次快照冒充实时事件覆盖。
    func refreshWorkspace() async {
        guard let client = workspaceClient else { return }
        // Imports re-scan incrementally on the host when stale; cheap to ask.
        Task { await importBook.refresh() }
        taskSnapshotReadState = .loading
        if let projects = try? await client.listProjects() {
            for project in projects {
                _ = projectStore.upsertProject(id: project.id, path: project.path)
                _ = taskStore.upsertProject(id: project.id, path: project.path)
            }
        }
        do {
            let tasks = try await client.listTasks()
            for task in tasks {
                taskStore.upsertTask(TaskStore.TaskEntry(
                    id: task.id,
                    projectID: task.projectID,
                    title: task.title,
                    status: task.status,
                    agent: task.agent,
                    createdAtMs: task.createdAtMs,
                    updatedAtMs: task.updatedAtMs,
                    archived: task.archived,
                    starred: task.starred,
                    worktreePath: task.worktreePath,
                    worktreeBranch: task.worktreeBranch,
                    additions: task.additions,
                    deletions: task.deletions,
                    prompt: task.prompt
                ))
                if let generation = task.generation {
                    pendingGenerations[task.id] = generation
                }
            }
            taskSnapshotReadState = .loaded
        } catch {
            taskSnapshotReadState = .failed(String(describing: error))
        }
    }

    /// 创建任务：task.run，taskId 由窗口草稿 owner 在请求前铸造；
    /// 回执后镜像一个 pending 条目（下轮刷新校正）。
    @discardableResult
    func runTask(_ draft: TaskDraft, projectID: String, taskId: String) async throws -> String {
        guard let client = workspaceClient else {
            throw ProjectError.notConnected
        }
        guard let project = taskStore.projects.first(where: { $0.id == projectID }) else {
            throw ProjectError.notConnected
        }
        let run = WorkspaceClient.RunRequest(
            projectPath: project.path,
            agent: draft.agent,
            prompt: draft.prompt,
            taskId: taskId,
            model: draft.model,
            effort: draft.effort,
            permissionMode: draft.permissionMode,
            worktree: draft.worktree,
            attachmentPaths: draft.attachmentPaths
        )
        let returned = try await client.runTask(run)
        taskStore.upsertTask(TaskStore.TaskEntry(
            id: returned,
            projectID: projectID,
            title: draft.prompt.split(separator: "\n").first.map(String.init) ?? returned,
            status: "pending",
            agent: draft.agent,
            prompt: draft.prompt
        ))
        return returned
    }

    func sessionConfigurations(projectID: String?) -> [SessionConfiguration] {
        projectID.flatMap { projectConfigurations[$0] } ?? []
    }

    func loadSessionConfigurations(projectID: String) async {
        guard let client = workspaceClient else {
            configurationReadStates[projectID] = .unavailable
            return
        }
        guard configurationReadStates[projectID] != .loading else { return }
        let read = UUID()
        configurationReads[projectID] = read
        configurationReadStates[projectID] = .loading
        do {
            let catalog = try await client.configurations(projectID: projectID)
            guard configurationReads[projectID] == read else { return }
            projectConfigurations[projectID] = catalog.configurations.map {
                SessionConfiguration(
                    id: $0.id, version: $0.version, title: $0.displayName, agent: $0.agent,
                    model: nil, effort: nil,
                    options: .init(agents: [$0.agent], models: $0.models, efforts: $0.efforts,
                                   permissionModes: $0.permissionModes),
                    harness: $0.harness, blockedReasons: $0.blockedReasons,
                    requiresModel: true, requiresPermissionIdentity: true
                )
            }
            configurationReadStates[projectID] = .loaded
        } catch {
            guard configurationReads[projectID] == read else { return }
            configurationReadStates[projectID] = .failed(String(describing: error))
        }
    }

    func submissionRequest(_ draft: NewSessionDraft, projectID: String) throws -> NativeTaskSubmission {
        guard let configurationID = draft.configurationID,
              let configurationVersion = draft.configurationVersion,
              let permissionAgent = draft.permissionAgent else { throw ProjectError.malformedReply }
        return NativeTaskSubmission(
            submissionId: draft.submissionID, projectId: projectID,
            configurationId: configurationID, configurationVersion: configurationVersion,
            agent: draft.task.agent, prompt: draft.task.prompt,
            permissionMode: draft.task.permissionMode, permissionAgent: permissionAgent,
            selectedModel: draft.task.model, reasoningEffort: draft.task.effort,
            attachments: draft.task.attachmentPaths, worktree: draft.task.worktree,
            recipeId: draft.recipeID, team: draft.capabilities.team,
            review: draft.capabilities.review, evolution: draft.capabilities.evolution
        )
    }

    func submitSession(_ draft: NewSessionDraft, projectID: String) async throws -> NativeSubmissionReceipt {
        guard let client = workspaceClient else { throw ProjectError.notConnected }
        let receipt = try await client.submitTask(submissionRequest(draft, projectID: projectID))
        if receipt.state == "accepted" { await refreshWorkspace() }
        return receipt
    }

    func inspectSubmission(_ draft: NewSessionDraft, projectID: String) async throws -> NativeSubmissionReceipt? {
        guard let client = workspaceClient else { throw ProjectError.notConnected }
        let receipt = try await client.submission(submissionRequest(draft, projectID: projectID))
        if receipt?.state == "accepted" { await refreshWorkspace() }
        return receipt
    }

    func cancelTask(taskId: String) async {
        guard let client = workspaceClient else { return }
        try? await client.cancelTask(taskId: taskId)
    }

    /// P5-03 接线：星标/归档的窗口内即时反馈（镜像翻转）。宿主权威面
    /// （task.star/task.archive）随下一轮刷新校正；本操作不推断快照。
    func toggleStar(taskId: String) {
        if let index = taskStore.tasks.firstIndex(where: { $0.id == taskId }) {
            var entry = taskStore.tasks[index]
            entry.starred.toggle()
            taskStore.upsertTask(entry)
        }
    }

    /// 2026-10-27：归档写到宿主（task.archive → 任务行的 archivedAt），不再只是窗口内镜像。
    func toggleArchived(taskId: String) {
        guard let index = taskStore.tasks.firstIndex(where: { $0.id == taskId }) else { return }
        var entry = taskStore.tasks[index]
        entry.archived.toggle()
        taskStore.upsertTask(entry)
        let archived = entry.archived
        Task { @MainActor in
            try? await workspaceClient?.archiveTask(taskId: taskId, archived: archived)
            await refreshWorkspace()
        }
    }

    /// Sidebar context menu: 归档 / 取消归档 for tasks and imports, 移除 for imports.
    func sessionAction(_ id: String, _ action: SessionRowAction) {
        if let taskID = SessionNavigation.taskID(id) {
            let archived = taskStore.tasks.first { $0.id == taskID }?.archived ?? false
            if (action == .archive) != archived { toggleArchived(taskId: taskID) }
        } else if let importedID = SessionNavigation.importedID(id) {
            Task { @MainActor in
                switch action {
                case .archive: await importBook.setArchived(importedID, true)
                case .unarchive: await importBook.setArchived(importedID, false)
                case .remove: await importBook.remove(importedID)
                }
            }
        }
    }

    /// P5-03 接线：任务会话句柄。generation 只有快照带了 sessionId 才有
    /// 意义（宿主端会话已分配）；缺失返回 nil，UI 不挂终端。
    func generation(for task: TaskStore.TaskEntry) -> UInt64? {
        guard task.agent != nil else { return nil }
        // 会话句柄来自镜像外的权威查询：task.get（快照字段不上 store，
        // 避免 store 变成会话簿记）。
        return pendingGenerations[task.id]
    }

    /// task.get 回填的 generation 缓存（接线视图消费；不进 TaskStore）。
    var pendingGenerations: [String: UInt64] {
        get { _pendingGenerations }
        set { _pendingGenerations = newValue }
    }
    private var _pendingGenerations: [String: UInt64] = [:]

    // MARK: - P5-04 会话/审批（SessionsClient 接线）

    /// P5-04：会话/审批类型化客户端（连接附着后创建）。
    // MARK: - D: branch row above the new-session input

    /// Per project: its local branches, or nil when it is not a git repository.
    var branchLists: [String: NewSessionHome.BranchList] = [:]

    func loadBranches(projectID: String) async {
        guard let reader = projectResourceReader,
              let project = taskStore.projects.first(where: { $0.id == projectID }) else { return }
        if let branches = try? await reader.branches(projectPath: project.path) {
            branchLists[projectID] = .init(current: branches.first(where: \.current)?.name,
                                           names: branches.map(\.name))
        } else {
            branchLists[projectID] = nil
        }
    }

    /// Q10-a: switch only over a clean working tree, and only after the user
    /// confirms; the checkout runs in the project folder itself.
    func switchBranch(projectID: String, to branch: String) async {
        guard let reader = projectResourceReader, let connection = attachedConnection,
              let project = taskStore.projects.first(where: { $0.id == projectID }) else { return }
        let changed: Int
        do {
            changed = try await reader.changes(projectPath: project.path).count
        } catch {
            Self.alert("读不到工作区状态，没有切换分支", error.localizedDescription)
            return
        }
        if case let .blocked(reason) = BranchSwitch.decision(changedFiles: changed) {
            Self.alert("不能切换分支", reason)
            return
        }
        let confirm = NSAlert()
        confirm.messageText = "在“\(project.name)”中切换到 \(branch)？"
        confirm.informativeText = "会在项目目录 \(project.path) 执行 git checkout \(branch)。"
        confirm.addButton(withTitle: "切换")
        confirm.addButton(withTitle: "取消")
        guard confirm.runModal() == .alertFirstButtonReturn else { return }
        do {
            try await GitClient(connection: connection).checkoutBranch(projectPath: project.path, branch: branch)
        } catch {
            Self.alert("切换分支失败", error.localizedDescription)
        }
        await loadBranches(projectID: projectID)
    }

    private static func alert(_ title: String, _ detail: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = detail
        alert.runModal()
    }

    /// G: `agentConnections.*`, while attached.
    var agentConnectionsClient: AgentConnectionsClient? {
        guard let connection = attachedConnection else { return nil }
        return AgentConnectionsClient(connection: connection)
    }

    /// The Agent configuration page's state, made on first use and kept so a
    /// half-edited draft survives switching sections.
    var agentConfigStore: AgentConfigStore {
        if let store = agentConfigStoreStorage { return store }
        let store = AgentConfigStore(backend: HostAgentConfigBackend(services: self))
        store.onDocumentChange = { [weak self] in self?.agentCatalogEpoch = UUID() }
        agentConfigStoreStorage = store
        return store
    }
    @ObservationIgnored private var agentConfigStoreStorage: AgentConfigStore?

    var sessionsClient: SessionsClient? {
        guard let connection = attachedConnection else { return nil }
        return SessionsClient(connection: connection)
    }

    /// P5-04：DSH composer 发送（DSH prompt 走 DSH webui 命令通道；宿主
    /// prompt 投递判定在 core attachments.rs，客户端只传文本）。
    func sendDshPrompt(sessionTaskId: String, text: String) async throws {
        guard let connection = attachedConnection else { throw ProjectError.notConnected }
        _ = try await connection.request(
            "task.input",
            params: .object(["taskId": .string(sessionTaskId), "text": .string(text)])
        )
    }

    /// P5-04：审批/问题回执（approval.respond；outcome 值即结果本身）。
    func respondDshOutcome(eventId: String, outcome: [String: JSONValue]) async throws {
        guard let sessions = sessionsClient else { throw ProjectError.notConnected }
        try await sessions.respondApproval(eventId: eventId, outcome: outcome)
    }

    /// P5-04：会话导出（session.export）。
    func exportDshSession(taskId: String, out: String) async throws {
        guard let sessions = sessionsClient else { throw ProjectError.notConnected }
        try await sessions.exportSession(taskId: taskId, out: out)
    }

    /// P5-03 接线：该任务的宿主侧结构化能力是否已验证。P5-04 兑现：DSH
    /// 已验证 → native；其他家保守 false（只终端交互）。
    func structuredCapabilitiesVerified(_ task: TaskStore.TaskEntry) -> Bool {
        // P5-04 兑现：DSH 的结构化能力协商已通过（宿主 DshProtocolCapabilities
        // 快照 + 审批/问题 waterfall + composer 面 P5-04 已接通）；DSH 之外的
        // 家（claude/codex/omp）宿主只提供终端交互（AGENTS.md：omp 只走原生
        // PTY），保守 false。
        return task.agent == "dsh"
    }

    /// P5-03 接线：草稿选项集。宿主 capability 回答前＝空集（不伪造
    /// 可用项）；回答后由 refreshWorkspace 填充。
    var draftOptions: TaskDraft.Options {
        draftOptionsStorage ?? .none
    }
    var draftOptionsStorage: TaskDraft.Options?

    // MARK: - P5-01 终端挂载（plan_replay 轮询 + task.input 回填）

    /// 挂上选中任务的终端轮询：plan_replay（cursor 续读）50ms 一拍，
    /// 字节喂 coordinator；delegate 输入经 task.input 回填。
    func attachTerminal(taskId: String, generation: UInt64, presentationOwner: UUID) {
        detachTerminal()
        guard let client = workspaceClient else { return }
        let coordinator = TerminalCoordinator(taskID: taskId, generation: generation, store: terminalStore)
        coordinator.attach(holdsLease: true)
        coordinator.onInput = { [weak self, weak coordinator] taskId, generation, data in
            guard let self, let text = String(data: Data(data), encoding: .utf8) else { return }
            let bytes = Data(text.utf8)
            Task { [weak self, weak coordinator] in
                // Re-check at the async dispatch boundary: hiding a task
                // must also block input queued before the visibility change.
                // This local guard is not a host-authoritative lease ticket.
                guard let self, let coordinator,
                      self.terminalCoordinator === coordinator,
                      coordinator.holdsLease, !coordinator.isHidden,
                      coordinator.taskID == taskId, coordinator.generation == generation,
                      self.workspaceClient != nil else { return }
                // task.input（宿主侧 lease 短顶替语义）。
                _ = try? await attachedConnection?.request(
                    "task.input",
                    params: .object([
                        "taskId": .string(taskId),
                        "text": .string(text),
                        "bytes": .string(bytes.base64EncodedString()),
                    ])
                )
            }
        }
        terminalCoordinator = coordinator
        terminalPresentationOwner = presentationOwner
        terminalPollTask = Task { [weak self] in
            var cursor: TerminalStreamClient.Cursor = .init(bytes: 0)
            while !Task.isCancelled {
                if let connection = self?.attachedConnection,
                   let tail = try? await TerminalStreamClient(connection: connection).planReplay(
                       taskId: taskId, generation: generation, cursor: cursor
                   ) {
                    let coordinator = self?.terminalCoordinator
                    let tailCopy = tail
                    await MainActor.run {
                        coordinator?.ingest(TerminalCoordinator.PlanTail(
                            kind: tailCopy.kind.rawValue,
                            from: tailCopy.from,
                            bytes: tailCopy.bytes,
                            resumeAfter: tailCopy.resumeAfter,
                            reasonKind: tailCopy.reasonKind
                        ))
                    }
                    cursor = tail.nextCursor
                }
                try? await Task.sleep(nanoseconds: 50_000_000)
            }
        }
    }

    func detachTerminal() {
        terminalPollTask?.cancel()
        terminalPollTask = nil
        terminalCoordinator?.detach()
        terminalCoordinator = nil
        terminalPresentationOwner = nil
    }

    func suspendTerminalInput(presentationOwner: UUID) {
        guard terminalPresentationOwner == presentationOwner else { return }
        terminalCoordinator?.detach()
    }

    func detachTerminal(presentationOwner: UUID) {
        guard terminalPresentationOwner == presentationOwner else { return }
        detachTerminal()
    }

    /// UI-test hook (P4-04 acceptance: per-window selection needs real
    /// sidebar rows, and `NSOpenPanel` cannot be driven from XCUITest).
    /// `AERORIC_TEST_OPEN_PROJECT="path1:path2"` opens the listed directories
    /// through the real `project.open` RPC once the host is ready. Unset in
    /// every plain launch — production behaviour is untouched, and the paths
    /// only ever point at the run's scratch data dir.
    func openInjectedTestProjectsIfRequested() async {
        guard let raw = ProcessInfo.processInfo.environment["AERORIC_TEST_OPEN_PROJECT"],
              !raw.isEmpty else { return }
        // TEMPORARY DIAGNOSTIC (P4-04): the hook's RPC failures are invisible
        // under XCUITest (no app stderr), so the hook narrates to
        // `<dataDir>/test-hook.log`. Remove once the row flow is green.
        func hookLog(_ line: String) {
            let url = dataDirectory.appendingPathComponent("test-hook.log")
            let stamped = "\(Date().timeIntervalSince1970) \(line)\n"
            if let handle = try? FileHandle(forWritingTo: url) {
                defer { try? handle.close() }
                _ = try? handle.seekToEnd()
                _ = try? handle.write(contentsOf: Data(stamped.utf8))
            } else {
                try? Data(stamped.utf8).write(to: url)
            }
        }
        // Ready can arrive late on a busy machine; each open retries because
        // a transient `not connected` (phase flips between poll and call)
        // must not leave the sidebar empty for the whole test.
        let paths = raw.split(separator: ":", omittingEmptySubsequences: true).map(String.init)
        hookLog("hook start; paths=\(paths)")
        var lastPhase = ""
        for _ in 0..<300 {
            if attachedConnection != nil, case .ready = connectionStore.phase { break }
            if connectionStore.phaseText != lastPhase {
                lastPhase = connectionStore.phaseText
                hookLog("phase: \(lastPhase)")
            }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        guard attachedConnection != nil else {
            hookLog("give up: never ready; phase=\(connectionStore.phaseText)")
            return
        }
        for path in paths {
            for attempt in 0..<5 {
                do {
                    let entry = try await openProject(at: path)
                    hookLog("opened \(path) as \(entry.id)")
                    break
                } catch {
                    hookLog("open \(path) attempt \(attempt) failed: \(error)")
                    if attempt == 4 { break }
                    try? await Task.sleep(nanoseconds: 1_000_000_000)
                }
            }
        }
    }

    /// Opens a project through the host (`project.open`). ADR-013: the
    /// caller mints the request id, so the app-side `newId` travels in
    /// params and the host echoes the confirmed entry back.
    @discardableResult
    func openProject(at path: String) async throws -> ProjectStore.ProjectEntry {
        guard let connection = attachedConnection else {
            throw ProjectError.notConnected
        }
        let newId = UUID().uuidString
        let frame = try await connection.request(
            "project.open",
            params: .object(["path": .string(path), "newId": .string(newId)])
        )
        guard
            let id = frame.result["project"]?["id"], case let .string(projectId) = id,
            let rawPath = frame.result["project"]?["path"], case let .string(projectPath) = rawPath
        else {
            throw ProjectError.malformedReply
        }
        _ = taskStore.upsertProject(id: projectId, path: projectPath)
        return projectStore.upsertProject(id: projectId, path: projectPath)
    }
}

// MARK: - Shell state injection (P4-04)

/// Maps the client's `SupervisorPhase` onto the shell's snapshot enum at the
/// injection boundary (AeroricUI has no client dependency, INV-02).
extension AppServices: WorkspaceStateProviding {
    var phaseSnapshot: ConnectionPhaseSnapshot {
        switch connectionStore.phase {
        case .idle: .idle
        case .connecting: .connecting
        case .ready: .ready
        case let .degraded(reason): .degraded(reason: reason)
        // Attempt/delay stay in `phaseText`; the snapshot is deliberately
        // detail-free so the shell never re-formats supervisor internals.
        case let .reconnecting(attempt, nextDelayMs): .reconnecting
        case let .incompatible(reason): .incompatible(reason: reason)
        case .stopped: .stopped
        }
    }

    var phaseText: String { connectionStore.phaseText }
    var environmentLabel: String { configuration.environmentLabel }
    var protocolVersionText: String { "v\(protocolVersion)" }
    var applicationTag: String { applicationStore.environmentTag }

    /// The navigation spine: the workspace's own surfaces first (grouped by
    /// what they are for), then the user's projects.
    ///
    /// The workspace rows come first because the daily job is running and
    /// watching tasks; the projects follow as the scoping dimension. That
    /// ordering is the hierarchy the flat 12-segment strip could not express
    /// (see `WorkspaceSection`).
    var sidebarItems: [SidebarItem] {
        // The canvas row appears only when the host can store boards.
        let sections = WorkspaceSection.allCases.filter { $0 != .canvas || canvasBook.isAvailable }.map { section in
            SidebarItem(
                id: section.sidebarID,
                title: section.title,
                systemImage: section.systemImage,
                badge: section == .tasks ? openTaskCount : section == .attention ? attentionCount : nil,
                group: section.group
            )
        }
        let projects = projectStore.projects.map { project in
            SidebarItem(
                id: project.id,
                title: URL(fileURLWithPath: project.path).lastPathComponent,
                systemImage: "folder",
                group: .projects,
                children: [
                    SidebarItem(id: SessionNavigation.draft(projectID: project.id),
                                title: "新建会话", systemImage: "plus", group: .projects),
                ] + taskStore.tasks.filter { task in task.projectID == project.id && !task.archived }
                    .sorted { ($0.starred ? 0 : 1, $1.updatedAtMs ?? 0) < ($1.starred ? 0 : 1, $0.updatedAtMs ?? 0) }
                    .map { task in
                        SidebarItem(id: SessionNavigation.task(task.id), title: task.title,
                                    systemImage: "bubble.left", group: .projects,
                                    status: task.status, pinned: task.starred, agent: task.agent,
                                    updatedAt: task.updatedAtMs.map { Date(timeIntervalSince1970: Double($0) / 1000) })
                    }
                    + importBook.sessions(inProject: project.id).map(Self.importedItem)
            )
        }
        let unassigned = importBook.unassigned.map(Self.importedItem).map { item -> SidebarItem in
            var item = item
            item.group = .imports
            return item
        }
        let archivedTasks = taskStore.tasks.filter(\.archived).map { task in
            SidebarItem(id: SessionNavigation.task(task.id), title: task.title,
                        systemImage: "bubble.left", group: .archived,
                        status: task.status, agent: task.agent,
                        updatedAt: task.updatedAtMs.map { Date(timeIntervalSince1970: Double($0) / 1000) })
        }
        let archivedImports = importBook.archivedSessions.map(Self.importedItem).map { item -> SidebarItem in
            var item = item
            item.group = .archived
            return item
        }
        let archived = (archivedTasks + archivedImports)
            .sorted { ($0.updatedAt ?? .distantPast) > ($1.updatedAt ?? .distantPast) }
        return [SidebarItem(id: SessionNavigation.globalDraft, title: "新建会话",
                            systemImage: "plus", group: .work)] + sections + projects + unassigned + archived
    }

    /// B1 Git page (read-only): status, branches and history of `projectPath`.
    func refreshGit(projectPath: String) async {
        guard let connection = attachedConnection else { return }
        let client = GitClient(connection: connection)
        gitStore.beginRead()
        gitStore.ingest(stash: [])
        do {
            let status = try await client.status(projectPath: projectPath)
            gitStore.ingest(changes: status.map { GitStore.Change(path: $0.path, status: $0.status, staged: $0.staged) },
                            conflictPaths: [])
        } catch {
            gitStore.failChanges("读取更改失败：\(Self.gitReason(error))")
        }
        gitStore.setDiff("")
        if let branches = try? await client.branches(projectPath: projectPath) {
            gitStore.ingest(branches: branches.map { GitStore.Branch(name: $0.name, current: $0.current, remote: $0.remote) })
        }
        do {
            let commits = try await client.log(projectPath: projectPath)
            gitStore.ingest(commits: commits.map {
                GitStore.Commit(hash: $0.hash, shortHash: $0.shortHash, author: $0.author, date: $0.date, message: $0.message)
            })
        } catch {
            let reason = Self.gitReason(error)
            gitStore.failHistory(reason.contains("does not have any commits") ? "这个仓库还没有提交" : "读取历史失败：\(reason)")
        }
    }

    /// Files page (read-only): one directory level of `projectID`.
    func loadFiles(projectID: String, path: String) async {
        guard let connection = attachedConnection else { return }
        fileTreeStore.setLoading(path, loading: true)
        let entries = (try? await ProjectResourcesClient(connection: connection).directory(projectID: projectID, path: path)) ?? []
        fileTreeStore.ingestChildren(of: path, entries: entries.map {
            FileTreeStore.Node(name: $0.name, path: $0.path, isDir: $0.isDir, isSymlink: $0.isSymlink)
        })
    }

    func loadGitDiff(projectPath: String, change: GitStore.Change) async {
        guard let connection = attachedConnection else { return }
        do {
            gitStore.setDiff(try await GitClient(connection: connection)
                .fileDiff(projectPath: projectPath, filePath: change.path, staged: change.staged))
        } catch {
            gitStore.setDiff("读取 diff 失败：\(Self.gitReason(error))")
        }
    }

    private static func gitReason(_ error: Error) -> String {
        if case let .refused(failure)? = error as? ConnectionError { return failure.error.message }
        return String(describing: error)
    }

    /// A19 badge: how many tasks need the user; nil renders no zero.
    var attentionCount: Int? {
        let count = AttentionQueue.items(taskStore.tasks, checkFailures: checkFailures,
                                         nowMs: UInt64(Date().timeIntervalSince1970 * 1000)).count
        return count > 0 ? count : nil
    }

    static func importedItem(_ session: ImportsClient.ImportedSession) -> SidebarItem {
        SidebarItem(id: SessionNavigation.imported(session.id), title: session.title,
                    systemImage: "tray.and.arrow.down", group: .projects, agent: session.agent,
                    updatedAt: Date(timeIntervalSince1970: Double(session.lastAtMs) / 1000),
                    imported: true, path: session.cwd)
    }

    /// "添加为项目…" on an unassigned import: open its cwd as a project, then
    /// re-list so the host re-assigns those sessions to it.
    func addProject(path: String) {
        Task { @MainActor in
            _ = try? await openProject(at: path)
            await importBook.rescan()
        }
    }

    /// Trailing badge for the task row: how many tasks are live right now.
    /// `nil` when there are none, so the row renders no zero.
    ///
    /// "Live" is the protocol's own active set — `pending`, `running`,
    /// `input_required`, `detached` (`ProtocolModels.swift:16`,
    /// `TaskStatus::is_active` on the core side). Never a locally invented
    /// status list: the wire vocabulary is the authority and the client
    /// mirrors it.
    private var openTaskCount: Int? {
        let live: Set<String> = ["pending", "running", "input_required", "detached"]
        let count = taskStore.tasks.filter { live.contains($0.status) }.count
        return count > 0 ? count : nil
    }

    /// Why the status bar has no counts yet; nil once the mirror is loaded
    /// (the counts speak for themselves then).
    var statusContext: String? {
        guard taskSnapshotReadState == .loaded else {
            if case .failed = taskSnapshotReadState { return "任务状态读取失败（保留缓存）" }
            return "任务状态尚未确认"
        }
        return nil
    }

    /// Running / waiting counts from the cached task mirror. Event completeness
    /// and freshness are not established by the current typed API.
    var activityCounts: ActivityCounts? {
        guard taskSnapshotReadState == .loaded else { return nil }
        return ActivityCounts(
            running: taskStore.tasks.filter { ["pending", "running", "detached"].contains($0.status) }.count,
            waiting: taskStore.tasks.filter { $0.status == "input_required" }.count
        )
    }

    var paletteCommands: [PaletteCommand] {
        [
            PaletteCommand(id: "task.new", title: "新建会话"),
            PaletteCommand(id: "project.open", title: "打开项目…"),
            PaletteCommand(id: "settings.show", title: "设置…"),
        ] + Self.sectionCommands(canvasAvailable: canvasBook.isAvailable)
    }

    /// "前往：…" for every workspace section (the tools behind the toolbar's
    /// “更多” menu included); the canvas only when the host can store boards.
    static func sectionCommands(canvasAvailable: Bool) -> [PaletteCommand] {
        WorkspaceSection.allCases
            .filter { $0 != .canvas || canvasAvailable }
            .map { PaletteCommand(id: $0.sidebarID, title: "前往：\($0.title)") }
    }
}

extension AppServices: SettingsStateProviding {
    var legacyCleanup: LegacyCleanupStore? { legacyCleanupStore }
    var connectionPhaseText: String? { phaseText }
    var settingsImportBook: ImportBook? { importBook }
    var settingsKeepAwake: KeepAwakeStore? { keepAwakeStore }
    var settingsDiagnosticsExporter: (@MainActor () async throws -> DiagnosticsClient.Report)? {
        guard let connection = attachedConnection else { return nil }
        return { try await DiagnosticsClient(connection: connection).exportReport() }
    }
    var settingsOpenAgentConfig: (@MainActor () -> Void)? {
        { [weak self] in
            WorkspaceActions.showMainWindow()
            self?.requestedSelection = WorkspaceSection.agentConfigs.sidebarID
        }
    }
    var keepingAwake: Bool { keepAwakeStore.active }
}
