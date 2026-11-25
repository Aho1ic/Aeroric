@testable import AeroricMobileTransport
import Foundation
import XCTest

/// 1124d (2026-11-25): the task list a person can find a task in.
final class CompanionTaskListTests: XCTestCase {
    private func task(_ id: String, project: String = "p1", status: String = "done", at: Int64?) -> CompanionTask {
        CompanionTask(id: id, projectId: project, title: id, agent: "claude", status: status, activityMs: at)
    }

    func testWhatIsGoingOnComesFirstThenTheNewest() {
        let tasks = [
            task("old", at: 1), task("new", at: 300), task("running", status: "running", at: 5),
            task("asking", project: "p2", status: "input_required", at: 50), task("undated", at: nil),
        ]
        let sections = CompanionTaskList.sections(tasks, projectId: nil)
        XCTAssertEqual(sections.map(\.title), ["进行中", "最近"])
        XCTAssertEqual(sections[0].tasks.map(\.id), ["asking", "running"])
        XCTAssertEqual(sections[1].tasks.map(\.id), ["new", "old", "undated"], "no time: last")
    }

    func testOneProjectAndTheCap() {
        var tasks = (0..<70).map { task("t\($0)", at: Int64($0)) }
        tasks.append(task("other", project: "p2", at: 1000))
        let all = CompanionTaskList.sections(tasks, projectId: nil)
        XCTAssertEqual(all.count, 1)
        XCTAssertEqual(all[0].tasks.count, CompanionTaskList.recentLimit)
        XCTAssertEqual(all[0].tasks.first?.id, "other")
        XCTAssertEqual(all[0].omitted, 71 - CompanionTaskList.recentLimit)

        let one = CompanionTaskList.sections(tasks, projectId: "p1")
        XCTAssertEqual(one.map(\.title), ["这个项目的任务"])
        XCTAssertEqual(one[0].tasks.count, CompanionTaskList.projectLimit)
        XCTAssertEqual(one[0].tasks.first?.id, "t69")
        XCTAssertFalse(one[0].tasks.contains { $0.projectId == "p2" })
        XCTAssertEqual(one[0].omitted, 70 - CompanionTaskList.projectLimit)
        XCTAssertTrue(CompanionTaskList.sections([], projectId: nil).isEmpty)
    }

    func testARecordsNewestTimeIsItsActivity() {
        XCTAssertEqual(CompanionTaskList.activity(of: ["createdAt": 10, "completedAt": 40]), 40)
        XCTAssertEqual(CompanionTaskList.activity(of: ["createdAt": NSNumber(value: 1_795_522_058_683)]),
                       1_795_522_058_683)
        XCTAssertNil(CompanionTaskList.activity(of: ["createdAt": "soon"]))
    }

    func testAFolderThatIsNotARepositorySaysSo() {
        XCTAssertEqual(CompanionBrowse.unavailable(["available": false, "reason": "not_git"]),
                       "这个项目不是 Git 仓库，没有改动可看")
    }
}
