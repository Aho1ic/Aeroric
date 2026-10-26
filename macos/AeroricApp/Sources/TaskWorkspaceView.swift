import AeroricClient
import AeroricProtocol
import AeroricUI
import SwiftUI

/// P5-03 App 场景接线：宿主 ready 后，详情列从 shell 占位切到任务工作区
/// 视图——左侧任务列表（窗口局部过滤）＋右侧详情头；选中运行中任务挂
/// P5-01 终端（`SessionPresentation.resolve` 决定呈现：advanced 模式或
/// native 下能力已验证才直接呈现终端，否则显示状态卡与切换按钮）。
/// 任务生命周期调用全部走 `AppServices`（类型化 workspace client），
/// 本视图不触碰 wire 格式（INV-02 归属不变）。
/// The workspace's surfaces, as navigation rows rather than a tab strip.
///
/// ## Why these are rows and not segments (2026-10-13)
///
/// They used to be the 12 segments of one `NativeSegmentedPicker` across the
/// top of the detail column. Measured in the real window: 12 segments plus
/// the leaked group title spanning 1516pt, which put "run a task" at the
/// same visual weight as "运维" and made the daily job one of thirteen
/// peers — the "杂乱" the user reported.
///
/// The reference clients under `harness/` agree on the alternative:
/// **top-level navigation is a grouped sidebar; a tab strip is for open
/// documents** (`opencode` `titlebar.tsx:172-186` keeps session tabs in the
/// titlebar while navigation lives in the sidebar; `deepseek-harness`
/// `columns.ts:21-39`; `codeg`'s sidebar is `folders / chats / recent` —
/// `sidebar-view-mode-storage.ts:30,39`). So each surface becomes a
/// `SidebarItem` in one of the four groups, and the grouping is what carries
/// the hierarchy the flat strip had lost.
enum WorkspaceSection: String, CaseIterable, Hashable, Sendable {
    case tasks
    /// A19: tasks waiting on the user, and recent failures.
    case attention
    case canvas
    case files
    case git
    case ide
    case connections
    case infrastructure
    case database
    case notebook
    case extensions
    case remote
    case operations
    case agentTools

    /// The row label. A plain `String` (not a `LocalizedStringKey`) because
    /// the sidebar row model carries text, not view builders.
    var title: String {
        switch self {
        case .tasks: "任务"
        case .attention: "需要我处理"
        case .canvas: "画布"
        case .files: "文件"
        case .git: "Git"
        case .ide: "IDE"
        case .connections: "SSH"
        case .infrastructure: "设施"
        case .database: "数据库"
        case .notebook: "笔记"
        case .extensions: "扩展"
        case .remote: "远控"
        case .operations: "运维"
        case .agentTools: "Agent 工具"
        }
    }

    /// The sidebar row id. One namespace with the project rows, so the
    /// selection binding carries either kind and the shell stays ignorant of
    /// which is which.
    var sidebarID: String { "section.\(rawValue)" }

    /// Leading glyph for the navigation row.
    var systemImage: String {
        switch self {
        case .tasks: "checklist"
        case .attention: "bell.badge"
        case .canvas: "square.on.square.dashed"
        case .files: "folder"
        case .git: "arrow.triangle.branch"
        case .ide: "ladybug"
        case .connections: "network"
        case .infrastructure: "shippingbox"
        case .database: "cylinder"
        case .notebook: "book"
        case .extensions: "puzzlepiece.extension"
        case .remote: "antenna.radiowaves.left.and.right"
        case .operations: "gauge.with.dots.needle.67percent"
        case .agentTools: "wrench.and.screwdriver"
        }
    }

    /// Which labelled group the row sits under. The split is by *what the
    /// surface is for*, which is the hierarchy the flat strip could not
    /// express: the work, the project-scoped tools, and the machine-level
    /// configuration.
    /// Host wiring per section (M4b audit, research/sections-audit.md): only
    /// Agent tools gets real data today; the rest explain what they will do.
    var status: ToolSectionStatus {
        switch self {
        case .tasks, .attention, .canvas: .wired
        case .files: .partial("只读：浏览当前项目的目录，点文件预览。新建、重命名、删除与编辑下一批接入。")
        case .git: .partial("只读：查看当前项目的更改、逐文件 diff、分支与历史。暂存、提交、推送等写操作下一批接入。")
        case .ide: .comingSoon("问题列表、符号大纲与调试会话。")
        case .connections: .comingSoon("SSH 主机、主机密钥确认与 SFTP 文件传输。")
        case .infrastructure: .comingSoon("Docker 容器与镜像、端口占用、Conda 环境。")
        case .database: .comingSoon("SQLite、Redis、MongoDB 的浏览与查询。")
        case .notebook: .comingSoon("项目笔记、同步与知识检索。")
        case .extensions: .comingSoon("技能、MCP 服务器与 Hook 的管理。")
        case .remote: .comingSoon("与手机配对、设备管理与远程访问审计。")
        case .operations: .comingSoon("用量统计、诊断与通知中心。")
        case .agentTools: .partial("能力来自宿主对外部 Agent 的探测；具体操作由外部 Agent 自己执行。")
        }
    }

    var group: SidebarGroup {
        switch self {
        case .tasks, .attention, .canvas: .work
        case .files, .git, .ide, .database, .notebook, .agentTools: .tools
        case .connections, .infrastructure, .extensions, .remote, .operations: .system
        }
    }

    /// Resolve a sidebar row id back to its surface. `nil` for project rows
    /// and anything unrecognised — the caller decides the fallback.
    static func resolve(sidebarID: String?) -> WorkspaceSection? {
        guard let sidebarID, sidebarID.hasPrefix("section.") else { return nil }
        return WorkspaceSection(rawValue: String(sidebarID.dropFirst("section.".count)))
    }
}

