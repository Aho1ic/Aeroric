import XCTest
import AeroricProtocol
@testable import AeroricClient

/// Pure request encoding only: no connection, subprocess, files or agent.
final class ApprovedSessionEncodingTests: XCTestCase {
    func testModelAndEffortUseHostCanonicalFields() {
        let request = WorkspaceClient.RunRequest(
            projectPath: "/fixture/project", agent: "codex", prompt: "fixture",
            taskId: "stable-draft-id", model: "fixture-model", effort: "high"
        )
        XCTAssertEqual(request.json["selectedModel"], .string("fixture-model"))
        XCTAssertEqual(request.json["reasoningEffort"], .string("high"))
        XCTAssertEqual(request.json["taskId"], .string("stable-draft-id"))
        // Preserve the existing public encoding contract for older consumers.
        XCTAssertEqual(request.json["model"], .string("fixture-model"))
        XCTAssertEqual(request.json["effort"], .string("high"))
    }
}
