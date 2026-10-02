import SwiftUI

/// P5-03 Action 4：任务详情头与操作区。**删除/取消/丢弃三分**：
/// * 取消（cancel）：只对运行中的任务；不删除记录。
/// * 删除（delete）：移除记录；**运行中的删除必须确认**（二段确认）。
/// * 丢弃（discard draft）：只丢弃未提交的草稿，无宿主副作用。
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
            VStack(alignment: .leading, spacing: 2) {
                Text(task.title)
                    .font(.headline)
                    .accessibilityIdentifier("task.detail.title")
                Text("\(task.status) · \(task.agent ?? "—")")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
            }
            Spacer()
            if let onStar {
                Button(task.starred ? "取消星标" : "星标") { onStar() }
                    .accessibilityIdentifier("task.detail.star")
            }
            if let onArchive {
                Button(task.archived ? "取消归档" : "归档") { onArchive() }
                    .accessibilityIdentifier("task.detail.archive")
            }
            if Self.isRunning(task.status), let onCancel {
                Button("取消任务") { onCancel() }
                    .accessibilityIdentifier("task.detail.cancel")
            }
            if let onDelete {
                if confirmingDelete {
                    if Self.isRunning(task.status) {
                        Text("任务仍在运行，确定删除？")
                            .font(.caption)
                            .foregroundStyle(Color.dsDanger)
                            .accessibilityIdentifier("task.detail.delete-confirm")
                    }
                    Button("确认删除", role: .destructive) {
                        confirmingDelete = false
                        onDelete()
                    }
                    .accessibilityIdentifier("task.detail.delete-confirm-button")
                    Button("放弃") { confirmingDelete = false }
                } else {
                    Button("删除") { confirmingDelete = true }
                        .accessibilityIdentifier("task.detail.delete")
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
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
