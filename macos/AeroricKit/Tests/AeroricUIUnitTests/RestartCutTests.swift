@testable import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// 2026-11-14 宿主重启后续跑.
@MainActor
final class RestartCutTests: XCTestCase {
    private func task(_ id: String, _ status: String, cut: Bool) -> TaskStore.TaskEntry {
        var entry = TaskStore.TaskEntry(id: id, projectID: "p", title: id, status: status, agent: "codex",
                                        createdAtMs: 9, updatedAtMs: 9, archived: false, starred: false)
        entry.restartCut = cut
        return entry
    }

    /// A row the restart cut says so; the user's own stop does not.
    func testARestartCutRowSaysSoAndAStopDoesNot() {
        let items = AttentionQueue.items([task("cut", "interrupted", cut: true),
                                          task("stopped", "interrupted", cut: false)], nowMs: 10)
        let detail = Dictionary(uniqueKeysWithValues: items.map { ($0.task.id, $0.detail) })
        XCTAssertEqual(detail["cut"], RestartCutText.attention)
        XCTAssertEqual(detail["stopped"], .some(nil))
        XCTAssertEqual(TurnBillText.notRunReason("hostRestart"), "Aeroric 重启")
    }

    /// The record's mark reaches the task mirror.
    func testTheRecordMarkIsParsed() {
        let record: JSONValue = .object(["id": .string("t"), "status": .string("interrupted"),
                                         "interruptedBy": .string("hostRestart")])
        XCTAssertEqual(WorkspaceClient.parseTask(record, projectID: "p")?.interruptedBy, "hostRestart")
        XCTAssertNil(WorkspaceClient.parseTask(.object(["id": .string("t")]), projectID: "p")?.interruptedBy)
    }

    /// The switch reads, flips, and goes back with the reason on a failed save.
    func testTheAutoContinueSwitchFlipsAndGoesBackOnAFailedSave() async {
        let store = ResumeAfterRestartStore()
        await store.refresh()
        XCTAssertFalse(store.isAvailable, "no host: hidden")
        store.attach(read: { false }, write: { $0 })
        await store.refresh()
        XCTAssertTrue(store.isAvailable)
        XCTAssertFalse(store.enabled)
        await store.setEnabled(true)
        XCTAssertTrue(store.enabled)
        store.attach(read: { true }, write: { _ in throw NSError(domain: "x", code: 1) })
        await store.refresh()
        await store.setEnabled(false)
        XCTAssertTrue(store.enabled, "put back")
        XCTAssertNotNil(store.lastError)
    }
}
