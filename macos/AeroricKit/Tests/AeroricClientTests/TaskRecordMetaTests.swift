@testable import AeroricClient
import AeroricProtocol
import XCTest

/// Task records the sidebar's 更多 menu reads (2026-10-28).
final class TaskRecordMetaTests: XCTestCase {
    func testARenamedTaskShowsItsNameAndANewOneItsPrompt() {
        let renamed: JSONValue = .object(["id": .string("t"), "agent": .string("codex"),
                                          "name": .string("修复导入"), "prompt": .string("hi\nmore")])
        XCTAssertEqual(WorkspaceClient.displayTitle(renamed), "修复导入")
        let fresh: JSONValue = .object(["id": .string("t"), "agent": .string("codex"),
                                        "name": .string("codex"), "prompt": .string("hi\nmore")])
        XCTAssertEqual(WorkspaceClient.displayTitle(fresh), "hi", "a new task's name is its agent")
    }

    func testTheSessionIDAndHarnessFollowTheFamily() {
        let record: JSONValue = .object(["id": .string("t"), "sessionFamily": .string("claude"),
                                         "claudeSessionId": .string("c1"), "sessionId": .string("legacy")])
        let task = WorkspaceClient.parseTask(record, projectID: "p")
        XCTAssertEqual(task?.sessionId, "c1")
        XCTAssertEqual(task?.harness, "claude")
        let legacy = WorkspaceClient.parseTask(.object(["id": .string("t"), "sessionId": .string("x")]),
                                               projectID: "p")
        XCTAssertEqual(legacy?.sessionId, "x")
    }
}
