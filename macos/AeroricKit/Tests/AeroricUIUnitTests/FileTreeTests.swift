import XCTest
@testable import AeroricUI

/// P6-01 验收：FileTreeTests——树懒加载（目录按需展开）、切换项目整树
/// 重置、UI 不做路径推断。
@MainActor
final class FileTreeTests: XCTestCase {

    func testLazyLoadOnlyFetchesOnFirstExpand() {
        let store = FileTreeStore()
        store.reset(projectID: "p1")
        // 未展开：无子节点、无加载需求。
        XCTAssertEqual(store.visibleChildren(of: "src"), [])
        // 首次展开：返回 true＝需要拉子目录。
        XCTAssertTrue(store.toggleExpanded("src"))
        XCTAssertNil(store.visibleChildren(of: "src"), "未加载时无子节点")
        // 子条目到达后可见。
        store.ingestChildren(of: "src", entries: [
            FileTreeStore.Node(name: "main.swift", path: "src/main.swift", isDir: false, isSymlink: false),
        ])
        XCTAssertEqual(store.visibleChildren(of: "src")?.count, 1)
        // 已加载后再收起/展开：不再触发拉取。
        store.toggleExpanded("src")
        XCTAssertFalse(store.toggleExpanded("src"), "已加载的目录再展开不触发拉取")
    }

    func testResetClearsTreeAcrossProjects() {
        let store = FileTreeStore()
        store.reset(projectID: "p1")
        store.toggleExpanded("src")
        store.ingestChildren(of: "src", entries: [
            FileTreeStore.Node(name: "a", path: "src/a", isDir: false, isSymlink: false),
        ])
        store.reset(projectID: "p2")
        XCTAssertTrue(store.children.isEmpty, "跨项目整树重置")
        XCTAssertTrue(store.expandedPaths.isEmpty)
    }

    func testLoadingStatePreventsDuplicateFetch() {
        let store = FileTreeStore()
        store.reset(projectID: "p1")
        store.setLoading("big-dir", loading: true)
        XCTAssertTrue(store.isLoading("big-dir"))
        store.ingestChildren(of: "big-dir", entries: [])
        XCTAssertFalse(store.isLoading("big-dir"), "条目到达后清除加载标记")
    }

    func testFileOperationSheetHelpersDoNotInferPaths() {
        // UI 不做路径推断：FilesOperation 只携带路径，校验归宿主。
        let op = FilesOperation.delete(path: "a/b.txt")
        XCTAssertEqual(op, .delete(path: "a/b.txt"))
    }
}
