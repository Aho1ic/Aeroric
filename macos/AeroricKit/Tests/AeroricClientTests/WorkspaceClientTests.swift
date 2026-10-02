import XCTest
import AeroricProtocol
@testable import AeroricClient

/// WorkspaceClient 的宽容镜像解析（纯函数面）：认得的字段取走，不认得
/// 的忽略；wire 面由 contracts/smoke 守护。
final class WorkspaceClientTests: XCTestCase {

    func testParseTaskDerivesTitleFromPromptFirstLine() {
        let record: JSONValue = .object([
            "id": .string("t1"),
            "projectId": .string("p1"),
            "status": .string("running"),
            "agent": .string("claude"),
            "prompt": .string("帮我修 bug\n第二行"),
            "createdAt": .number(String(100)),
        ])
        let task = try? XCTUnwrap(WorkspaceClient.parseTask(record, projectID: nil))
        XCTAssertEqual(task?.title, "帮我修 bug", "展示标题＝prompt 首行（展示推导）")
        XCTAssertEqual(task?.status, "running")
        XCTAssertEqual(task?.agent, "claude")
        XCTAssertEqual(task?.createdAtMs, 100)
    }

    func testParseTaskToleratesUnknownShapes() {
        // 无 prompt/无 agent/未知字段：认得的取走，其余忽略，不崩。
        let record: JSONValue = .object([
            "id": .string("t2"),
            "status": .string("todo"),
            "someFutureField": .string("ignored"),
        ])
        let task = try? XCTUnwrap(WorkspaceClient.parseTask(record, projectID: "p-fallback"))
        XCTAssertEqual(task?.id, "t2")
        XCTAssertEqual(task?.projectID, "p-fallback", "缺 projectId 时用调用方回填")
        XCTAssertEqual(task?.title, "t2", "无 prompt 时回退 id")
        XCTAssertNil(task?.agent)
    }

    func testParseTaskWithoutIdIsDropped() {
        let record: JSONValue = .object([
            "status": .string("todo"),
        ])
        XCTAssertNil(WorkspaceClient.parseTask(record, projectID: "p"), "无 id 的记录不进镜像")
    }

    func testRunRequestCarriesOptionalFieldsAndAttachments() {
        let request = WorkspaceClient.RunRequest(
            projectPath: "/tmp/proj",
            agent: "claude",
            prompt: "hi",
            taskId: "t-1",
            model: "m1",
            effort: "low",
            permissionMode: "ask",
            worktree: "/tmp/wt",
            attachmentPaths: ["/a.txt", "/b.txt"]
        )
        guard case let .object(fields) = request.json else {
            return XCTFail("run request must be an object")
        }
        XCTAssertEqual(fields["model"], .string("m1"))
        XCTAssertEqual(fields["effort"], .string("low"))
        XCTAssertEqual(fields["permissionMode"], .string("ask"))
        XCTAssertEqual(fields["worktree"], .string("/tmp/wt"))
        XCTAssertEqual(fields["attachments"], .array([.string("/a.txt"), .string("/b.txt")]))
        // 空可选不出现（wire 面不带空壳）。
        let minimal = WorkspaceClient.RunRequest(
            projectPath: "/p", agent: "a", prompt: "x", taskId: "t"
        )
        guard case let .object(minimalFields) = minimal.json else {
            return XCTFail("minimal request must be an object")
        }
        XCTAssertNil(minimalFields["model"])
        XCTAssertNil(minimalFields["attachments"])
    }
}
