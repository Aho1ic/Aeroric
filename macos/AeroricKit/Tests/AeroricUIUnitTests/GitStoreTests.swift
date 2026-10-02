import XCTest
@testable import AeroricUI

/// P6-04 验收：GitStoreTests——变更/提交/分支簿记、冲突路径高亮、ours/theirs
/// 侧提取、diff 不截断。
@MainActor
final class GitStoreTests: XCTestCase {

    func testChangeIngestAndStagedSplit() {
        let store = GitStore()
        store.ingest(
            changes: [
                GitStore.Change(path: "a.txt", status: "M", staged: true),
                GitStore.Change(path: "b.txt", status: "?", staged: false),
            ],
            conflictPaths: []
        )
        XCTAssertEqual(store.stagedChanges.count, 1)
        XCTAssertEqual(store.unstagedChanges.count, 1)
    }

    func testConflictPathsHighlight() {
        let store = GitStore()
        store.ingest(
            changes: [GitStore.Change(path: "a.txt", status: "UU", staged: false)],
            conflictPaths: ["a.txt"]
        )
        XCTAssertTrue(store.conflictPaths.contains("a.txt"), "冲突路径高亮数据源")
    }

    func testCommitAndBranchIngest() {
        let store = GitStore()
        store.ingest(commits: [
            GitStore.Commit(hash: "abc", shortHash: "abc", author: "t", date: "2026-10-02", message: "init"),
        ])
        store.ingest(branches: [
            GitStore.Branch(name: "main", current: true),
            GitStore.Branch(name: "feature", current: false, remote: "origin/feature"),
        ])
        XCTAssertEqual(store.commits.count, 1)
        XCTAssertEqual(store.branches.count, 2)
        XCTAssertTrue(store.branches.first { $0.name == "feature" }?.remote == "origin/feature")
    }

    func testDiffNotTruncated() {
        let store = GitStore()
        // 原始 diff 不截断（巨大 diff 也完整持有——下载到用户指定位置由调用方）。
        let big = String(repeating: "line\n", count: 10_000)
        store.setDiff(big)
        XCTAssertEqual(store.diffText, big)
    }

    func testOursTheirsSideExtraction() {
        let preview = """
        context
        <<<<<<< HEAD
        ours line
        =======
        theirs line
        >>>>>>> b
        trailing
        """
        XCTAssertEqual(ConflictResolver.oursSide(preview), "ours line")
        XCTAssertEqual(ConflictResolver.theirsSide(preview), "theirs line")
        // 无标记＝原文。
        XCTAssertEqual(ConflictResolver.oursSide("no markers"), "no markers")
    }
}
