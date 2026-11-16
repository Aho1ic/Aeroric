@testable import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// 2026-11-14 无人值守自动重试.
@MainActor
final class AutoRetryTests: XCTestCase {
    private func task(_ id: String, _ retry: AutoRetryInfo?) -> TaskStore.TaskEntry {
        var entry = TaskStore.TaskEntry(id: id, projectID: "p", title: id, status: "completed", agent: "codex",
                                        createdAtMs: 9, updatedAtMs: 9, archived: false, starred: false)
        entry.autoRetry = retry
        return entry
    }

    /// The record's chain reaches the task mirror.
    func testTheRecordChainIsParsed() {
        let record: JSONValue = .object([
            "id": .string("t"),
            "autoRetry": .object(["state": .string("waiting"), "reason": .string("network"),
                                  "count": .number("1"), "max": .number("3"), "dueMs": .number("1800000000000"),
                                  "turnStartedAt": .number("5")]),
        ])
        let retry = WorkspaceClient.parseTask(record, projectID: "p")?.autoRetry
        XCTAssertEqual(retry, AutoRetryInfo(state: "waiting", reason: "network", count: 1, max: 3,
                                            dueMs: 1_800_000_000_000))
        XCTAssertNil(WorkspaceClient.parseTask(.object(["id": .string("t")]), projectID: "p")?.autoRetry)
    }

    /// The banner says why, when and which try; a given-up chain says so.
    func testTheWordsSayWhyWhenAndWhichTry() {
        let due: UInt64 = 1_800_000_000_000
        let waiting = AutoRetryInfo(state: "waiting", reason: "network", count: 1, max: 3, dueMs: due)
        XCTAssertEqual(AutoRetryText.banner(waiting),
                       "上一轮因为网络不通没跑成，\(AutoRetryText.clock(due)) 自动重试（第 2/3 次）")
        XCTAssertEqual(AutoRetryText.gaveUp(AutoRetryInfo(state: "gaveUp", reason: "quota", count: 1, max: 1)),
                       "自动重试 1 次后仍因为额度用完没跑成")
        XCTAssertEqual(AutoRetryText.gaveUp(AutoRetryInfo(state: "gaveUp", reason: "service", count: 3,
                                                          error: "Agent 没装")),
                       "自动重试 3 次后仍因为服务出错没跑成：Agent 没装")
    }

    /// Only a given-up chain asks for the user; a waiting one does not.
    func testOnlyAGivenUpChainReachesTheAttentionQueue() {
        let items = AttentionQueue.items([
            task("waiting", AutoRetryInfo(state: "waiting", reason: "network", dueMs: 20)),
            task("gaveUp", AutoRetryInfo(state: "gaveUp", reason: "network", count: 3)),
            task("none", nil),
        ], nowMs: 10)
        XCTAssertEqual(items.map(\.id), ["gaveUp#autoRetry"])
        XCTAssertEqual(items.first?.reason, .autoRetryGaveUp)
        XCTAssertEqual(items.first?.detail, "自动重试 3 次后仍因为网络不通没跑成")
    }
}
