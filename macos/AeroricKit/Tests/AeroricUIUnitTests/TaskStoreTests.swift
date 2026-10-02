import XCTest
@testable import AeroricUI

/// P5-03 验收：TaskStore 的镜像/过滤/批量记账与查询视图。
/// 宿主协议面（plan_replay/task.run 等）由 smoke e2e 与宿主测试守护；
/// 这里钉住的是「UI 不推断、只记账」的边界。
@MainActor
final class TaskStoreTests: XCTestCase {

    func testUpsertProjectAndTaskMirrorAuthoritativeState() {
        let store = TaskStore()
        store.upsertProject(id: "p1", path: "/tmp/demo", name: "demo")
        store.upsertTask(TaskStore.TaskEntry(
            id: "t1", projectID: "p1", title: "实现 A",
            status: "running", agent: "claude", createdAtMs: 100, updatedAtMs: 200
        ))
        XCTAssertEqual(store.projects.count, 1)
        XCTAssertEqual(store.projects[0].name, "demo")
        XCTAssertEqual(store.tasks(inProject: "p1").count, 1)
        XCTAssertEqual(store.tasks[0].status, "running", "状态来自快照，不推断")

        // 同 id 再来一遍＝更新（宿主权威），不是新增。
        store.upsertTask(TaskStore.TaskEntry(
            id: "t1", projectID: "p1", title: "实现 A",
            status: "done", agent: "claude", createdAtMs: 100, updatedAtMs: 300
        ))
        XCTAssertEqual(store.tasks.count, 1)
        XCTAssertEqual(store.tasks[0].status, "done")
        XCTAssertEqual(store.tasks[0].updatedAtMs, 300)
    }

    func testRemoveTaskDropsItFromMirror() {
        let store = TaskStore()
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p1", title: "x", status: "todo"))
        store.removeTask(id: "t1")
        XCTAssertTrue(store.tasks.isEmpty)
    }

    func testFilterIsWindowLocalAndCaseInsensitive() {
        let store = TaskStore()
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p", title: "Fix Login", status: "todo"))
        store.upsertTask(TaskStore.TaskEntry(id: "t2", projectID: "p", title: "Write docs", status: "done"))
        store.filterText = "fix"
        XCTAssertEqual(store.visibleTasks.map(\.id), ["t1"], "窗口局部过滤命中标题")
        store.filterText = "done"
        XCTAssertEqual(store.visibleTasks.map(\.id), ["t2"], "也命中状态")
        store.filterText = "  "
        XCTAssertEqual(store.visibleTasks.count, 2, "空白过滤＝不过滤")
    }

    func testBatchResultsAreRecordedPerItem() {
        let store = TaskStore()
        struct Boom: Error {}
        let outcome = store.applyBatchResults([
            ("t1", .success(())),
            ("t2", .failure(Boom())),
        ])
        XCTAssertEqual(outcome.succeeded, ["t1"])
        XCTAssertEqual(outcome.failed.count, 1)
        XCTAssertEqual(outcome.failed[0].id, "t2")
        XCTAssertFalse(outcome.failed[0].reason.isEmpty, "局部失败要带原因，不能被全局提示吞掉")
    }

    func testArchivedAndStarredMirrorWithoutFiltering() {
        // 宿主 archive/star 是权威；镜像不代替过滤（两层边界）。
        let store = TaskStore()
        store.upsertTask(TaskStore.TaskEntry(
            id: "t1", projectID: "p", title: "x", status: "todo", archived: true, starred: true
        ))
        store.filterText = "x"
        XCTAssertEqual(store.visibleTasks.count, 1, "镜像层的归档不参与局部过滤")
        XCTAssertTrue(store.tasks[0].archived)
        XCTAssertTrue(store.tasks[0].starred)
    }
}
