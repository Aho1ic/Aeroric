import AeroricClient
import AeroricProtocol
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
        /// B5-3b: an experience was proposed as a skill.
        case skillProposalNeedsYou
        /// R1-2: a message may or may not have reached the session.
        case deliveryNeedsYou
        /// R3-2: the agent keeps failing the same call the same way.
        case retryNeedsYou
        /// 2026-11-14: an unattended session's automatic retries ran out.
        case autoRetryGaveUp

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
            case .skillProposalNeedsYou: "技能提案待审"
            case .deliveryNeedsYou: "投递待确认"
            case .retryNeedsYou: "在重试"
            case .autoRetryGaveUp: "没跑成"
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
        /// CU 2026-10-31: memory and skill rows stand beside the task's own
        /// row, so they get their own id.
        public var id: String {
            switch reason {
            case .memoryNeedsYou: "\(task.id)#memory"
            case .skillProposalNeedsYou: "\(task.id)#skill"
            case .deliveryNeedsYou: "\(task.id)#delivery"
            case .retryNeedsYou: "\(task.id)#retry"
            case .autoRetryGaveUp: "\(task.id)#autoRetry"
            default: task.id
            }
        }
    }

    /// B6 second batch: dream proposals waiting in one scope. They belong
    /// to no session, so they get rows of their own above the tasks.
    public struct DreamRow: Identifiable, Equatable, Sendable {
        /// `nil` is global.
        public var projectID: String?
        public var count: Int
        /// The newest one's kind, to open its tab.
        public var kind: String
        public var atMs: UInt64
        public var id: String { "dream#\(projectID ?? "global")" }

        public init(projectID: String?, count: Int, kind: String, atMs: UInt64) {
            self.projectID = projectID
            self.count = count
            self.kind = kind
            self.atMs = atMs
        }
    }

    /// One row per scope with proposals waiting, newest first.
    public static func dreamRows(_ proposals: [LiveDreamProposal]) -> [DreamRow] {
        let pending = proposals.filter(\.isPending)
        return Dictionary(grouping: pending, by: \.projectId).compactMap { scope, group -> DreamRow? in
            guard let newest = group.max(by: { $0.createdMs < $1.createdMs }) else { return nil }
            return DreamRow(projectID: scope, count: group.count, kind: newest.kind, atMs: newest.createdMs)
        }
        .sorted { ($0.atMs, $0.id) > ($1.atMs, $1.id) }
    }

    /// Failed and interrupted tasks stay listed this long, so old ones do not pile up.
    public static let recentWindowMs: UInt64 = 24 * 60 * 60 * 1000

    /// Waiting first (they block an agent), then the newest. A task's own row
    /// is listed once: its status wins over a failed check, which then shows
    /// as detail. Memory proposals and skill proposals get rows of their own
    /// beside it (CU 2026-10-31: a pending memory hid a skill proposal).
    public static func items(_ tasks: [TaskStore.TaskEntry], checkFailures: [String: CheckFailure] = [:],
                             reviews: [String: CheckFailure] = [:], delegations: [String: CheckFailure] = [:],
                             boards: [String: CheckFailure] = [:], memories: [String: CheckFailure] = [:],
                             skills: [String: CheckFailure] = [:], deliveries: [String: CheckFailure] = [:],
                             retries: [String: CheckFailure] = [:], nowMs: UInt64) -> [Item] {
        tasks.flatMap { task -> [Item] in
            guard !task.archived else { return [] }
            var rows: [Item] = []
            if let row = primary(task, checkFailures: checkFailures, reviews: reviews, delegations: delegations,
                                 boards: boards, nowMs: nowMs) {
                rows.append(row)
            }
            if let memory = memories[task.id] {
                rows.append(Item(task: task, reason: .memoryNeedsYou, detail: memory.summary, atMs: memory.atMs))
            }
            if let skill = skills[task.id] {
                rows.append(Item(task: task, reason: .skillProposalNeedsYou, detail: skill.summary, atMs: skill.atMs))
            }
            if let delivery = deliveries[task.id] {
                rows.append(Item(task: task, reason: .deliveryNeedsYou, detail: delivery.summary, atMs: delivery.atMs))
            }
            if let retry = retries[task.id] {
                rows.append(Item(task: task, reason: .retryNeedsYou, detail: retry.summary, atMs: retry.atMs))
            }
            if let auto = task.autoRetry, auto.state == "gaveUp" {
                let at = task.updatedAtMs ?? task.createdAtMs ?? 0
                if nowMs <= at + recentWindowMs {
                    rows.append(Item(task: task, reason: .autoRetryGaveUp, detail: AutoRetryText.gaveUp(auto), atMs: at))
                }
            }
            return rows
        }
        .sorted { lhs, rhs in
            let l = lhs.reason == .waitingOnYou ? 0 : 1
            let r = rhs.reason == .waitingOnYou ? 0 : 1
            if l != r { return l < r }
            return lhs.atMs > rhs.atMs
        }
    }

    /// The task's own row: its status, else a failed check, review,
    /// delegation or board.
    private static func primary(_ task: TaskStore.TaskEntry, checkFailures: [String: CheckFailure],
                                reviews: [String: CheckFailure], delegations: [String: CheckFailure],
                                boards: [String: CheckFailure], nowMs: UInt64) -> Item? {
        let updated = task.updatedAtMs ?? task.createdAtMs ?? 0
        let check = checkFailures[task.id].flatMap { nowMs <= $0.atMs + recentWindowMs ? $0 : nil }
        switch task.status {
        case "input_required":
            return Item(task: task, reason: .waitingOnYou, detail: check?.summary, atMs: updated)
        // A `where` binds only the pattern before it, so both share one guard.
        case "failed" where nowMs <= updated + recentWindowMs,
             "interrupted" where nowMs <= updated + recentWindowMs:
            let cut = task.status == "interrupted" && task.restartCut
            return Item(task: task, reason: task.status == "failed" ? .failed : .interrupted,
                        detail: check?.summary ?? (cut ? RestartCutText.attention(outward: task.outwardBeforeCut) : nil),
                        atMs: updated)
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
            guard let board = boards[task.id] else { return nil }
            return Item(task: task, reason: .boardNeedsYou, detail: board.summary, atMs: board.atMs)
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
    /// B6 second batch: 记忆整理待确认, one row per scope.
    let dreams: [AttentionQueue.DreamRow]
    let onOpenDreams: ((AttentionQueue.DreamRow) -> Void)?
    /// 2026-11-14: 继续 a session an Aeroric restart cut.
    let onContinue: ((String) async -> Void)?
    /// 2026-11-15: todos waiting for acceptance, and (第二批) Agent
    /// proposals: one row.
    let todoReview: Int
    let todoProposals: Int
    let onOpenTodos: (() -> Void)?

    public init(items: [AttentionQueue.Item], projectName: @escaping (String) -> String?,
                onOpen: @escaping (String) -> Void, onOpenItem: ((AttentionQueue.Item) -> Bool)? = nil,
                dreams: [AttentionQueue.DreamRow] = [], onOpenDreams: ((AttentionQueue.DreamRow) -> Void)? = nil,
                onContinue: ((String) async -> Void)? = nil, todoReview: Int = 0, todoProposals: Int = 0,
                onOpenTodos: (() -> Void)? = nil) {
        self.items = items
        self.projectName = projectName
        self.onOpen = onOpen
        self.onOpenItem = onOpenItem
        self.dreams = dreams
        self.onOpenDreams = onOpenDreams
        self.onContinue = onContinue
        self.todoReview = todoReview
        self.todoProposals = todoProposals
        self.onOpenTodos = onOpenTodos
    }

    private var todoCount: Int { todoReview + todoProposals }
    private var total: Int { items.count + dreams.count + (todoCount > 0 ? 1 : 0) }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if total == 0 {
                ContentUnavailableView("没有需要你处理的事", systemImage: "checkmark.circle",
                                       description: Text("等你回答的审批与问题，以及最近 24 小时失败、被中断或检查没通过的任务会出现在这里。"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    if todoCount > 0 {
                        Button { onOpenTodos?() } label: { todoRow }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("attention.todos")
                    }
                    ForEach(dreams) { dream in
                        Button { onOpenDreams?(dream) } label: { dreamRow(dream) }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("attention.\(dream.id)")
                    }
                    ForEach(items) { item in
                        HStack(spacing: AeroricSpace.m) {
                            Button {
                                if onOpenItem?(item) != true { onOpen(item.task.id) }
                            } label: { row(item) }
                                .buttonStyle(.plain)
                                .accessibilityIdentifier("attention.row.\(item.id)")
                            if item.reason == .interrupted, item.task.restartCut, let onContinue {
                                Button("继续") { Task { await onContinue(item.task.id) } }
                                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                                    .fixedSize()
                                    .help(RestartCutText.help)
                                    .accessibilityIdentifier("attention.continue.\(item.task.id)")
                            }
                        }
                    }
                }
                .listStyle(.inset)
            }
        }
        .frame(maxWidth: NativePanelGeometry.mainShell.readingColumnWidth, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        .windowTitle(WindowTitle("需要我处理", subtitle: total == 0 ? nil : "\(total) 项",
                                 systemImage: "bell.badge"))
        .accessibilityIdentifier("attention.page")
    }

    private var todoRow: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "checkmark.circle")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.accent)
                .frame(width: 14)
            VStack(alignment: .leading, spacing: 2) {
                Text(TodoText.attention(review: todoReview, proposals: todoProposals)).lineLimit(1)
                Text(TodoText.attentionDetail(review: todoReview, proposals: todoProposals))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Spacer()
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
    }

    private func dreamRow(_ dream: AttentionQueue.DreamRow) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "wand.and.stars")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.warning)
                .frame(width: 14)
            VStack(alignment: .leading, spacing: 2) {
                Text(MemoryText.dreamAttention(dream.count)).lineLimit(1)
                Text(dream.projectID.map { projectName($0) ?? $0 } ?? "全局")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Spacer()
            if dream.atMs > 0 {
                Text(RelativeTime.short(Date(timeIntervalSince1970: Double(dream.atMs) / 1000)))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
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
