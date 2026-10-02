import AeroricClient
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
            NativeTaskSummaryView(task: task)
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

/// native 呈现（P5-04 前的明确未接通状态）：只显示任务元数据，不伪造
/// composer/approval——与 `SessionPresentation.native` 注释一致。
private struct NativeTaskSummaryView: View {
    let task: TaskStore.TaskEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            LabeledContent("状态") { Text(task.status) }
            LabeledContent("Agent") { Text(task.agent ?? "—") }
            if let created = task.createdAtMs {
                LabeledContent("创建") { Text(Self.timeText(created)) }
            }
            if let updated = task.updatedAtMs {
                LabeledContent("更新") { Text(Self.timeText(updated)) }
            }
            Text("此会话的原生消息面板尚未接通（P5-04）；交互请切换到高级模式。")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
            Spacer()
        }
        .padding(16)
        .accessibilityIdentifier("tasks.native-summary")
    }

    static func timeText(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000).formatted(date: .abbreviated, time: .shortened)
    }
}
