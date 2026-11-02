import SwiftUI

/// P5-03 Action 4：任务详情头与操作区。**删除/取消/丢弃三分**：
/// * 取消（cancel）：只对运行中的任务；不删除记录。
/// * 删除（delete）：移除记录；**运行中的删除必须确认**（二段确认）。
/// * 丢弃（discard draft）：只丢弃未提交的草稿，无宿主副作用。
///
/// 2026-11-02 (owner): no row of its own any more. The state, title and
/// `已完成 · codex` go to the window toolbar beside the sidebar toggle
/// (`WindowTitle`); 停止, 置顶 and 更多 to the toolbar's trailing edge.
/// Destructive actions keep the two-step confirmation for running tasks.
public struct TaskDetailHeader: View {
    public let task: TaskStore.TaskEntry
    public var onCancel: (() -> Void)?
    public var onDelete: (() -> Void)?
    public var onStar: (() -> Void)?
    public var onArchive: (() -> Void)?

    @State private var confirmingDelete = false

    public init(
        task: TaskStore.TaskEntry,
        onCancel: (() -> Void)? = nil,
        onDelete: (() -> Void)? = nil,
        onStar: (() -> Void)? = nil,
        onArchive: (() -> Void)? = nil
    ) {
        self.task = task
        self.onCancel = onCancel
        self.onDelete = onDelete
        self.onStar = onStar
        self.onArchive = onArchive
    }

    static func isRunning(_ status: String) -> Bool {
        status == "running" || status == "input_required"
    }

    static func windowTitle(_ task: TaskStore.TaskEntry) -> WindowTitle {
        WindowTitle(task.title, subtitle: "\(SessionStatusDot.label(task.status)) · \(task.agent ?? "—")",
                    status: task.status)
    }

    public var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .windowTitle(Self.windowTitle(task))
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    if Self.isRunning(task.status), let onCancel {
                        Button("停止", action: onCancel)
                            .buttonStyle(.borderless)
                            .font(Font.system(size: AeroricType.caption, weight: .medium))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .accessibilityIdentifier("task.detail.cancel")
                    }
                    if let onStar {
                        NativeIconButton(systemImage: task.starred ? "pin.fill" : "pin",
                                         label: task.starred ? "取消置顶" : "置顶",
                                         identifier: "task.detail.star", action: onStar)
                            .foregroundStyle(task.starred ? AeroricColor.accent : AeroricColor.textSecondary)
                    }
                    moreMenu
                }
            }
            .confirmationDialog(
                Self.isRunning(task.status) ? "任务仍在运行，确定删除？" : "删除此会话记录？",
                isPresented: $confirmingDelete
            ) {
                Button("确认删除", role: .destructive) { onDelete?() }
                    .accessibilityIdentifier("task.detail.delete-confirm-button")
                Button("放弃", role: .cancel) {}
            } message: {
                Text("只移除本地记录；不会替你停止或清理宿主上的其他资源。")
            }
            .accessibilityIdentifier("task.detail")
    }

    private var moreMenu: some View {
        Menu {
            if let onArchive {
                Button(task.archived ? "取消归档" : "归档", action: onArchive)
                    .accessibilityIdentifier("task.detail.archive")
            }
            if onDelete != nil {
                Divider()
                Button("删除…", role: .destructive) { confirmingDelete = true }
                    .accessibilityIdentifier("task.detail.delete")
            }
        } label: {
            Image(systemName: "ellipsis")
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: NativePanelGeometry.iconButtonSize, height: NativePanelGeometry.iconButtonSize)
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("更多操作")
        .accessibilityLabel("更多操作")
    }
}

/// 丢弃草稿（Action 4 的第三分）：纯本地动作，无宿主副作用。
public struct TaskDiscardDraftButton: View {
    public var onDiscard: () -> Void

    public init(onDiscard: @escaping () -> Void) {
        self.onDiscard = onDiscard
    }

    public var body: some View {
        Button("丢弃草稿") { onDiscard() }
            .accessibilityIdentifier("draft.discard")
    }
}
