import SwiftUI

/// A19: "需要我处理" — the tasks waiting on the user, plus the ones that
/// failed or were interrupted recently, or whose latest auto-check failed.
/// Built from the task mirror plus the host's list of failed checks.
public enum AttentionQueue {
    public enum Reason: Equatable, Sendable {
        /// An approval or question waits for an answer (`input_required`).
        case waitingOnYou
        case failed
        /// Stopped without finishing (e.g. the host restarted under it).
        case interrupted
        /// The latest auto-check run after a turn failed (A05).
        case checksFailed
        /// B4-1: a review loop stopped for the user's decision.
        case reviewNeedsYou
        /// B4-2: a delegation failed, ran over budget, or was interrupted.
        case delegationNeedsYou
        /// B4-3: a decision on the board waits, or a claim went stale.
        case boardNeedsYou
        /// B5-1: an agent proposed a memory.
        case memoryNeedsYou

        public var text: String {
            switch self {
            case .waitingOnYou: "等你回答"
            case .failed: "失败了"
            case .interrupted: "被中断"
            case .checksFailed: "检查没通过"
            case .reviewNeedsYou: "审查要你定"
            case .delegationNeedsYou: "委派要你定"
            case .boardNeedsYou: "黑板要你定"
            case .memoryNeedsYou: "记忆待确认"
            }
        }
    }

    /// A task's latest failed check run, as the host reports it.
    public struct CheckFailure: Equatable, Sendable {
        public var summary: String
        public var atMs: UInt64
        public init(summary: String, atMs: UInt64) {
            self.summary = summary
            self.atMs = atMs
        }
    }

    public struct Item: Identifiable, Equatable, Sendable {
        public let task: TaskStore.TaskEntry
        public let reason: Reason
        /// The failed check's summary, also on a row listed for another reason.
        public var detail: String?
        /// When the row's reason happened; the queue sorts by it.
        public var atMs: UInt64
        public var id: String { task.id }
    }

    /// Failed and interrupted tasks stay listed this long, so old ones do not pile up.
    public static let recentWindowMs: UInt64 = 24 * 60 * 60 * 1000

    /// Waiting first (they block an agent), then the newest. A task is listed
    /// once: its own status wins over a failed check, which then shows as detail.
    public static func items(_ tasks: [TaskStore.TaskEntry], checkFailures: [String: CheckFailure] = [:],
                             reviews: [String: CheckFailure] = [:], delegations: [String: CheckFailure] = [:],
                             boards: [String: CheckFailure] = [:], memories: [String: CheckFailure] = [:],
                             nowMs: UInt64) -> [Item] {
        tasks.compactMap { task -> Item? in
            guard !task.archived else { return nil }
            let updated = task.updatedAtMs ?? task.createdAtMs ?? 0
            let check = checkFailures[task.id].flatMap { nowMs <= $0.atMs + recentWindowMs ? $0 : nil }
            switch task.status {
            case "input_required":
                return Item(task: task, reason: .waitingOnYou, detail: check?.summary, atMs: updated)
            // A `where` binds only the pattern before it, so both share one guard.
            case "failed" where nowMs <= updated + recentWindowMs,
                 "interrupted" where nowMs <= updated + recentWindowMs:
                return Item(task: task, reason: task.status == "failed" ? .failed : .interrupted,
                            detail: check?.summary, atMs: updated)
            default:
                if let check {
                    return Item(task: task, reason: .checksFailed, detail: check.summary, atMs: check.atMs)
                }
                if let review = reviews[task.id] {
                    return Item(task: task, reason: .reviewNeedsYou, detail: review.summary, atMs: review.atMs)
                }
                if let delegation = delegations[task.id] {
                    return Item(task: task, reason: .delegationNeedsYou, detail: delegation.summary,
                                atMs: delegation.atMs)
                }
                if let board = boards[task.id] {
                    return Item(task: task, reason: .boardNeedsYou, detail: board.summary, atMs: board.atMs)
                }
                guard let memory = memories[task.id] else { return nil }
                return Item(task: task, reason: .memoryNeedsYou, detail: memory.summary, atMs: memory.atMs)
            }
        }
        .sorted { lhs, rhs in
            let l = lhs.reason == .waitingOnYou ? 0 : 1
            let r = rhs.reason == .waitingOnYou ? 0 : 1
            if l != r { return l < r }
            return lhs.atMs > rhs.atMs
        }
    }
}

/// The "需要我处理" page: one row per item; a click opens the task.
public struct AttentionListView: View {
    let items: [AttentionQueue.Item]
    let projectName: (String) -> String?
    let onOpen: (String) -> Void
    /// B5-1: a memory proposal opens the memory page, not the task.
    let onOpenItem: ((AttentionQueue.Item) -> Bool)?

    public init(items: [AttentionQueue.Item], projectName: @escaping (String) -> String?,
                onOpen: @escaping (String) -> Void, onOpenItem: ((AttentionQueue.Item) -> Bool)? = nil) {
        self.items = items
        self.projectName = projectName
        self.onOpen = onOpen
        self.onOpenItem = onOpenItem
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("需要我处理")
                .font(.system(size: AeroricType.title, weight: .semibold))
                .padding(AeroricSpace.xl)
            if items.isEmpty {
                ContentUnavailableView("没有需要你处理的事", systemImage: "checkmark.circle",
                                       description: Text("等你回答的审批与问题，以及最近 24 小时失败、被中断或检查没通过的任务会出现在这里。"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(items) { item in
                    Button {
                        if onOpenItem?(item) != true { onOpen(item.task.id) }
                    } label: { row(item) }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("attention.row.\(item.task.id)")
                }
                .listStyle(.inset)
            }
        }
        .frame(maxWidth: NativePanelGeometry.mainShell.readingColumnWidth, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        .accessibilityIdentifier("attention.page")
    }

    private func row(_ item: AttentionQueue.Item) -> some View {
        HStack(spacing: AeroricSpace.m) {
            StatusDot(ActivityState(wireStatus: item.task.status), label: item.task.status)
            AgentGlyph(agent: item.task.agent, size: 14)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.task.title).lineLimit(1)
                Text([item.reason.text, item.detail, projectName(item.task.projectID)].compactMap { $0 }
                        .joined(separator: " · "))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(item.reason == .waitingOnYou ? AeroricColor.warning : AeroricColor.textSecondary)
            }
            Spacer()
            if item.atMs > 0 {
                Text(RelativeTime.short(Date(timeIntervalSince1970: Double(item.atMs) / 1000)))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
    }
}
