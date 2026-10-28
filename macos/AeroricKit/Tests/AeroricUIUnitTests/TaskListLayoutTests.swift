@testable import AeroricUI
import XCTest

/// The task page's list mode (T1/T2, 2026-10-28).
@MainActor
final class TaskListLayoutTests: XCTestCase {
    private func task(_ id: String, _ project: String, status: String = "done") -> TaskStore.TaskEntry {
        TaskStore.TaskEntry(id: id, projectID: project, title: id, status: status, updatedAtMs: 1)
    }

    func testTasksGroupUngroupedFirstThenByGroupThenUnknownProjects() {
        let projects = [
            TaskBoardProject(id: "a", name: "A", group: "工作"),
            TaskBoardProject(id: "b", name: "B"),
            TaskBoardProject(id: "c", name: "C", group: "工作"),
            TaskBoardProject(id: "e", name: "空项目"),
        ]
        let groups = TaskListLayout.groups([task("1", "a"), task("2", "b"), task("3", "c"), task("4", "x")],
                                           projects: projects)
        XCTAssertEqual(groups.map(\.name), ["", "工作"])
        XCTAssertEqual(groups[0].projects.map(\.id), ["b", "x"], "a project with no tasks is left out")
        XCTAssertEqual(groups[0].projects[1].project.name, "其他项目")
        XCTAssertEqual(groups[1].projects.map(\.id), ["a", "c"])
    }

    func testTheMetaLineLeavesOutWhatIsMissing() {
        var full = task("1", "a")
        full.model = "opus"
        full.worktreeBranch = "fix/import"
        full.additions = 128
        full.deletions = 34
        let line = TaskListLayout.meta(full, remote: false, preset: "DeepSeek")
        XCTAssertTrue(line.hasPrefix("DeepSeek · opus · 分支 fix/import · +128 −34"), line)
        XCTAssertTrue(TaskListLayout.meta(task("2", "a"), remote: true, preset: nil).hasPrefix("远程自带登录"))
        XCTAssertEqual(TaskStatusTag.tint("input_required"), AeroricColor.warning)
        XCTAssertEqual(TaskStatusTag.tint("weird"), AeroricColor.textTertiary)
    }
}
