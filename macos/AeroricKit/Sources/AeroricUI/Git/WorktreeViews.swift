import SwiftUI

/// P6-05 Action 2：worktree 视图集（四视图）。生命周期判定纯函数（Swift 侧
/// 词汇表；与 core `worktree_policy` 同语义）。
public enum WorktreePolicy {
    /// 任务关联 worktree 的分支名（与 core `task_worktree_branch_name` 对齐）。
    public static func taskWorktreeBranchName(_ taskID: String) -> String {
        "aeroric-task/\(taskID)"
    }

    /// 创建判定（running task 拒绝；同名目录拒绝——不能覆盖用户文件）。
    public static func canCreate(taskRunning: Bool, worktreeExists: Bool) -> String? {
        if taskRunning { return "task is already running" }
        if worktreeExists { return "worktree path already exists" }
        return nil
    }

    /// merge 判定（running/dirty/归属/分支）。
    public static func canMerge(
        taskRunning: Bool, worktreeDirty: Bool, underRoot: Bool, branchMatchesTask: Bool
    ) -> String? {
        if taskRunning { return "task is running; stop it before merging" }
        if worktreeDirty { return "Worktree has uncommitted changes; commit or stash them before merging" }
        if !underRoot { return "worktree is not under the worktrees root" }
        if !branchMatchesTask { return "worktree does not belong to this task" }
        return nil
    }

    /// remove 判定（确认；running 拒绝）。
    public static func canRemove(taskRunning: Bool, underRoot: Bool, confirmed: Bool) -> String? {
        if taskRunning { return "task is running; stop it before removing" }
        if !underRoot { return "worktree is not under the worktrees root" }
        if !confirmed { return "removal requires explicit confirmation" }
        return nil
    }

    /// 已 discarded 的 task 禁用 resume/merge/discard（历史与路径审计保留）。
    public static func discardedBlocksInteraction(_ discarded: Bool) -> String? {
        discarded ? "task is discarded; resume/merge/discard are disabled" : nil
    }

    /// 远程项目不得带 worktree。
    public static func remoteProjectBlocksWorktree(_ isRemote: Bool) -> String? {
        isRemote ? "remote projects cannot have worktrees" : nil
    }
}

/// WorktreeCreateSheet（创建 worktree；显式选择 base branch——尊重目标项目
/// 的 branch policy）。
public struct WorktreeCreateSheet: View {
    public let taskID: String
    public let baseBranches: [String]
    /// 目标项目的 branch policy（AGENTS main-only：开发 Aeroric 本身禁止
    /// 新增分支——产品能力仍可给用户其他仓库创建 worktree）。
    public let branchPolicyBlocksNewBranch: Bool
    public var onCreate: ((_ taskID: String, _ baseBranch: String) -> Void)?
    public var onCancel: () -> Void
    @State private var baseBranch = ""

    public init(
        taskID: String,
        baseBranches: [String],
        branchPolicyBlocksNewBranch: Bool = false,
        onCreate: ((_ taskID: String, _ baseBranch: String) -> Void)? = nil,
        onCancel: @escaping () -> Void
    ) {
        self.taskID = taskID
        self.baseBranches = baseBranches
        self.branchPolicyBlocksNewBranch = branchPolicyBlocksNewBranch
        self.onCreate = onCreate
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("创建 Worktree")
                .font(.headline)
            Picker("基于分支", selection: $baseBranch) {
                ForEach(baseBranches, id: \.self) { branch in
                    Text(branch).tag(branch)
                }
            }
            .accessibilityIdentifier("worktree.base-branch")
            if branchPolicyBlocksNewBranch {
                // 目标项目 branch policy（AGENTS main-only）。
                Text("该项目禁止新增分支；worktree 创建已被 branch policy 阻止。")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.warning)
                    .accessibilityIdentifier("worktree.policy-blocked")
            }
            Text("分支名：\(WorktreePolicy.taskWorktreeBranchName(taskID))")
                .font(.caption)
                .foregroundStyle(AeroricColor.textTertiary)
            HStack {
                Button("创建") {
                    onCreate?(taskID, baseBranch)
                }
                .disabled(baseBranch.isEmpty || branchPolicyBlocksNewBranch)
                .accessibilityIdentifier("worktree.create")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("worktree.create-cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 380)
        .accessibilityIdentifier("worktree.create-sheet")
    }
}

/// WorktreeTaskBadge（任务行上的 worktree 标记；discarded 禁用交互）。
public struct WorktreeTaskBadge: View {
    public let taskID: String
    public let branch: String
    public let discarded: Bool
    public var additions: Int?
    public var deletions: Int?

