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

    private func entry(_ id: String, _ harness: String, title: String? = nil, version: String = "1",
                       models: [String] = ["gpt-6", "glm-5"], efforts: [String] = ["low", "high"],
                       modes: [String] = ["ask", "full_access"]) -> SessionConfiguration {
        SessionConfiguration(id: id, version: version, title: title ?? id, agent: harness, model: models.first,
                             effort: efforts.first,
                             options: .init(agents: [harness], models: models, efforts: efforts, permissionModes: modes),
                             harness: harness)
    }

    /// 1123: switching harness keeps the choices, in the new harness's terms.
    func testSwitchingHarnessCarriesTheChoicesOver() {
        let catalogue = [
            entry("codex", "codex"), entry("claude", "claude"),
            entry("agent-config:via-codex-0123", "codex"), entry("agent-config:via-claude-0123", "claude"),
            entry("agent-config:via-claude-9999", "claude"),
            entry("agent-config:mine", "codex", title: "GPT 5.5 · Codex"),
            entry("agent-config:mine-c", "claude", title: "GPT 5.5 · Claude", efforts: []),
        ]
        var draft = NewSessionDraft()
        draft.switchHarness("codex", configurations: catalogue)
        draft.apply(catalogue[2])
        draft.task.model = "glm-5"
        draft.task.effort = "high"
        draft.selectPermission("full_access")
        draft.switchHarness("claude", configurations: catalogue)
        XCTAssertEqual(draft.configurationID, "agent-config:via-claude-0123", "the same connection")
        XCTAssertEqual(draft.task.agent, "claude")
        XCTAssertEqual(draft.task.model, "glm-5")
        XCTAssertEqual(draft.task.effort, "high")
        XCTAssertEqual(draft.task.permissionMode, "full_access")
        XCTAssertTrue(draft.blockingReasons(scope: .project("p"), configurations: catalogue, projectExists: true)
            .allSatisfy { !$0.contains("权限") })
        draft.switchHarness("codex", configurations: catalogue)
        XCTAssertEqual(draft.configurationID, "agent-config:via-codex-0123")

        draft.apply(catalogue[5])
        draft.task.effort = "high"
        draft.switchHarness("claude", configurations: catalogue)
        XCTAssertEqual(draft.configurationID, "agent-config:mine-c", "the same name")
        XCTAssertEqual(draft.task.effort, nil, "an effort the entry does not offer is not kept")

        draft.switchHarness("codex", configurations: catalogue)
        draft.apply(catalogue[0])
        draft.switchHarness("claude", configurations: catalogue)
        XCTAssertEqual(draft.configurationID, "claude", "own login for own login")
    }

    /// 1123: choices are the project's as soon as they are made; a page
    /// opened before the saved ones are read back yields to them.
    func testChoicesAreRememberedWhenMadeAndSurviveARestart() throws {
        var book = NewSessionDraftBook()
        book.open(.project("p1"))
        book.edit(.project("p1")) {
            $0.switchHarness("claude", configurations: [self.entry("claude", "claude")])
            $0.task.model = "glm-5"
            $0.selectPermission("full_access")
        }
        let data = try JSONEncoder().encode(book.memos)
        let memos = try JSONDecoder().decode(NewSessionMemos.self, from: data)
        XCTAssertEqual(memos.preferences["p1"]?.model, "glm-5", "kept without sending")

        var next = NewSessionDraftBook()
        next.open(.project("p1"))
        next.open(.project("p2"))
        next.restore(memos)
        XCTAssertEqual(next[.project("p1")]?.task.model, "glm-5", "an untouched page takes the saved one")
        XCTAssertEqual(next[.project("p1")]?.selectedHarness, "claude")

        var sent = NewSessionDraftBook()
        sent.restore(memos)
        sent.edit(.project("p1")) { $0.task.prompt = "go" }
        let begun = try XCTUnwrap(sent.begin(.project("p1")))
        XCTAssertTrue(sent.acknowledge(.project("p1"), submissionID: begun.submissionID))
        sent.open(.project("p1"))
        XCTAssertEqual(sent[.project("p1")]?.task.model, "glm-5", "the next new session opens with them")
        XCTAssertEqual(sent[.project("p1")]?.task.permissionMode, "full_access")
        XCTAssertEqual(sent[.project("p1")]?.task.prompt, "")
    }

    func testANewerVersionOfTheChosenEntryIsTakenQuietly() {
        var draft = NewSessionDraft()
        draft.switchHarness("codex", configurations: [entry("agent-config:x", "codex")])
        draft.task.model = "glm-5"
        draft.refresh(from: [entry("agent-config:x", "codex", version: "2")])
        XCTAssertEqual(draft.configurationVersion, "2")
        XCTAssertEqual(draft.task.model, "glm-5")
    }

    func testSlashOffersOnlyWhatWorksBeforeASession() {
        XCTAssertEqual(SlashCommand.newSessionSuggestions(for: "/", harness: "claude").map(\.command),
                       ["/记住", "/plan", "/goal"])
        XCTAssertEqual(SlashCommand.newSessionSuggestions(for: "/pl", harness: "claude").map(\.command), ["/plan"])
        XCTAssertTrue(SlashCommand.newSessionSuggestions(for: "/记住 x", harness: "claude").isEmpty)
        XCTAssertFalse(SlashCommand.newSessionSuggestions(for: "/goal", harness: "codex")[0].enabled)
    }
}
