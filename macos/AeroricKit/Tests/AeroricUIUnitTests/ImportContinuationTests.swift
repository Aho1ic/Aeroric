@testable import AeroricUI
import XCTest

@MainActor
final class ImportContinuationTests: XCTestCase {
    func testOnlyTheFourHarnessesResumeAndAFolderComesFirst() {
        XCTAssertEqual(ImportContinuation.of(agent: "claude", projectID: "p1", cwd: "/w"), .resume)
        XCTAssertEqual(ImportContinuation.of(agent: "omp", projectID: nil, cwd: "/w/x"), .addProject(path: "/w/x"))
        XCTAssertEqual(ImportContinuation.of(agent: "dsh", projectID: nil, cwd: nil), .forkWithoutProject)
        XCTAssertEqual(ImportContinuation.of(agent: "cursor", projectID: "p1", cwd: "/w"), .forkOnly)
        XCTAssertEqual(ImportContinuation.of(agent: "opencode", projectID: nil, cwd: "/w"), .forkOnly)
    }

    func testTheForkPromptKeepsTheTitleAndTheNewestTurns() {
        let messages = (1...9).map { index in
            SessionMessage(role: index.isMultiple(of: 2) ? "assistant" : "user",
                           content: [.text(text: "第 \(index) 轮"), .toolUse(id: "t", name: "bash", input: "{}")])
        }
        let prompt = ImportForkPrompt.make(agent: "cursor", title: "重构导航", messages: messages)
        XCTAssertTrue(prompt.hasPrefix("以下是从 Cursor 导入的会话「重构导航」"))
        XCTAssertFalse(prompt.contains("第 3 轮"), "only the newest six turns")
        XCTAssertTrue(prompt.contains("用户：第 9 轮"))
        XCTAssertTrue(prompt.contains("助手：第 4 轮"))
        XCTAssertFalse(prompt.contains("bash"), "tool calls are left out")

        let long = [SessionMessage(role: "user", content: [.text(text: String(repeating: "长", count: 900))])]
        XCTAssertTrue(ImportForkPrompt.make(agent: "qoder", title: "t", messages: long).contains("…"))
    }
}
