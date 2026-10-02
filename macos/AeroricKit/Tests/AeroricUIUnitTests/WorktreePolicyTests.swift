import XCTest
@testable import AeroricUI

/// P6-05 验收：WorktreePolicyTests——生命周期判定（create/merge/remove/
/// discarded/remote）、分支名、reopen 反例。
@MainActor
final class WorktreePolicyTests: XCTestCase {

    func testCreatePolicy() {
        XCTAssertNil(WorktreePolicy.canCreate(taskRunning: false, worktreeExists: false))
        XCTAssertEqual(WorktreePolicy.canCreate(taskRunning: true, worktreeExists: false), "task is already running")
        XCTAssertEqual(WorktreePolicy.canCreate(taskRunning: false, worktreeExists: true), "worktree path already exists", "reopen 同名目录拒绝（不覆盖用户文件）")
    }

    func testMergePolicy() {
        XCTAssertNil(WorktreePolicy.canMerge(
            taskRunning: false, worktreeDirty: false, underRoot: true,
            branchMatchesTask: true
        ))
        XCTAssertEqual(
            WorktreePolicy.canMerge(taskRunning: true, worktreeDirty: false, underRoot: true, branchMatchesTask: true),
            "task is running; stop it before merging"
        )
        XCTAssertEqual(
            WorktreePolicy.canMerge(taskRunning: false, worktreeDirty: true, underRoot: true, branchMatchesTask: true),
            "Worktree has uncommitted changes; commit or stash them before merging"
        )
        XCTAssertEqual(
            WorktreePolicy.canMerge(taskRunning: false, worktreeDirty: false, underRoot: false, branchMatchesTask: true),
            "worktree is not under the worktrees root"
        )
        XCTAssertEqual(
            WorktreePolicy.canMerge(taskRunning: false, worktreeDirty: false, underRoot: true, branchMatchesTask: false),
            "worktree does not belong to this task"
        )
    }

    func testRemovePolicyRequiresConfirmation() {
        XCTAssertEqual(WorktreePolicy.canRemove(taskRunning: false, underRoot: true, confirmed: false), "removal requires explicit confirmation")
        XCTAssertNil(WorktreePolicy.canRemove(taskRunning: false, underRoot: true, confirmed: true))
        XCTAssertEqual(WorktreePolicy.canRemove(taskRunning: true, underRoot: true, confirmed: true), "task is running; stop it before removing")
    }

    func testDiscardedBlocksInteractionButKeepsHistory() {
        // 已 discarded 的 task 禁用 resume/merge/discard；历史与路径审计保留
        // （判定只禁交互，不删数据）。
        XCTAssertEqual(
            WorktreePolicy.discardedBlocksInteraction(true),
            "task is discarded; resume/merge/discard are disabled"
        )
        XCTAssertNil(WorktreePolicy.discardedBlocksInteraction(false))
    }

    func testRemoteProjectBlocksWorktree() {
        XCTAssertEqual(WorktreePolicy.remoteProjectBlocksWorktree(true), "remote projects cannot have worktrees")
        XCTAssertNil(WorktreePolicy.remoteProjectBlocksWorktree(false))
    }

    func testBranchNameMatchesCore() {
        // 与 core `task_worktree_branch_name` 对齐。
        XCTAssertEqual(WorktreePolicy.taskWorktreeBranchName("task-1"), "aeroric-task/task-1")
    }
}
