import SwiftUI

/// P5-03 Action 4：任务详情头与操作区。**删除/取消/丢弃三分**：
/// * 取消（cancel）：只对运行中的任务；不删除记录。
/// * 删除（delete）：移除记录；**运行中的删除必须确认**（二段确认）。
/// * 丢弃（discard draft）：只丢弃未提交的草稿，无宿主副作用。
///
/// Redesign 01: one 44pt row (codeg chat header) — status dot, title, agent
/// caption, then bare icon actions. Destructive actions stay in a menu with
/// the same two-step confirmation for running tasks.
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

    public var body: some View {
        HStack(spacing: 10) {
            SessionStatusDot(status: task.status)
            Text(task.title)
                .font(AeroricFont.ui(AeroricFont.body, weight: .semibold))
                .lineLimit(1)
                .truncationMode(.tail)
                .accessibilityIdentifier("task.detail.title")
            Text("\(SessionStatusDot.label(task.status)) · \(task.agent ?? "—")")
                .font(AeroricFont.ui(AeroricFont.meta))
                .foregroundStyle(SessionPalette.muted)
                .lineLimit(1)
            Spacer(minLength: 8)
            if Self.isRunning(task.status), let onCancel {
                Button("停止", action: onCancel)
                    .buttonStyle(.borderless)
                    .font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                    .foregroundStyle(SessionPalette.secondary)
                    .accessibilityIdentifier("task.detail.cancel")
            }
            if let onStar {
                NativeIconButton(systemImage: task.starred ? "pin.fill" : "pin",
                                 label: task.starred ? "取消置顶" : "置顶",
                                 identifier: "task.detail.star", action: onStar)
                    .foregroundStyle(task.starred ? SessionPalette.accent : SessionPalette.secondary)
            }
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
                    .foregroundStyle(SessionPalette.secondary)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("更多操作")
            .accessibilityLabel("更多操作")
        }
        .padding(.horizontal, 16)
        .frame(height: 44)
        .overlay(alignment: .bottom) {
            Rectangle().fill(SessionPalette.edge).frame(height: 1)
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
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task.detail")
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
