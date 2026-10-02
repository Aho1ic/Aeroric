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

/// P5-03 生命周期与冲突（AC：创建→pending→running→input_required→done；
/// cancel/failed/interrupted/resume；重启后仍可见；revision conflict
/// 自动刷新＋提示重试）。快照全部来自宿主应答——本套件钉住「镜像不
/// 推断」在状态机上的表现。
extension TaskStoreTests {

    func testLifecycleVocabularyMirrorsHostSnapshots() {
        let store = TaskStore()
        let lifecycle = ["pending", "running", "input_required", "done"]
        var last: String?
        for status in lifecycle {
            store.upsertTask(TaskStore.TaskEntry(
                id: "t1", projectID: "p", title: "x", status: status
            ))
            XCTAssertEqual(store.tasks[0].status, status, "宿主快照逐拍到达，镜像逐拍跟随")
            last = status
        }
        XCTAssertEqual(last, "done")
        // cancel/failed/interrupted/resume 同样是快照直照（不是 store 推导）。
        for status in ["cancelled", "failed", "interrupted"] {
            store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p", title: "x", status: status))
            XCTAssertEqual(store.tasks[0].status, status)
        }
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p", title: "x", status: "running"))
        XCTAssertEqual(store.tasks[0].status, "running", "resume 后回到 running 也由宿主说了算")
    }

    func testTasksSurviveAStoreRestartViaHostSnapshot() {
        // 「重启后仍可见」：重启＝新 store＋宿主快照重灌（镜像无持久化，
        // 权威在宿主——这与 ADR-021 的数据权威一致）。
        let first = TaskStore()
        first.upsertProject(id: "p1", path: "/tmp/demo")
        first.upsertTask(TaskStore.TaskEntry(
            id: "t1", projectID: "p1", title: "长任务", status: "done"
        ))

        let second = TaskStore()
        second.upsertProject(id: "p1", path: "/tmp/demo")
        second.upsertTask(TaskStore.TaskEntry(
            id: "t1", projectID: "p1", title: "长任务", status: "done"
        ))
        XCTAssertEqual(second.tasks.count, 1, "宿主重灌后任务可见")
        XCTAssertEqual(second.tasks[0].title, "长任务")
    }

    func testRevisionConflictRecordsThenClearsOnRefresh() {
        let store = TaskStore()
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p", title: "x", status: "todo"))
        // 另一窗口赢得写：本窗口记录冲突，**不本地覆写**。
        store.recordConflict(taskID: "t1")
        XCTAssertTrue(store.conflictedTaskIDs.contains("t1"))
        // 自动刷新（宿主快照重新到达）后清标记，用户再点重试。
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p", title: "x", status: "todo"))
        store.clearConflict(taskID: "t1")
        XCTAssertFalse(store.conflictedTaskIDs.contains("t1"))
    }

    func testDraftValidationIsFieldWiseAndHostOptionBounded() {
        let options = TaskDraft.Options(
            agents: ["claude", "omp"],
            models: [],
            efforts: ["low", "high"],
            permissionModes: ["ask", "auto"]
        )
        // 全空：prompt/agent 报错。
        let empty = TaskDraft(agent: "", permissionMode: "", prompt: "   ")
        XCTAssertEqual(
            Set(empty.validate(against: options).keys),
            ["prompt", "agent", "permissionMode"],
            "空权限模式也在宿主选项校验范围内"
        )
        // agent 不在宿主选项内。
        let badAgent = TaskDraft(agent: "nope", permissionMode: "ask", prompt: "hi")
        XCTAssertEqual(badAgent.validate(against: options)["agent"], "Agent nope 不在宿主提供的选项内")
        // effort 越界逐字段报。
        let badEffort = TaskDraft(agent: "claude", effort: "ultra", permissionMode: "ask", prompt: "hi")
        XCTAssertEqual(badEffort.validate(against: options)["effort"], "Effort ultra 不在宿主提供的选项内")
        // 合法：零错误。
        let good = TaskDraft(agent: "claude", effort: "low", permissionMode: "ask", prompt: "hi")
        XCTAssertTrue(good.validate(against: options).isEmpty)
        // 空选项集＝宿主未回答，不伪造越界。
        let unbounded = TaskDraft(agent: "anything", permissionMode: "any", prompt: "hi")
        XCTAssertTrue(unbounded.validate(against: .none).isEmpty)
    }

    func testOpenProjectSheetNilPickMeansNoAction() {
        // 客户端「选过」的路径只交给调用方；服务端校验是 host 的职责。
        var picked: String?
        let sheet = OpenProjectSheet(pickPath: { nil })
        sheet.run { picked = $0 }
        XCTAssertNil(picked, "panel 取消不产生任何动作")
        let sheet2 = OpenProjectSheet(pickPath: { "/tmp/chosen" })
        sheet2.run { picked = $0 }
        XCTAssertEqual(picked, "/tmp/chosen")
    }
}
