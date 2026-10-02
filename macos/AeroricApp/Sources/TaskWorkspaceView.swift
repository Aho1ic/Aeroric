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
struct TaskWorkspaceView: View {
    let services: AppServices
    /// 每窗口选中的任务 id（窗口局部状态，不进 store）。
    @State private var selectedTaskID: String?
    @State private var draftPresented = false
    @State private var draft = TaskWorkspaceView.emptyDraft
    @State private var draftError: String?
    /// native 模式下能力未验证会话的用户主动切换（不自动改全局偏好，
    /// P5-02 语义）。
    @State private var forcedTerminal = false

    static var emptyDraft: TaskDraft {
        TaskDraft(
            agent: "",
            model: nil,
            effort: nil,
            permissionMode: "",
            prompt: "",
            attachmentPaths: [],
            worktree: nil
        )
    }

    var body: some View {
        HSplitView {
            TaskListView(
                visibleTasks: Binding(
                    get: { services.taskStore.visibleTasks },
                    set: { _ in }
                ),
                filterText: Binding(
                    get: { services.taskStore.filterText },
                    set: { services.taskStore.filterText = $0 }
                ),
                onOpenDraft: { draftPresented = true }
            )
            .frame(minWidth: 240, maxWidth: 400, maxHeight: .infinity)

            VStack(spacing: 0) {
                if let task = selectedTask {
                    TaskDetailHeader(
                        task: task,
                        onCancel: { Task { await services.cancelTask(taskId: task.id) } },
                        onDelete: { services.taskStore.removeTask(id: task.id) },
                        onStar: { services.toggleStar(taskId: task.id) },
                        onArchive: { services.toggleArchived(taskId: task.id) }
                    )
                    taskBody(task)
                } else {
                    Text("选择一个任务查看详情")
                        .foregroundStyle(Color.dsTextMuted)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .accessibilityIdentifier("tasks.empty-detail")
                }
            }
            .frame(minWidth: 360, maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier("tasks.workspace")
        // 选中运行中任务 → 挂 P5-01 终端轮询；取消选中/切任务 → 释放。
        .onChange(of: selectedTaskID) { _, newValue in
            forcedTerminal = false
            guard let newValue, let task = services.taskStore.tasks.first(where: { $0.id == newValue })
            else {
                services.detachTerminal()
                return
            }
            // generation 缺失（快照还没带会话句柄）不挂载；下轮刷新补上。
            if let generation = services.generation(for: task) {
                services.attachTerminal(taskId: task.id, generation: generation)
            } else {
                services.detachTerminal()
            }
        }
        .sheet(isPresented: $draftPresented) {
            TaskDraftView(
                options: services.draftOptions,
                draft: $draft,
                onSubmit: { submitted in
                    draftError = nil
                    guard let projectID = services.taskStore.selectedProjectID
                        ?? services.taskStore.projects.first?.id
                    else {
                        draftError = "未选择项目"
                        return
                    }
                    Task { @MainActor in
                        do {
                            _ = try await services.runTask(submitted, projectID: projectID)
                            draftPresented = false
                            self.draft = Self.emptyDraft
                        } catch {
                            draftError = String(describing: error)
                        }
                    }
                },
                onCancel: { draftPresented = false }
            )
            .padding(20)
            .frame(minWidth: 420, minHeight: 380)
            .accessibilityIdentifier("tasks.draft-sheet")
        }
        .alert("提交失败", isPresented: Binding(
            get: { draftError != nil },
            set: { if !$0 { draftError = nil } }
        )) {
            Button("好", role: .cancel) {}
        } message: {
            Text(draftError ?? "")
        }
    }

    private var selectedTask: TaskStore.TaskEntry? {
        selectedTaskID.flatMap { id in
            services.taskStore.tasks.first(where: { $0.id == id })
        }
    }

    /// P5-02 SessionPresentation 接线落点：由模式＋能力验证状态决定
    /// 呈现；`forcedTerminal` 只影响本窗口本次选择（用户触发，不自动改
    /// 全局偏好）。
    @ViewBuilder
    private func taskBody(_ task: TaskStore.TaskEntry) -> some View {
        let presentation = SessionPresentation.resolve(
            mode: forcedTerminal ? .advanced : services.interactionModeController.mode,
            structuredCapabilitiesVerified: services.structuredCapabilitiesVerified(task)
        )
        switch presentation {
        case .native:
            // P5-04 兑现：DSH 会话的 native 呈现已接通（消息面板＋composer＋
            // 审批/问题）。会话读取/发送全部走 SessionsClient 类型化调用。
            DshSessionPanel(services: services, task: task)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .terminal:
            if services.terminalCoordinator?.taskID == task.id, let coordinator = services.terminalCoordinator {
                NativeTerminalView(coordinator: coordinator)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("tasks.terminal")
            } else {
                Text("终端未挂载（等待会话句柄）")
                    .foregroundStyle(Color.dsTextMuted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("tasks.terminal-pending")
            }
        case let .requiresAdvanced(reason):
            VStack(spacing: 12) {
                Text(reason)
                    .foregroundStyle(Color.dsTextSecondary)
                Button("切换到高级模式") { forcedTerminal = true }
                    .accessibilityIdentifier("tasks.switch-advanced")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("tasks.requires-advanced")
        }
    }
}

/// P5-04 Action 6：DSH 会话 native 面板（消息＋composer＋审批/问题）。
/// 会话读取走 SessionsClient（task.history taskId 寻址，G13 不全量放行）；
/// 发送走 composer（斜杠命令早跳）；审批/问题走 PendingRequestStore＋
/// respond_dsh_remote_event。非 DSH 会话保持 unsupported 状态（Action 6：
/// 移除对应未接通状态但保留其他 Agent 的 unsupported 状态——这里只由
/// SessionPresentation.native 分支到达，非 DSH 不会走到这里）。
struct DshSessionPanel: View {
    let services: AppServices
    let task: TaskStore.TaskEntry

    @State private var paginator = SessionPaginator()
    @State private var pendingStore = PendingRequestStore()
    @State private var composerDraft = ""
    @State private var composerAttachments: [String] = []
    @State private var usage = SessionUsage()
    @State private var approvalPresented = false
    @State private var questionPresented = false
    @State private var sendError: String?
    @State private var pageLoadTask: Task<Void, Never>?

    var body: some View {
        VStack(spacing: 0) {
            SessionView(
                paginator: paginator,
                onLoadMore: { loadEarlierPage() },
                onExport: { exportSession() },
                usage: usage
            )
            ComposerView(
                catalog: nil,
                draft: $composerDraft,
                attachmentPaths: $composerAttachments,
                onSend: { text, attachments in
                    sendMessage(text: text, attachments: attachments)
                }
            )
        }
        .onAppear {
            paginator.reset(sessionKey: task.id, isCodex: false)
            loadEarlierPage()
        }
        .onChange(of: task.id) { oldID, newID in
            guard oldID != newID else { return }
            pageLoadTask?.cancel()
            paginator.reset(sessionKey: newID, isCodex: false)
            loadEarlierPage()
        }
        .sheet(isPresented: $approvalPresented) {
            if let request = pendingStore.pending.first(where: { $0.kind == .approval }) {
                ApprovalSheet(
                    request: request,
                    store: pendingStore,
                    onSubmit: { eventId, outcome in submitOutcome(eventId: eventId, outcome: outcome) },
                    onDismiss: { approvalPresented = false }
                )
            }
        }
        .sheet(isPresented: $questionPresented) {
            if let request = pendingStore.pending.first(where: { $0.kind == .question }) {
                QuestionSheet(
                    request: request,
                    store: pendingStore,
                    onSubmit: { eventId, outcome in submitOutcome(eventId: eventId, outcome: outcome) },
                    onDismiss: { questionPresented = false }
                )
            }
        }
        .alert("发送失败", isPresented: Binding(
            get: { sendError != nil },
            set: { if !$0 { sendError = nil } }
        )) {
            Button("好", role: .cancel) {}
        } message: {
            Text(sendError ?? "")
        }
    }

    /// 加载更早页（分页循环：cursor=null 起步 → session.history → 合并；
    /// !hasMore 或坏游标停止）。
    private func loadEarlierPage() {
        guard paginator.canLoadMore || paginator.messages.isEmpty else { return }
        paginator.setLoadingPage(true)
        pageLoadTask?.cancel()
        pageLoadTask = Task { @MainActor in
            guard let sessions = services.sessionsClient else {
                paginator.setLoadingPage(false)
                return
            }
            if let page = try? await sessions.historyPage(
                taskId: task.id,
                cursor: paginator.cursor.map { String($0) }
            ) {
                paginator.ingest(page: SessionMessagePage(
                    messages: page.messages.map(Self.toStoreMessage),
                    nextCursor: nil,
                    hasMore: false
                ))
            }
            paginator.setLoadingPage(false)
        }
    }

    /// 发送（composer → DSH prompt；斜杠命令已在 ComposerView 早跳）。
    private func sendMessage(text: String, attachments: [String]) {
        Task { @MainActor in
            do {
                try await services.sendDshPrompt(sessionTaskId: task.id, text: text)
            } catch {
                sendError = String(describing: error)
            }
        }
    }

    /// 提交审批/问题回执（approval.respond；值即结果本身，无信封）。
    private func submitOutcome(eventId: String, outcome: PendingRequestStore.Outcome) {
        pendingStore.markSubmitted(eventId: eventId)
        let payload = Self.toJSONValue(PendingRequestStore.outcomePayload(outcome))
        Task { @MainActor in
            try? await services.respondDshOutcome(eventId: eventId, outcome: payload)
            pendingStore.resolve(eventId: eventId)
        }
    }

    /// [String: Any]（JSONSerialization 产物）→ JSONValue。
    static func toJSONValue(_ any: [String: Any]) -> [String: JSONValue] {
        any.mapValues { value in
            switch value {
            case let text as String: .string(text)
            case let number as NSNumber: .number(number.stringValue)
            case let flag as Bool: .bool(flag)
            case let dict as [String: Any]: .object(toJSONValue(dict))
            default: .null
            }
        }
    }

    /// 导出 Markdown（session.export；out 路径由宿主校验）。
    private func exportSession() {
        Task { @MainActor in
            let out = NSTemporaryDirectory() + "session-\(task.id).md"
            try? await services.exportDshSession(taskId: task.id, out: out)
        }
    }

    /// 协议层消息 → store 镜像（v1 ContentBlock → UI SessionContent）。
    static func toStoreMessage(_ message: AeroricProtocol.SessionMessage) -> AeroricUI.SessionMessage {
        var content: [SessionContent] = []
        for block in message.blocks {
            switch block {
            case let .text(_, text):
                content.append(.text(text: text))
            case let .unknown(opaque):
                content.append(.text(text: opaque.kind))
            }
        }
        return SessionMessage(role: message.role.rawValue, content: content, messageId: message.id)
    }
}
