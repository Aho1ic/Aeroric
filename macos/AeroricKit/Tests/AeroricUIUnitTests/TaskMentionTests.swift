@testable import AeroricUI
import XCTest

/// E12 (2026-11-21): "@" in the session composer references the project's
/// other tasks.
final class TaskMentionTests: XCTestCase {
    private func task(_ id: String, project: String = "p", title: String? = nil, updated: UInt64,
                      archived: Bool = false) -> TaskStore.TaskEntry {
        TaskStore.TaskEntry(id: id, projectID: project, title: title ?? id, status: "completed", agent: "claude",
                            createdAtMs: updated, updatedAtMs: updated, archived: archived, starred: false)
    }

    func testOptionsAreTheProjectsOtherTasksNewestFirst() {
        let me = task("me", updated: 5)
        let tasks = [me, task("old", updated: 1), task("new", title: "修登录", updated: 9),
                     task("gone", updated: 8, archived: true), task("elsewhere", project: "q", updated: 7)]
        XCTAssertEqual(TaskMention.options(tasks, for: me).map(\.id), ["new", "old"])
        XCTAssertEqual(TaskMention.options(tasks, for: me).first?.title, "修登录")
    }

    func testOnlyAnAtThatStartsAWordIsAQuery() {
        XCTAssertEqual(TaskMention.query("@"), "")
        XCTAssertEqual(TaskMention.query("看看 @修"), "修")
        XCTAssertEqual(TaskMention.query("第一行\n@登"), "登")
        XCTAssertNil(TaskMention.query("写信给 a@b.com"))
        XCTAssertNil(TaskMention.query("@修登录 然后"))
        XCTAssertNil(TaskMention.query("没有"))
        XCTAssertEqual(TaskMention.removingQuery("看看 @修"), "看看 ")
        XCTAssertEqual(TaskMention.removingQuery("a@b"), "a@b")
    }

    func testSuggestionsMatchTheTitleSkipPickedAndStopAtTheLimit() {
        let options = [TaskMention.Option(id: "t1", title: "修登录", status: "completed"),
                       TaskMention.Option(id: "t2", title: "写文档", status: "running"),
                       TaskMention.Option(id: "t3", title: "登录页样式", status: "completed")]
        XCTAssertEqual(TaskMention.suggestions("@登录", options: options, picked: []).map(\.id), ["t1", "t3"])
        XCTAssertEqual(TaskMention.suggestions("@", options: options, picked: [options[0]]).map(\.id), ["t2", "t3"])
        XCTAssertEqual(TaskMention.suggestions("@t2", options: options, picked: []).map(\.id), ["t2"])
        XCTAssertEqual(TaskMention.suggestions("没有 at", options: options, picked: []), [])
        let full = (0..<TaskMention.maxReferences).map { TaskMention.Option(id: "x\($0)", title: "x", status: "") }
        XCTAssertEqual(TaskMention.suggestions("@", options: options, picked: full), [])
    }
}
