import XCTest
@testable import AeroricUI

/// P5-04 验收：ApprovalRoutingTests——请求按 ID 去重、精确清除不串题、
/// 提交后 host 确认前不抹条目、断线/host restart 恢复。
@MainActor
final class ApprovalRoutingTests: XCTestCase {

    private func frame(eventId: String, tool: String = "bash", extra: [String: Any] = [:]) -> [String: Any] {
        var frame: [String: Any] = [
            "eventId": eventId,
            "clientId": "web-1",
            "sessionId": "s1",
            "toolName": tool,
        ]
        frame.merge(extra) { _, new in new }
        return frame
    }

    func testUpsertDeduplicatesByEventID() {
        let store = PendingRequestStore()
        let first = ApprovalRouting.request(fromFrame: frame(eventId: "ev-1"), kind: .approval)!
        let second = ApprovalRouting.request(
            fromFrame: frame(eventId: "ev-1", tool: "renamed"), kind: .approval
        )!
        store.upsert(first)
        store.upsert(second)
        XCTAssertEqual(store.pending.count, 1, "按 eventId 去重")
        XCTAssertEqual(store.pending[0].toolName, "renamed", "重复事件更新字段")
    }

    func testResolvedClearsExactlyByIDNotByQueueHead() {
        let store = PendingRequestStore()
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-1"), kind: .approval)!)
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-2"), kind: .approval)!)
        store.resolve(eventId: "ev-2")
        XCTAssertEqual(store.pending.map(\.id), ["ev-1"], "精确清除 ev-2，不动队首")
    }

    func testTwoPendingApprovalsDoNotCrossSubmit() {
        let store = PendingRequestStore()
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-a"), kind: .approval)!)
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-b"), kind: .approval)!)
        // 两个请求同时等待：按 eventId 路由（不是队列位置）。
        XCTAssertTrue(ApprovalRouting.canSubmit(eventId: "ev-b", pendingIDs: store.pending.map(\.id)))
        store.markSubmitted(eventId: "ev-b")
        XCTAssertTrue(store.isSubmitted(eventId: "ev-b"))
        XCTAssertFalse(store.isSubmitted(eventId: "ev-a"), "不串题")
    }

    func testSubmitDoesNotRemoveEntryUntilHostConfirms() {
        let store = PendingRequestStore()
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-1"), kind: .approval)!)
        store.markSubmitted(eventId: "ev-1")
        XCTAssertEqual(store.pending.count, 1, "拒绝后 host 确认前不本地抹掉")
        store.resolve(eventId: "ev-1")
        XCTAssertTrue(store.pending.isEmpty, "resolved 到达才清除")
        XCTAssertFalse(store.isSubmitted(eventId: "ev-1"))
    }

    func testOutcomePayloadsMatchDshWaterfallShape() {
        XCTAssertEqual(
            PendingRequestStore.outcomePayload(.approvalAllowedOnce) as NSDictionary,
            ["kind": "result", "value": "allowed-once"] as NSDictionary
        )
        XCTAssertEqual(
            PendingRequestStore.outcomePayload(.approvalRejected) as NSDictionary,
            ["kind": "result", "value": "rejected"] as NSDictionary
        )
        let answers = PendingRequestStore.outcomePayload(.answers([JSONBox(["id": "q1", "selected": ["a"]])]))
        XCTAssertEqual(answers["kind"] as? String, "result")
        // 值即结果本身，无 {ok,value} 信封。
        XCTAssertNil(answers["ok"])
    }

    func testFetchAndClearRecoversAfterHostRestart() {
        let store = PendingRequestStore()
        store.upsert(ApprovalRouting.request(fromFrame: frame(eventId: "ev-1"), kind: .approval)!)
        store.markSubmitted(eventId: "ev-1")
        store.fetchAndClear()
        XCTAssertTrue(store.pending.isEmpty)
        XCTAssertTrue(store.submittedEventIDs.isEmpty, "host restart 后本地簿记失效")
    }

    func testQuestionFrameCarriesQuestionsAndSessionFallback() {
        var frame = frame(eventId: "q-1")
        frame.removeValue(forKey: "sessionId")
        frame["agentId"] = "agent-9"
        frame["questions"] = [["id": "q1", "question": "选哪个？", "options": ["a", "b"]]]
        let request = ApprovalRouting.request(fromFrame: frame, kind: .question)
        XCTAssertEqual(request?.sessionId, "agent-9", "sessionId 缺失回退 agentId")
        XCTAssertEqual(request?.questions.count, 1)
    }

    func testFrameWithoutEventIDIsRejected() {
        XCTAssertNil(ApprovalRouting.request(fromFrame: ["toolName": "x"], kind: .approval))
    }
}
