import XCTest
import AeroricClient
@testable import AeroricUI

@MainActor
final class OnboardingAndImportTests: XCTestCase {
    func testStepsRunInOrderAndStopAtTheEnds() {
        XCTAssertEqual(OnboardingStep.allCases, [.welcome, .agents, .projects, .imports, .done])
        XCTAssertNil(OnboardingStep.welcome.previous)
        XCTAssertNil(OnboardingStep.done.next)
        XCTAssertEqual(OnboardingStep.projects.next, .imports)
        XCTAssertTrue(OnboardingStep.allCases.allSatisfy { !$0.title.isEmpty })
    }

    func testOnboardingShowsOnlyWhenReadyAndNotCompleted() {
        XCTAssertTrue(OnboardingFlow.shouldPresent(completed: false, phase: .ready))
        XCTAssertFalse(OnboardingFlow.shouldPresent(completed: true, phase: .ready))
        XCTAssertFalse(OnboardingFlow.shouldPresent(completed: false, phase: .connecting),
                       "never before the host is ready: the import page needs it")
    }

    func testImportRowsOfferEveryImportableAgent() {
        let rows = OnboardingFlow.importRows(detected: [])
        XCTAssertEqual(rows.map(\.agent), ["claude", "codex", "dsh", "omp", "qoder", "cursor", "opencode"])
        XCTAssertTrue(rows.allSatisfy(\.importable))
    }

    func testNativeSessionIDsFollowEachAgentsNaming() {
        XCTAssertEqual(ImportsClient.sessionID(fromSource: "/h/.dsh/sessions/--w--/session-ab12/session.v4.jsonl.zstd",
                                               agent: "dsh"), "ab12")
        XCTAssertEqual(ImportsClient.sessionID(fromSource: "/h/.omp/agent/sessions/-w/2026-09-02T03-47-26-356Z_01a0.jsonl",
                                               agent: "omp"), "01a0")
        XCTAssertEqual(ImportsClient.sessionID(fromSource: "/h/.cursor/chats/9f/chat-7/store.db", agent: "cursor"), "chat-7")
        XCTAssertEqual(ImportsClient.sessionID(fromSource: "/h/.claude/projects/-w/0b1c.jsonl"), "0b1c")
    }

    func testEstimateTextAndPathAbbreviation() {
        XCTAssertEqual(ImportPresentation.estimateText(nil), "正在估算…")
        let missing = ImportsClient.Estimate(agent: "claude", root: "/h/.claude", rootExists: false, sessions: 0, bytes: 0)
        XCTAssertEqual(ImportPresentation.estimateText(missing), "未找到会话目录")
        let found = ImportsClient.Estimate(agent: "codex", root: "/h/.codex", rootExists: true, sessions: 12, bytes: 0)
        XCTAssertTrue(ImportPresentation.estimateText(found).hasPrefix("12 个会话"))
        XCTAssertEqual(ImportPresentation.abbreviated("/Users/a/.claude", home: "/Users/a"), "~/.claude")
        XCTAssertEqual(ImportPresentation.abbreviated("/opt/x", home: "/Users/a"), "/opt/x")
    }

    func testImportedNavigationIDsRoundTripAndDoNotLookLikeTasks() {
        let id = SessionNavigation.imported("imp_1")
        XCTAssertEqual(SessionNavigation.importedID(id), "imp_1")
        XCTAssertNil(SessionNavigation.taskID(id))
        XCTAssertNil(SessionNavigation.importedID(SessionNavigation.task("t1")))
    }

    func testABookWithoutTheCapabilityStaysHidden() {
        let book = ImportBook()
        book.attach(client: nil, capabilityEnabled: true)
        XCTAssertFalse(book.isAvailable)
        XCTAssertTrue(book.unassigned.isEmpty)
    }
}
