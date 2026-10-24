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

    func testImportRowsOfferClaudeAndCodexAndMarkTheRestLater() {
        let rows = OnboardingFlow.importRows(detected: [])
        XCTAssertEqual(rows.map(\.agent), ["claude", "codex", "dsh", "omp"])
        XCTAssertEqual(rows.map(\.importable), [true, true, false, false])
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