struct TaskWorkspaceView: View {
    let services: AppServices
    /// The shell's sidebar selection. The navigation spine lives in the
    /// shell (`ProjectSidebar`); this view renders whichever surface the
    /// selected row names. A project row (or nothing) falls back to the task
    /// surface, which is the app's default view.
    @Binding var selection: String?
    /// 本窗口的项目作用域：侧栏选中一个**项目**行时＝那个项目，否则 nil。
    ///
    /// 窗口局部，不进 store——`TaskStore` 由所有窗口共享，把作用域存进去会让
    /// 两个窗口互相覆盖对方的选择（§2.4「selected project 归窗口」）。
    @State private var scopedProjectID: String?
    /// 每窗口选中的任务 id（窗口局部状态，不进 store）。
    @State private var selectedTaskID: String?
    @State private var visitedTaskIDs: Set<String> = []
    @State private var taskProjectContexts: [String: String] = [:]
    @State private var taskFilterText = ""
    @State private var draftBook = NewSessionDraftBook()
    @State private var projectContextID: String?
    @State private var resourceBook = ProjectResourceBook()
    @State private var taskInputGate = TaskPresentationInputGate()
    @State private var terminalPresentationOwner = UUID()
    @State private var visitedResourceProjects: Set<String> = []
    @Environment(\.mainShellBudget) private var shellBudget
    @Environment(\.mainShellNavigationEpoch) private var navigationEpoch
    @Environment(\.mainShellInspectorWidth) private var inspectorWidthPreference
    @AppStorage("settings.surfacePreference") private var surfaceRawValue = SurfacePreference.glass.rawValue
    @State private var submissionCheck: String?
    @State private var sqlDraft = "SELECT 1;"
    /// native 模式下能力未验证会话的用户主动切换（不自动改全局偏好，
    /// P5-02 语义）。
    @State private var forcedTerminalTaskIDs: Set<String> = []
    /// Q25: bottom terminal dock. Expanded by default at 28% of the column.
    @State private var terminalExpanded = true
    @SceneStorage("session.terminal-fraction") private var terminalFraction =
        Double(NativePanelGeometry.mainShell.terminalInitialFraction)
    /// Q17: codeg auxiliary panel tabs.
    @State private var auxiliaryTab: AuxiliaryTab = .details
    enum AuxiliaryTab: Hashable { case details, files, changes, history }

    /// Which surface the selected navigation row names. A project row or an
    /// empty selection is the task surface — the app's default view.
    private var activeSection: WorkspaceSection {
        WorkspaceSection.resolve(sidebarID: selection) ?? .tasks
    }

    var body: some View {
        // Split so the type checker sees three small chains, not one large one.
        navigationHandlers(sessionHandlers(layout))
            .alert("原交办核对", isPresented: Binding(
                get: { submissionCheck != nil },
                set: { if !$0 { submissionCheck = nil } }
            )) {
                Button("好", role: .cancel) {}
            } message: {
                Text(submissionCheck ?? "")
            }
    }

    private var showsAuxiliary: Bool {
        !usesFullWorkspace && (shellBudget?.inspectorWidth ?? 0) > 0
    }

