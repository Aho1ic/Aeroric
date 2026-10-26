import AeroricClient
import AeroricProtocol
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

    func testAFailedCheckJoinsTheQueueOnceAndAgesOut() {
        let day = AttentionQueue.recentWindowMs
        let now: UInt64 = 10 * day
        let check = { (at: UInt64) in AttentionQueue.CheckFailure(summary: "测试 2 项失败", atMs: at) }
        let items = AttentionQueue.items([
            task("checked", "done", updated: now - 500),
            task("fail", "failed", updated: now - 100),
            task("stale", "done", updated: now - 2 * day),
            task("gone", "done", updated: now, archived: true),
            task("clean", "running", updated: now),
        ], checkFailures: [
            "checked": check(now - 20),
            "fail": check(now - 90),
            "stale": check(now - day - 1),
            "gone": check(now),
        ], nowMs: now)
        XCTAssertEqual(items.map(\.task.id), ["checked", "fail"])
        XCTAssertEqual(items[0].reason, .checksFailed)
        XCTAssertEqual(items[0].atMs, now - 20, "a failed check sorts by when it finished")
        XCTAssertEqual(items[1].reason, .failed, "the task's own status wins")
        XCTAssertEqual(items[1].detail, "测试 2 项失败")
    }

    func testTheHostListDecodes() {
        let reply: JSONValue = .object(["failures": .array([
            .object(["taskId": .string("t"), "turnId": .string("u"), "summary": .string("检查没跑完"),
                     "atMs": .number("42")]),
        ])])
        let decoded = SessionsClient.decodeAutoCheckFailures(reply)
        XCTAssertEqual(decoded["t"]?.summary, "检查没跑完")
        XCTAssertEqual(decoded["t"]?.atMs, 42)
    }
}
