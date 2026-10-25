import SwiftUI

/// A19: "需要我处理" — the tasks waiting on the user, plus the ones that
/// failed or were interrupted recently. Built from the task mirror only.
public enum AttentionQueue {
    public enum Reason: Equatable, Sendable {
        /// An approval or question waits for an answer (`input_required`).
        case waitingOnYou
        case failed
        /// Stopped without finishing (e.g. the host restarted under it).
        case interrupted

        public var text: String {
            switch self {
            case .waitingOnYou: "等你回答"
            case .failed: "失败了"
            case .interrupted: "被中断"
            }
        }
    }

    public struct Item: Identifiable, Equatable, Sendable {
        public let task: TaskStore.TaskEntry
        public let reason: Reason
        public var id: String { task.id }
    }

    /// Failed and interrupted tasks stay listed this long, so old ones do not pile up.
    public static let recentWindowMs: UInt64 = 24 * 60 * 60 * 1000

    /// Waiting first (they block an agent), then the newest.
    public static func items(_ tasks: [TaskStore.TaskEntry], nowMs: UInt64) -> [Item] {
        tasks.compactMap { task -> Item? in
            guard !task.archived else { return nil }
            switch task.status {
            case "input_required":
                return Item(task: task, reason: .waitingOnYou)
            case "failed", "interrupted":
                guard let updated = task.updatedAtMs ?? task.createdAtMs,
                      nowMs <= updated + recentWindowMs else { return nil }
                return Item(task: task, reason: task.status == "failed" ? .failed : .interrupted)
            default:
                return nil
            }
        }
        .sorted { lhs, rhs in
            let l = lhs.reason == .waitingOnYou ? 0 : 1
            let r = rhs.reason == .waitingOnYou ? 0 : 1
            if l != r { return l < r }
            return (lhs.task.updatedAtMs ?? 0) > (rhs.task.updatedAtMs ?? 0)
        }
    }
}

/// The "需要我处理" page: one row per item; a click opens the task.
public struct AttentionListView: View {
    let items: [AttentionQueue.Item]
    let projectName: (String) -> String?
    let onOpen: (String) -> Void

    public init(items: [AttentionQueue.Item], projectName: @escaping (String) -> String?,
                onOpen: @escaping (String) -> Void) {
        self.items = items
        self.projectName = projectName
        self.onOpen = onOpen
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("需要我处理")
                .font(.system(size: AeroricType.title, weight: .semibold))
                .padding(AeroricSpace.xl)
            if items.isEmpty {
                ContentUnavailableView("没有需要你处理的事", systemImage: "checkmark.circle",
                                       description: Text("等你回答的审批与问题、最近 24 小时失败或被中断的任务会出现在这里。"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(items) { item in
                    Button { onOpen(item.task.id) } label: { row(item) }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("attention.row.\(item.task.id)")
                }
                .listStyle(.inset)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityIdentifier("attention.page")
    }

    private func row(_ item: AttentionQueue.Item) -> some View {
        HStack(spacing: AeroricSpace.m) {
            StatusDot(ActivityState(wireStatus: item.task.status), label: item.task.status)
            AgentGlyph(agent: item.task.agent, size: 14)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.task.title).lineLimit(1)
                Text([item.reason.text, projectName(item.task.projectID)].compactMap { $0 }.joined(separator: " · "))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(item.reason == .waitingOnYou ? AeroricColor.warning : AeroricColor.textSecondary)
            }
            Spacer()
            if let updated = item.task.updatedAtMs {
                Text(RelativeTime.short(Date(timeIntervalSince1970: Double(updated) / 1000)))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
    }
}
