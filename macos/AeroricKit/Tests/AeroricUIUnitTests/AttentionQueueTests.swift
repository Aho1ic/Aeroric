@testable import AeroricUI
import XCTest

/// A19: which tasks need the user, in which order.
final class AttentionQueueTests: XCTestCase {
    private func task(_ id: String, _ status: String, updated: UInt64, archived: Bool = false) -> TaskStore.TaskEntry {
        TaskStore.TaskEntry(id: id, projectID: "p", title: id, status: status, agent: "claude",
                            createdAtMs: updated, updatedAtMs: updated, archived: archived, starred: false)
    }

    func testWaitingComesFirstThenRecentFailuresAndOldOnesDropOut() {
        let day = AttentionQueue.recentWindowMs
        let now: UInt64 = 10 * day
        let items = AttentionQueue.items([
            task("done", "done", updated: now),
            task("old-fail", "failed", updated: now - day - 1),
            task("fail", "failed", updated: now - 100),
            task("intr", "interrupted", updated: now - 50),
            task("wait-old", "input_required", updated: now - 5 * day),
            task("wait", "input_required", updated: now - 10),
            task("archived", "input_required", updated: now, archived: true),
        ], nowMs: now)
        XCTAssertEqual(items.map(\.task.id), ["wait", "wait-old", "intr", "fail"])
        XCTAssertEqual(items.map(\.reason), [.waitingOnYou, .waitingOnYou, .interrupted, .failed])
    }
}
