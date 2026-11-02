@testable import AeroricUI
import XCTest

/// 2026-11-02: an unsent new-session page and the last choices survive a restart.
final class NewSessionMemoTests: XCTestCase {
    func testAnUnsentPageComesBackAndASendingOneDoesNot() throws {
        var book = NewSessionDraftBook()
        book.open(.project("p1"))
        book.edit(.project("p1")) {
            $0.selectHarness("codex")
            $0.task.model = "gpt-6"
            $0.task.effort = "high"
            $0.selectPermission("plan")
            $0.task.prompt = "修好超时"
        }
        book.open(.project("p2"))
        book.edit(.project("p2")) { $0.task.prompt = "正在发" }
        _ = book.begin(.project("p2"))

        let data = try JSONEncoder().encode(book.memos)
        var next = NewSessionDraftBook()
        next.restore(try JSONDecoder().decode(NewSessionMemos.self, from: data))
        let back = try XCTUnwrap(next[.project("p1")])
        XCTAssertEqual(back.task.prompt, "修好超时")
        XCTAssertEqual(back.task.model, "gpt-6")
        XCTAssertEqual(back.task.effort, "high")
        XCTAssertEqual(back.task.permissionMode, "plan")
        XCTAssertEqual(back.selectedHarness, "codex")
        XCTAssertNotEqual(back.submissionID, NewSessionDraft.placeholderID)
        XCTAssertNil(next[.project("p2")], "a page that was sending is not brought back")
    }

    func testSlashOffersOnlyWhatWorksBeforeASession() {
        XCTAssertEqual(SlashCommand.newSessionSuggestions(for: "/", harness: "claude").map(\.command),
                       ["/记住", "/plan", "/goal"])
        XCTAssertEqual(SlashCommand.newSessionSuggestions(for: "/pl", harness: "claude").map(\.command), ["/plan"])
        XCTAssertTrue(SlashCommand.newSessionSuggestions(for: "/记住 x", harness: "claude").isEmpty)
        XCTAssertFalse(SlashCommand.newSessionSuggestions(for: "/goal", harness: "codex")[0].enabled)
    }
}