    /// Overhaul M1: the centre is the opaque reading canvas; the auxiliary
    /// panel sits flush beside it behind a hairline drag strip, with a glass
    /// tab strip over opaque content. Its budgeted width includes the strip.
    /// It stays mounted while hidden so its list identities survive a toggle.
    private var layout: some View {
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                previewNavigation
                centralContent
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(AeroricColor.canvas)
            if let shellBudget {
                let gap = NativePanelGeometry.mainShell.handleWidth
                let panelWidth = max(0, (shellBudget.inspectorWidth > 0
                    ? shellBudget.inspectorWidth : NativePanelGeometry.mainShell.minimumInspectorWidth) - gap)
                InspectorResizeHandle(width: inspectorWidthPreference,
                                      effectiveWidth: shellBudget.inspectorWidth,
                                      totalWidth: shellBudget.totalWidth)
                    .frame(width: showsAuxiliary ? gap : 0)
                    .allowsHitTesting(showsAuxiliary)
                auxiliaryPanel
                    .frame(width: panelWidth)
                    .frame(maxHeight: .infinity)
                    .background(AeroricColor.layer1)
                    .frame(width: showsAuxiliary ? panelWidth : 0)
                .clipped()
                .opacity(showsAuxiliary ? 1 : 0)
                .allowsHitTesting(showsAuxiliary)
                .accessibilityHidden(!showsAuxiliary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func sessionHandlers<Content: View>(_ content: Content) -> some View {
        content
        .onChange(of: auxiliaryTab, initial: true) { _, _ in syncAuxiliarySection() }
        .onChange(of: resourceProjectID) { _, _ in syncAuxiliarySection() }
        .onChange(of: selectedTask?.status) { _, status in
            // Q25: a hook-reported wait expands the dock and gives the
            // terminal focus. It never answers or confirms anything.
            guard status == "input_required", taskSurfaceVisible else { return }
            terminalExpanded = true
            DispatchQueue.main.async {
                if let coordinator = services.terminalCoordinator, coordinator.holdsLease,
                   coordinator.taskID == selectedTaskID {
                    coordinator.renderer.window?.makeFirstResponder(coordinator.renderer)
                }
            }
        }
        .onChange(of: resourceProjectID, initial: true) { _, id in
            if let id { visitedResourceProjects.insert(id) }
        }
        .onChange(of: navigationEpoch) { _, _ in
            // An explicit task/draft navigation wins even when its sidebar
            // value was already selected. Preserve the reusable target.
            if SessionNavigation.taskID(selection) != nil || draftScope != nil,
               let id = resourceProjectID { resourceBook.showTask(projectID: id) }
        }
        .onChange(of: taskSurfaceVisible, initial: true) { _, visible in
            taskInputGate.update(taskID: selectedTaskID, visible: visible)
            if !visible || selectedTaskID.map({ !taskInputGate.permits($0) }) == true {
                services.suspendTerminalInput(presentationOwner: terminalPresentationOwner)
            }
            // Returning only restores reading. Never implicitly take control.
        }
        .onChange(of: services.configurationEpoch) { _, _ in resourceBook.invalidateSnapshots() }
        .task(id: resourceLoadKey) { await loadCurrentResources() }
    }

    private func navigationHandlers<Content: View>(_ content: Content) -> some View {
        content
        .onChange(of: selection, initial: true) { _, newValue in
            if let scope = SessionNavigation.draftScope(newValue) {
                draftBook.open(scope)
                if let projectID = scope.projectID {
                    projectContextID = projectID
                    resourceBook.showTask(projectID: projectID)
                }
            } else if let taskID = SessionNavigation.taskID(newValue) {
                selectedTaskID = taskID
                if let task = services.taskStore.tasks.first(where: { $0.id == taskID }) {
                    projectContextID = task.projectID
                    taskProjectContexts[taskID] = task.projectID
                    scopedProjectID = task.projectID
                    resourceBook.showTask(projectID: task.projectID)
                }
            } else if newValue == SessionNavigation.allTasks {
                scopedProjectID = nil
            } else if let project = services.taskStore.projects.first(where: { $0.id == newValue }) {
                projectContextID = project.id
                scopedProjectID = project.id
            }
        }
        .focusedSceneValue(\.newSessionAction, { openNewSession() })
        .onChange(of: selectedTask?.id, initial: true) { _, newValue in
            guard let newValue, let task = services.taskStore.tasks.first(where: { $0.id == newValue })
            else {
                taskInputGate.leave()
                services.detachTerminal(presentationOwner: terminalPresentationOwner)
                return
            }
            visitedTaskIDs.insert(task.id)
            taskInputGate.update(taskID: task.id, visible: taskSurfaceVisible)
            if services.liveStructured[task.id] == true {
                services.detachTerminal(presentationOwner: terminalPresentationOwner)
            } else if let generation = services.generation(for: task) {
                services.attachTerminal(taskId: task.id, generation: generation,
                                        presentationOwner: terminalPresentationOwner)
                if !taskInputGate.permits(task.id) {
                    services.suspendTerminalInput(presentationOwner: terminalPresentationOwner)
                }
            } else {
                services.detachTerminal(presentationOwner: terminalPresentationOwner)
            }
        }
        .onDisappear {
            taskInputGate.leave()
            services.suspendTerminalInput(presentationOwner: terminalPresentationOwner)
        }
        .onChange(of: selectedTaskID.flatMap { services.liveStructured[$0] }) { _, structured in
            // A native session has no PTY: never keep a terminal poll for it.
            if structured == true { services.detachTerminal(presentationOwner: terminalPresentationOwner) }
        }
        .onChange(of: services.requestedSelection) { _, requested in
            guard let requested else { return }
            selection = requested
            services.requestedSelection = nil
        }
        .onChange(of: services.requestedNewSessionLike) { _, taskID in
            guard let taskID else { return }
            services.requestedNewSessionLike = nil
            openNewSession(like: taskID)
        }
    }

    /// `/clear` (N4 D6): the new-session page prefilled like this task; the
    /// current session stays as it is.
    private func openNewSession(like taskID: String) {
        guard let task = services.taskStore.tasks.first(where: { $0.id == taskID }) else { return }
        let target = SessionNavigation.draft(projectID: task.projectID)
        guard let scope = SessionNavigation.draftScope(target) else { return }
        let configuration = services.sessionConfigurations(projectID: task.projectID).first { $0.agent == task.agent }
        let config = services.liveStores[taskID]?.model?.config
        draftBook.open(scope)
        draftBook.edit(scope) { draft in
            if let configuration {
                draft.selectHarness(configuration.harness)
                draft.apply(configuration)
            }
            if let model = config?.model { draft.task.model = model }
            if let permission = config?.permission { draft.selectPermission(permission) }
        }
        selection = target
    }

    // MARK: - Auxiliary panel (Q17: 详情 / 文件 / 改动 / Git 日志)

    private var auxiliaryPanel: some View {
        VStack(spacing: 0) {
            // v3: icon tabs across the top (codeg aux panel header).
            HStack(spacing: 0) {
                CapsuleSlider(selection: $auxiliaryTab, groupTitle: "辅助面板", mode: .iconsWithSelectedTitle,
                              identifier: "auxiliary.tab-slider", segments: [
                    .init(value: .details, title: "详情", systemImage: "info.circle"),
                    .init(value: .files, title: "文件", systemImage: "folder"),
                    .init(value: .changes, title: "改动", systemImage: "plusminus"),
                    .init(value: .history, title: "Git 日志", systemImage: "clock.arrow.circlepath"),
                ])
                .fixedSize()
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .frame(height: 40)
            .glassSurface(.tabStrip)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("辅助面板")
            .accessibilityIdentifier("auxiliary.tabs")
            HairlineDivider()
            ZStack {
                auxiliaryDetails
                    .opacity(auxiliaryTab == .details ? 1 : 0)
                    .allowsHitTesting(auxiliaryTab == .details)
                    .accessibilityHidden(auxiliaryTab != .details)
                resourceInspector
                    .opacity(auxiliaryTab == .files || auxiliaryTab == .changes ? 1 : 0)
                    .allowsHitTesting(auxiliaryTab == .files || auxiliaryTab == .changes)
                    .accessibilityHidden(auxiliaryTab != .files && auxiliaryTab != .changes)
                auxiliaryHistory
                    .opacity(auxiliaryTab == .history ? 1 : 0)
                    .allowsHitTesting(auxiliaryTab == .history)
                    .accessibilityHidden(auxiliaryTab != .history)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    /// Files and Changes share the existing per-project resource book; the
    /// tab only chooses which of its two lists is in front.
    private func syncAuxiliarySection() {
        guard let id = resourceProjectID else { return }
        switch auxiliaryTab {
        case .files: resourceBook.edit(id) { $0.section = .files }
        case .changes: resourceBook.edit(id) { $0.section = .git }
        case .details, .history: break
        }
    }

    private var auxiliaryDetails: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let task = selectedTask, SessionNavigation.taskID(selection) != nil {
                    detailRow("会话", task.title)
                    detailRow("状态", SessionStatusDot.label(task.status))
                    detailRow("Harness", task.agent ?? "—")
                    detailRow("项目", services.taskStore.projects.first { $0.id == task.projectID }?.name ?? "原项目不可用")
                    if let created = task.createdAtMs { detailRow("创建", Self.timestamp(created)) }
                    if let updated = task.updatedAtMs { detailRow("更新", Self.timestamp(updated)) }
                    detailRow("显示", forcedTerminalTaskIDs.contains(task.id)
                              || services.interactionModeController.mode == .advanced ? "终端" : "对话")
                } else if let scope = draftScope {
                    detailRow("草稿", scope.projectID == nil ? "全局（未选择项目）" : "项目草稿")
                    detailRow("项目", resourceProject?.name ?? "未选择")
                    if let draft = draftBook[scope] {
                        detailRow("Harness", draft.selectedHarness.isEmpty ? "未选择" : draft.selectedHarness)
                        detailRow("组合能力", draft.capabilities.enabledNames.isEmpty
                                  ? "未启用" : draft.capabilities.enabledNames.joined(separator: "、"))
                    }
                } else {
                    Text("选择一个会话或新建会话后，这里显示它的详情。")
                        .font(Font.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("auxiliary.details")
    }

    private func detailRow(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label)
                .font(Font.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            Text(value)
                .font(Font.system(size: AeroricType.body))
                .foregroundStyle(AeroricColor.text)
                .textSelection(.enabled)
                .lineLimit(3)
        }
    }

    private static func timestamp(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
            .formatted(date: .abbreviated, time: .shortened)
    }

    /// Read-only commit list from the existing `GitStore` snapshot. No new
    /// Git query is issued here; the state notice says when nothing is loaded.
    private var auxiliaryHistory: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let notice = services.gitStore.historyReadState.notice {
                Text(notice)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(AeroricSpace.l)
            }
            if let branches = resourceProjectID.map({ resourceBook.scene($0).branches }), !branches.isEmpty {
                DisclosureGroup {
                    ForEach(branches, id: \.name) { branch in
                        Label(branch.name, systemImage: branch.current ? "checkmark" : "arrow.triangle.branch")
                            .font(.system(size: AeroricType.secondary, weight: branch.current ? .semibold : .regular))
                            .foregroundStyle(branch.current ? AeroricColor.text : AeroricColor.textSecondary)
                            .lineLimit(1)
                    }
                } label: {
                    Text("分支 · \(branches.count)").font(AeroricType.sectionLabel()).foregroundStyle(AeroricColor.textTertiary)
                }
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s)
                .accessibilityIdentifier("auxiliary.history.branches")
                HairlineDivider()
            }
            List(services.gitStore.commits) { commit in
                VStack(alignment: .leading, spacing: 3) {
                    Text(commit.message.split(separator: "\n").first.map(String.init) ?? commit.message)
                        .font(.system(size: AeroricType.secondary, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(2)
                    HStack(spacing: AeroricSpace.s) {
                        Text(commit.shortHash)
                            .font(.system(size: AeroricType.caption, design: .monospaced))
                            .foregroundStyle(AeroricColor.accent)
                        Text("\(commit.author) · \(commit.date)")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                    }
                }
                .padding(.vertical, AeroricSpace.xs)
                .help(commit.hash)
                .listRowSeparator(.hidden)
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
        }
        .accessibilityIdentifier("auxiliary.history")
    }

    private var centralContent: some View {
        ZStack {
            // Keep the selected task's input/reading view alive while a
            // draft or independent tool is in front of it.
            taskContent
                .opacity(activeSection == .tasks && draftScope == nil && !previewActive ? 1 : 0)
                .allowsHitTesting(activeSection == .tasks && draftScope == nil && !previewActive)
                .disabled(!taskSurfaceVisible && SessionNavigation.taskID(selection) != nil)
                .accessibilityHidden(activeSection != .tasks || draftScope != nil || previewActive)
            if let scope = draftScope {
                composer(scope)
                    .opacity(previewActive ? 0 : 1)
                    .allowsHitTesting(!previewActive)
                    .disabled(previewActive)
                    .accessibilityHidden(previewActive)
            } else if activeSection == .attention {
                AttentionListView(
                    items: AttentionQueue.items(services.taskStore.tasks,
                                                nowMs: UInt64(Date().timeIntervalSince1970 * 1000)),
                    projectName: { id in services.taskStore.projects.first { $0.id == id }?.name },
                    onOpen: { selection = SessionNavigation.task($0) }
                )
                .opacity(previewActive ? 0 : 1)
                .allowsHitTesting(!previewActive)
                .accessibilityHidden(previewActive)
            } else if activeSection == .canvas {
                CanvasPage(
                    book: services.canvasBook, resolver: canvasResolver,
                    onOpenSession: { selection = $0 },
                    onOpenFile: { projectID, path in openPreview(.init(projectID: projectID, path: path, kind: .file)) }
                )
                .opacity(previewActive ? 0 : 1)
                .allowsHitTesting(!previewActive)
                .accessibilityHidden(previewActive)
            } else if activeSection != .tasks {
                ToolSectionScaffold(
                    title: activeSection.title, systemImage: activeSection.systemImage,
                    projectName: services.taskStore.projects.first(where: { $0.id == projectContextID })?.name,
                    status: activeSection.status,
                    returnTitle: selectedTask.map { task in
                        "返回：\(services.taskStore.projects.first { $0.id == task.projectID }?.name ?? "原项目") · \(task.title)"
                    },
                    onReturn: { if let task = selectedTask { selection = SessionNavigation.task(task.id) } }
                ) {
                    sectionContent
                }
                .opacity(previewActive ? 0 : 1)
                .allowsHitTesting(!previewActive)
                .accessibilityHidden(previewActive)
            }
            if let importedID = SessionNavigation.importedID(selection), !previewActive {
                ImportedSessionPane(book: services.importBook, id: importedID)
            }
            if previewActive, let id = resourceProjectID, let target = resourceBook.scene(id).target {
                CentralResourcePreview(
                    book: resourceBook, target: target,
                    projectName: resourceProject?.name ?? "原项目不可用",
                    projectAvailable: resourceProject != nil,
                    onRetry: { loadPreview(target) },
                    onOpenFile: { openPreview(.init(projectID: id, path: target.path, kind: .file)) }
                )
                .id(target)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// What canvas cards point at, looked up in this window's stores (M5).
    private var canvasResolver: CanvasResolver {
        CanvasResolver(
            task: { id in services.taskStore.tasks.first { $0.id == id } },
            imported: { id in services.importBook.session(id: id) },
            projects: { services.taskStore.projects },
            allTasks: { TaskBoard.sorted(services.taskStore.tasks.filter { !$0.archived }) },
            allImported: { services.importBook.sessions },
            readFile: { projectID, path in
                guard let reader = services.projectResourceReader else {
                    return .failure(AppServices.ProjectError.notConnected)
                }
                do { return .success(try await reader.text(projectID: projectID, path: path)) } catch { return .failure(error) }
            }
        )
    }

    private var draftScope: SessionDraftScope? { SessionNavigation.draftScope(selection) }
    private var surface: SurfacePreference { SurfacePreference(rawValue: surfaceRawValue) ?? .glass }
    private var resourceProjectID: String? {
        if let draftScope { return draftScope.projectID }
        if let id = SessionNavigation.taskID(selection) {
            return services.taskStore.tasks.first(where: { $0.id == id })?.projectID ?? taskProjectContexts[id]
        }
        if services.taskStore.projects.contains(where: { $0.id == selection }) { return selection }
        return projectContextID
    }
    private var resourceProject: TaskStore.ProjectEntry? {
        services.taskStore.projects.first { $0.id == resourceProjectID }
    }
    private var previewActive: Bool {
        !usesFullWorkspace && (resourceProjectID.map { resourceBook.scene($0).previewActive } ?? false)
    }
    private var usesFullWorkspace: Bool { activeSection != .tasks && draftScope == nil }
    private var taskSurfaceVisible: Bool {
        activeSection == .tasks && draftScope == nil && !previewActive
            && SessionNavigation.taskID(selection) == selectedTaskID && selectedTaskID != nil
    }
    private enum CentralPage: Hashable { case task, preview }
    // MSR01 keeps the two central segments visible even before a preview
    // target exists; the preview segment disables instead of disappearing.
    @ViewBuilder private var previewNavigation: some View {
        if !usesFullWorkspace {
            let id = resourceProjectID
            let target = id.flatMap { resourceBook.scene($0).target }
            NativeSegmentedControl(
                selection: Binding(
                    get: { previewActive ? CentralPage.preview : .task },
                    set: { page in
                        guard let id else { return }
                        if page == .task {
                            resourceBook.showTask(projectID: id)
                        } else if let target {
                            taskInputGate.leave()
                            services.suspendTerminalInput(presentationOwner: terminalPresentationOwner)
                            resourceBook.select(target)
                        }
                    }
                ),
                groupTitle: "中央工作面",
                segments: [
                    .init(value: .task, title: "当前任务", isEnabled: true),
                    .init(value: .preview, title: "预览", isEnabled: target != nil),
                ]
            )
            .padding(.horizontal, 12).padding(.vertical, 6)
            .accessibilityIdentifier("workspace.central-pages")
        }
    }
    private var resourceInspector: some View {
        ZStack {
            // Keep native List identities per window × project, including
            // their scroll/focus state, when another project is in front.
            ForEach(visitedResourceProjects.union(resourceProjectID.map { [$0] } ?? []).sorted(), id: \.self) { id in
                if let project = services.taskStore.projects.first(where: { $0.id == id }) {
                    ProjectResourceInspector(
                        book: resourceBook, projectID: id, projectName: project.name,
                        onOpen: openPreview,
                        onLoadDirectory: { path in
                            Task { await resourceBook.loadDirectory(
                                projectID: id, path: path, reader: services.projectResourceReader
                            ) }
                        },
                        onRefresh: { Task { await refreshResources(project) } },
                        onToggleChange: { change in
                            Task { await resourceBook.toggleInlineDiff(
                                change, projectID: id, projectPath: project.path,
                                reader: services.projectResourceReader
                            ) }
                        }
                    )
                    .opacity(resourceProjectID == id ? 1 : 0)
                    .allowsHitTesting(resourceProjectID == id)
                    .accessibilityHidden(resourceProjectID != id)
                }
            }
            if resourceProject == nil {
                PaneEmptyState(systemImage: "folder.badge.questionmark",
                               title: resourceProjectID == nil ? "未选择项目" : "原项目不可用",
                               hint: "资源读取需要明确的项目范围，不自动选择首项目。")
            }
        }
    }
    private struct ResourceLoadKey: Equatable {
        let projectID: String?
        let projectPath: String?
        let section: ResourceListSection?
        let visible: Bool
        let configurationEpoch: UUID
    }
    private var resourceLoadKey: ResourceLoadKey {
        ResourceLoadKey(
            projectID: resourceProjectID, projectPath: resourceProject?.path,
            section: resourceProjectID.map { resourceBook.scene($0).section },
            // Resizing an already-visible inspector must not cancel reads.
            visible: !usesFullWorkspace && (shellBudget?.inspectorWidth ?? 0) > 0,
            configurationEpoch: services.configurationEpoch
        )
    }
    private func loadCurrentResources() async {
        guard !usesFullWorkspace, let budget = shellBudget, budget.inspectorWidth > 0,
              let project = resourceProject else { return }
        let scene = resourceBook.scene(project.id)
        if scene.section == .files, (scene.directoryStates[""] ?? .notLoaded).needsAutomaticLoad {
            await resourceBook.loadDirectory(projectID: project.id, path: "", reader: services.projectResourceReader)
        } else if scene.section == .git, scene.gitState.needsAutomaticLoad || scene.branchState.needsAutomaticLoad {
            await resourceBook.loadGit(projectID: project.id, projectPath: project.path, reader: services.projectResourceReader)
        }
    }
    private func refreshResources(_ project: TaskStore.ProjectEntry) async {
        if resourceBook.scene(project.id).section == .files {
            // Refresh every already-expanded directory; never scan the tree.
            for path in resourceBook.scene(project.id).expandedPaths.sorted() {
                await resourceBook.loadDirectory(projectID: project.id, path: path, reader: services.projectResourceReader)
            }
        } else {
            await resourceBook.loadGit(projectID: project.id, projectPath: project.path, reader: services.projectResourceReader)
        }
    }
    private func openPreview(_ target: ResourcePreviewTarget) {
        // Synchronous local guard before hiding any task/IME surface.
        taskInputGate.leave()
        services.suspendTerminalInput(presentationOwner: terminalPresentationOwner)
        resourceBook.select(target)
        loadPreview(target)
    }
    private func loadPreview(_ target: ResourcePreviewTarget) {
        guard let project = services.taskStore.projects.first(where: { $0.id == target.projectID }) else { return }
        Task {
            await resourceBook.loadPreview(target, projectPath: project.path, reader: services.projectResourceReader)
        }
    }

    private func openNewSession() {
        selection = SessionNavigation.newDraftSelection(from: selection, items: services.sidebarItems)
    }

    private func composer(_ scope: SessionDraftScope) -> some View {
        let project = services.taskStore.projects.first { $0.id == scope.projectID }
        let value = draftBook[scope] ?? NewSessionDraft(submissionID: "unopened")
        let configurations = services.sessionConfigurations(projectID: scope.projectID)
        let reasons = sessionBlockingReasons(value, scope: scope)
        return NewSessionHome(
            draft: Binding(
                get: { draftBook[scope] ?? NewSessionDraft(submissionID: "unopened") },
                set: { value in draftBook.edit(scope) { $0 = value } }
            ),
            projectID: project?.id,
            projects: services.taskStore.projects,
            configurations: configurations,
            blockingReasons: reasons,
            onOpenProjectDraft: { selection = SessionNavigation.draft(projectID: $0) },
            onSubmit: { submit(scope) },
            onCheckSubmission: { checkSubmission(scope) }
        )
        .task(id: [scope.projectID ?? "", services.configurationEpoch.uuidString]) {
            if let projectID = scope.projectID { await services.loadSessionConfigurations(projectID: projectID) }
        }
    }

    private func sessionBlockingReasons(_ value: NewSessionDraft, scope: SessionDraftScope) -> [String] {
        var reasons = value.blockingReasons(
            scope: scope, configurations: services.sessionConfigurations(projectID: scope.projectID),
            projectExists: services.taskStore.projects.contains { $0.id == scope.projectID }
        )
        if let projectID = scope.projectID {
            switch services.configurationReadStates[projectID] ?? .notLoaded {
            case .loaded: break
            case .notLoaded: reasons.append("配置目录尚未读取，当前不能发送。")
            case .loading: reasons.append("正在读取配置目录，当前不能发送。")
            case .unavailable: reasons.append("宿主配置目录不可用，当前不能发送。")
            case let .failed(reason): reasons.append("配置目录读取失败，保留的选择仅供参考：\(reason)")
            }
        }
        return reasons
    }

    private func submit(_ scope: SessionDraftScope) {
        guard let projectID = scope.projectID,
              let value = draftBook[scope],
              sessionBlockingReasons(value, scope: scope).isEmpty,
              let submitted = draftBook.begin(scope) else { return }
        Task { @MainActor in
            do {
                let receipt = try await services.submitSession(submitted, projectID: projectID)
                if receipt.state == "accepted", let returnedID = receipt.taskId {
                    if draftBook.acknowledge(scope, submissionID: submitted.submissionID),
                       SessionNavigation.draftScope(selection) == scope {
                        selection = SessionNavigation.task(returnedID)
                    }
                } else {
                    draftBook.fail(scope, submissionID: submitted.submissionID,
                                   reason: receipt.reasons.joined(separator: "\n"),
                                   acceptanceUnknown: receipt.state != "rejected_before_side_effect")
                }
            } catch {
                // Only a verified pre-execution receipt proves refusal.
                // RPC errors, malformed replies and disconnects remain unknown.
                draftBook.fail(scope, submissionID: submitted.submissionID,
                               reason: String(describing: error),
                               acceptanceUnknown: !(error is AppServices.ProjectError))
            }
        }
    }

    private func checkSubmission(_ scope: SessionDraftScope) {
        guard let value = draftBook[scope], let projectID = scope.projectID else { return }
        Task { @MainActor in
            do {
                if let receipt = try await services.inspectSubmission(value, projectID: projectID) {
                    if receipt.state == "accepted", let taskID = receipt.taskId {
                        let cleared = draftBook.reconcileAccepted(scope, submissionID: value.submissionID)
                        if cleared && SessionNavigation.draftScope(selection) == scope {
                            selection = SessionNavigation.task(taskID)
                        }
                        submissionCheck = "已核对原交办的配置、指纹和逐项消费回执，任务：\(taskID)。"
                    } else if receipt.state == "rejected_before_side_effect" {
                        let reason = receipt.reasons.joined(separator: "\n")
                        draftBook.reconcileRejected(scope, submissionID: value.submissionID, reason: reason)
                        submissionCheck = "宿主明确拒绝且未执行。草稿已保留，修改后可重新发送：\(reason)"
                    } else {
                        submissionCheck = "原交办结果仍为待定或未知；保留草稿及身份，不重发。"
                    }
                } else {
                    submissionCheck = "本次查询没有找到原任务；这不能证明原交办未接收，仍不重发。"
                }
            } catch {
                submissionCheck = "核对失败，原草稿及提交身份保留：\(error)"
            }
        }
    }

    private var taskContent: some View {
        ZStack {
            VStack(spacing: 0) {
                if let task = selectedTask {
                    TaskDetailHeader(
                        task: task,
                        onCancel: { Task { await services.cancelTask(taskId: task.id) } },
                        onDelete: { services.taskStore.removeTask(id: task.id) },
                        onStar: { services.toggleStar(taskId: task.id) },
                        onArchive: { services.toggleArchived(taskId: task.id) }
                    )
                }
                ZStack {
                    ForEach(visitedTaskIDs.sorted(), id: \.self) { taskID in
                        if let task = services.taskStore.tasks.first(where: { $0.id == taskID }) {
                            taskBody(task)
                                .opacity(selectedTaskID == taskID && taskSurfaceVisible ? 1 : 0)
                                .allowsHitTesting(selectedTaskID == taskID && taskSurfaceVisible)
                                .disabled(selectedTaskID != taskID || !taskSurfaceVisible)
                                .accessibilityHidden(selectedTaskID != taskID || !taskSurfaceVisible)
                        }
                    }
                    if selectedTask == nil {
                        PaneEmptyState(
                            systemImage: "sidebar.left", title: selectedTaskID == nil ? "未选择任务" : "原任务不可用",
                            hint: "在项目树或全部任务中选择明确目标，或新建会话"
                        )
                        .accessibilityIdentifier("tasks.empty-detail")
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .opacity(SessionNavigation.taskID(selection) != nil ? 1 : 0)
            .allowsHitTesting(SessionNavigation.taskID(selection) != nil)
            .accessibilityHidden(SessionNavigation.taskID(selection) == nil)
            if SessionNavigation.taskID(selection) == nil {
                TaskBoardView(
                    tasks: visibleTasks,
                    filterText: $taskFilterText,
                    selectedTaskID: Binding(
                        get: { selectedTaskID },
                        set: { id in
                            selectedTaskID = id
                            if let id { selection = SessionNavigation.task(id) }
                        }
                    ),
                    projectNames: Dictionary(services.taskStore.projects.map { ($0.id, $0.name) },
                                             uniquingKeysWith: { first, _ in first }),
                    onOpenDraft: { openNewSession() },
                    onCancel: { id in Task { await services.cancelTask(taskId: id) } },
                    onStar: { services.toggleStar(taskId: $0) },
                    onArchive: { services.toggleArchived(taskId: $0) }
                )
            }
        }
        .accessibilityIdentifier("tasks.workspace")
    }

    /// The project the Git page reads: the window's project context.
    private var gitProjectPath: String? {
        services.taskStore.projects.first { $0.id == projectContextID }?.path
    }

    @ViewBuilder
    private var sectionContent: some View {
        switch activeSection {
        case .tasks, .attention, .canvas:
            EmptyView() // Mounted directly by centralContent.
        case .files:
            if let projectID = projectContextID, gitProjectPath != nil {
                // Read-only for now: browse and preview; no create/rename/delete handler.
                FileBrowserView(
                    store: services.fileTreeStore,
                    onExpand: { path in Task { await services.loadFiles(projectID: projectID, path: path) } },
                    onOpen: { path in openPreview(.init(projectID: projectID, path: path, kind: .file)) }
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .task(id: projectID) {
                    services.fileTreeStore.reset(projectID: projectID)
                    await services.loadFiles(projectID: projectID, path: "")
                }
            } else {
                PaneEmptyState(systemImage: "folder", title: "先选一个项目", hint: "文件页浏览侧栏当前项目的目录。")
            }
        case .git:
            if let path = gitProjectPath {
                GitPanel(store: services.gitStore, onSelect: { change in
                    Task { await services.loadGitDiff(projectPath: path, change: change) }
                })
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .task(id: path) { await services.refreshGit(projectPath: path) }
            } else {
                PaneEmptyState(systemImage: "folder", title: "先选一个项目", hint: "Git 页显示侧栏当前项目的仓库。")
            }
        case .ide:
            HSplitView {
                ProblemsView(store: services.diagnosticsStore)
                    .frame(minWidth: 260, maxWidth: 400, maxHeight: .infinity)
                DebugPanel(
                    sessionStore: services.debugSessionStore,
                    breakpointStore: services.breakpointStore,
                    debugId: "default"
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .connections:
            HSplitView {
                VStack(spacing: 8) {
                    ConnectionList(store: services.sshConnectionStore)
                    SSHConnectionEditor { name, host, port, username, _ in
                        var current = services.sshConnectionStore.connections
                        current.append(
                            AeroricUI.ConnectionStore.Connection(
                                id: UUID().uuidString,
                                name: name,
                                host: host,
                                port: port,
                                username: username
                            )
                        )
                        services.sshConnectionStore.ingest(current)
                    }
                }
                .frame(minWidth: 260, maxWidth: 380, maxHeight: .infinity)
                SFTPBrowser(store: services.sshConnectionStore, entries: [])
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .infrastructure:
            HSplitView {
                DockerPanel(store: services.infrastructureStore)
                    .frame(minWidth: 260, maxWidth: 400, maxHeight: .infinity)
                VStack(spacing: 8) {
                    PortInspector(store: services.infrastructureStore)
                    CondaEnvironmentPicker(store: services.infrastructureStore)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .database:
            HSplitView {
                DatabaseSidebar(store: services.databaseStore)
                    .frame(minWidth: 220, maxWidth: 340, maxHeight: .infinity)
                VStack(spacing: 0) {
                    SQLQueryEditor(sql: $sqlDraft)
                    ResultGrid(store: services.databaseStore)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        case .notebook:
            HSplitView {
                NotebookSidebar(store: services.notebookStore)
                    .frame(minWidth: 220, maxWidth: 340, maxHeight: .infinity)
                VStack(spacing: 8) {
                    SyncStatusPanel(store: services.syncStore)
                    RAGSearchView()
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .extensions:
            HSplitView {
                SkillHubView(store: services.extensionStore)
                    .frame(minWidth: 240, maxWidth: 360, maxHeight: .infinity)
                VStack(spacing: 8) {
                    MCPSettingsView(store: services.extensionStore)
                    HookStatusView(store: services.extensionStore)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .remote:
            RemoteAccessSettingsView(store: services.remoteStore)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .operations:
            HSplitView {
                UsageDashboard(store: services.maintenanceStore)
                    .frame(minWidth: 320, maxWidth: 480, maxHeight: .infinity)
                VStack(spacing: 8) {
                    DiagnosticsView(store: services.maintenanceStore)
                    NotificationCenterView(store: services.maintenanceStore)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .agentTools:
            // AP7:外部 Agent 的能力面。八个区块全是**展示**:执行者是外部 Agent,
            // 原生端不代跑工具、不代申请系统权限。
            AgentToolsPanel(
                store: services.externalProtocolStore,
                controls: services.agentToolsStore
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var selectedTask: TaskStore.TaskEntry? {
        selectedTaskID.flatMap { id in
            services.taskStore.tasks.first(where: { $0.id == id })
        }
    }

    private var visibleTasks: [TaskStore.TaskEntry] {
        let scoped = services.taskStore.tasks.filter { scopedProjectID == nil || $0.projectID == scopedProjectID }
        let query = taskFilterText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return scoped }
        return scoped.filter {
            $0.title.localizedCaseInsensitiveContains(query)
                || $0.status.localizedCaseInsensitiveContains(query)
        }
    }

    /// P5-02 SessionPresentation 接线落点：由模式＋能力验证状态决定
    /// 呈现；`forcedTerminal` 只影响本窗口本次选择（用户触发，不自动改
    /// 全局偏好）。
    /// N4: a task the host runs as a structured session gets the live page —
    /// the whole column is the conversation, no terminal (Q25). Anything else
    /// keeps the presentation below.
    @ViewBuilder
    private func taskBody(_ task: TaskStore.TaskEntry) -> some View {
        if services.liveCapability {
            switch services.liveStructured[task.id] {
            case true?:
                LiveSessionView(store: services.liveStore(for: task.id), facts: services.liveFacts(for: task),
                                focusApproval: services.approvalFocus[task.id])
                    .id("live.\(task.id)")
            case false?:
                legacyTaskBody(task)
            case nil:
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .task(id: task.id) { await services.resolveLive(taskID: task.id) }
            }
        } else {
            legacyTaskBody(task)
        }
    }

    @ViewBuilder
    private func legacyTaskBody(_ task: TaskStore.TaskEntry) -> some View {
        let presentation = SessionPresentation.resolve(
            // N6 D5: the global mode is fixed native; only this window's explicit
            // "switch to terminal" for a task still picks the terminal.
            mode: forcedTerminalTaskIDs.contains(task.id) ? .advanced : .native,
            structuredCapabilitiesVerified: services.structuredCapabilitiesVerified(task)
        )
        switch presentation {
        case .native:
            // Q15-b: conversation on top, the Agent's own PTY docked below.
            ConversationTerminalSplit(
                terminalExpanded: $terminalExpanded,
                terminalFraction: $terminalFraction,
                terminalTitle: "终端 · \(task.agent ?? "Agent")"
            ) {
                SessionRouteView(task: task, actions: HostSessionRouteActions(services: services),
                                 canSend: taskInputGate.permits(task.id))
            } terminal: {
                terminalSurface(task)
            }
        case .terminal:
            // 终端档（interactionMode = advanced）: terminal at full height.
            terminalSurface(task)
        case .requiresAdvanced:
            // Q26-a: no structured history → terminal full height, labelled.
            VStack(spacing: 0) {
                Label("此会话仅终端可读：宿主未提供结构化历史", systemImage: "terminal")
                    .font(Font.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 6)
                    .background(AeroricColor.layer1)
                    .overlay(alignment: .bottom) { Rectangle().fill(AeroricColor.separator2).frame(height: 1) }
                    .accessibilityIdentifier("tasks.requires-advanced")
                terminalSurface(task)
            }
        }
    }

    /// The real terminal renderer for `task`, or a pending note. Bytes are
    /// the Agent's PTY output only; this view never synthesises a TUI.
    @ViewBuilder
    private func terminalSurface(_ task: TaskStore.TaskEntry) -> some View {
        if services.terminalPresentationOwner == terminalPresentationOwner,
           services.terminalCoordinator?.taskID == task.id, let coordinator = services.terminalCoordinator {
            VStack(alignment: .leading, spacing: 0) {
                if !coordinator.holdsLease {
                    Text("只读返回：本地输入已释放；不会自动接管。宿主权威输入票据仍待接入。")
                        .font(Font.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary).padding(8)
                }
                NativeTerminalView(coordinator: coordinator)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("tasks.terminal")
        } else {
            Text("终端未挂载（等待会话句柄）")
                .font(Font.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("tasks.terminal-pending")
        }
    }
}

/// P5-04 会话面的宿主接线（M2）：历史走 session.history（v1 SessionPage，
/// taskId 寻址），发送走 task.input，审批走 approval.respond，停止走
/// task.cancel（D3）。视图只拿这个协议，不持有连接（INV-02）。
@MainActor
struct HostSessionRouteActions: SessionRouteActions {
    let services: AppServices

    func loadHistory(taskID: String) async throws -> [AeroricUI.SessionMessage] {
        guard let sessions = services.sessionsClient else { throw AppServices.ProjectError.notConnected }
        let page = try await sessions.historyPage(taskId: taskID, cursor: nil)
        return page.messages.map(SessionBlockMapping.message)
    }

    func send(taskID: String, text: String) async throws {
        try await services.sendDshPrompt(sessionTaskId: taskID, text: text)
    }

    func stop(taskID: String) async {
        await services.cancelTask(taskId: taskID)
    }

    func respond(eventID: String, outcome: PendingRequestStore.Outcome) async throws {
        let payload = Self.jsonObject(PendingRequestStore.outcomePayload(outcome))
        try await services.respondDshOutcome(eventId: eventID, outcome: payload)
    }

    /// session.export；输出路径由宿主校验。
    func export(taskID: String) async throws {
        guard let out = services.chooseSessionExportPath(taskID: taskID) else { return }
        try await services.exportDshSession(taskId: taskID, out: out)
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: out)])
    }

    /// JSONSerialization 形状 → JSONValue。Bool 先于 NSNumber 判断（Bool 桥接成
    /// NSNumber），数组逐项转换（问题回答是数组）。
    static func jsonObject(_ object: [String: Any]) -> [String: JSONValue] {
        object.mapValues(jsonValue)
    }

    static func jsonValue(_ value: Any) -> JSONValue {
        switch value {
        case let text as String: .string(text)
        case let number as NSNumber where CFGetTypeID(number) == CFBooleanGetTypeID(): .bool(number.boolValue)
        case let number as NSNumber: .number(number.stringValue)
        case let dict as [String: Any]: .object(jsonObject(dict))
        case let array as [Any]: .array(array.map(jsonValue))
        default: .null
        }
    }
}