    public init(
        taskID: String, branch: String, discarded: Bool,
        additions: Int? = nil, deletions: Int? = nil
    ) {
        self.taskID = taskID
        self.branch = branch
        self.discarded = discarded
        self.additions = additions
        self.deletions = deletions
    }

    public var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "arrow.triangle.branch")
                .font(.caption2)
                .foregroundStyle(discarded ? AeroricColor.textTertiary : AeroricColor.accent)
            Text(branch)
                .font(.caption2.monospaced())
            if discarded {
                Text("已丢弃")
                    .font(.caption2)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("worktree.badge.discarded")
            }
            if let additions, let deletions {
                Text("+\(additions)/−\(deletions)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("worktree.badge.diffstats")
            }
        }
        .accessibilityIdentifier("worktree.badge.\(taskID)")
    }
}

/// WorktreeMergeSheet（merge 前检查 running/dirty/归属——判定由
/// WorktreePolicy 承载，确认后提交）。
public struct WorktreeMergeSheet: View {
    public let taskID: String
    public let branch: String
    public let baseBranch: String
    public let taskRunning: Bool
    public let worktreeDirty: Bool
    public var onMerge: () -> Void
    public var onCancel: () -> Void

    public init(
        taskID: String, branch: String, baseBranch: String,
        taskRunning: Bool, worktreeDirty: Bool,
        onMerge: @escaping () -> Void, onCancel: @escaping () -> Void
    ) {
        self.taskID = taskID
        self.branch = branch
        self.baseBranch = baseBranch
        self.taskRunning = taskRunning
        self.worktreeDirty = worktreeDirty
        self.onMerge = onMerge
        self.onCancel = onCancel
    }

    private var blockedReason: String? {
        WorktreePolicy.canMerge(
            taskRunning: taskRunning,
            worktreeDirty: worktreeDirty,
            underRoot: true,
            branchMatchesTask: branch == WorktreePolicy.taskWorktreeBranchName(taskID)
        )
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("合并 Worktree")
                .font(.headline)
            Text("\(branch) → \(baseBranch)")
                .font(.callout)
            if let reason = blockedReason {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("worktree.merge-blocked")
            }
            HStack {
                Button("合并") { onMerge() }
                    .disabled(blockedReason != nil)
                    .accessibilityIdentifier("worktree.merge")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("worktree.merge-cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 380)
        .accessibilityIdentifier("worktree.merge-sheet")
    }
}

/// WorktreeDiscardSheet（移除需要确认——二段确认；失败返回逐步骤恢复信息）。
public struct WorktreeDiscardSheet: View {
    public let taskID: String
    public let branch: String
    public let taskRunning: Bool
    public var onDiscard: () -> Void
    public var onCancel: () -> Void
    @State private var confirmed = false

    public init(
        taskID: String, branch: String, taskRunning: Bool,
        onDiscard: @escaping () -> Void, onCancel: @escaping () -> Void
    ) {
        self.taskID = taskID
        self.branch = branch
        self.taskRunning = taskRunning
        self.onDiscard = onDiscard
        self.onCancel = onCancel
    }

    private var blockedReason: String? {
        WorktreePolicy.canRemove(
            taskRunning: taskRunning, underRoot: true, confirmed: confirmed
        )
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("丢弃 Worktree")
                .font(.headline)
            Text("移除 \(branch) 及其工作树（未合并的提交将丢失）。")
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
            Toggle("我已确认丢弃", isOn: $confirmed)
                .accessibilityIdentifier("worktree.discard-confirm")
            if let reason = blockedReason {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("worktree.discard-blocked")
            }
            HStack {
                Button("丢弃", role: .destructive) { onDiscard() }
                    .disabled(blockedReason != nil)
                    .accessibilityIdentifier("worktree.discard")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("worktree.discard-cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 380)
        .accessibilityIdentifier("worktree.discard-sheet")
    }
}
