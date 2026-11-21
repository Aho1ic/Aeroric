@testable import AeroricMobileTransport
import XCTest

/// D4 phase 2 (2026-11-21): a phone's approval shows what the agent got —
/// the Mac replies only once the driver settled it (A2).
final class ApprovalReceiptTests: XCTestCase {
    func testTheMacsReplyBecomesAnOutcome() {
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"accepted":true,"outcome":"resolved"}"#), .resolved)
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"outcome":"sendFailed","reason":"写不进"}"#),
                       .sendFailed(reason: "写不进"))
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"outcome":"ended","reason":"连接结束"}"#),
                       .ended(reason: "连接结束"))
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"outcome":"answeredElsewhere"}"#), .answeredElsewhere)
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"outcome":"unconfirmed","reason":"回到 Mac 上看"}"#),
                       .unconfirmed(reason: "回到 Mac 上看"))
        XCTAssertEqual(CompanionApprovalOutcome.fromReply(#"{"accepted":true}"#), .resolved, "an older Mac")
        XCTAssertNil(CompanionApprovalOutcome.fromReply(#"{"outcome":"what"}"#))
        XCTAssertNil(CompanionApprovalOutcome.fromReply("not json"))
    }

    @MainActor
    private func answered(_ reply: String) throws -> CompanionStore {
        let store = CompanionStore()
        try store.pairWithMac(hostName: "Mac", staticPubB64: ServerHandshakeResponder().publicBase64Url,
                              initialEpoch: "e1", mode: .directLAN)
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 0, mode: .directLAN, snapshotTasks: [],
                              snapshotApprovals: [CompanionApproval(requestId: "r1", taskId: "t1",
                                                                    summary: "rm -rf build", hostEpoch: "e1")])
        try store.submitApproval(requestId: "r1", approve: true)
        XCTAssertEqual(store.pendingApprovals.first?.state, .submitting(approve: true), "发送中")
        store.applyApprovalReply(requestId: "r1", resultJSON: reply)
        XCTAssertTrue(store.outbox.isEmpty, "the command is answered")
        return store
    }

    /// Only "the agent got it" removes the card; anything else keeps it and
    /// says what happened.
    @MainActor
    func testTheCardFollowsWhatTheAgentGot() throws {
        XCTAssertTrue(try answered(#"{"accepted":true,"outcome":"resolved"}"#).pendingApprovals.isEmpty, "已生效")

        let failed = try answered(#"{"accepted":false,"outcome":"sendFailed","reason":"写不进 Claude 的输入"}"#)
        XCTAssertEqual(failed.pendingApprovals.first?.state, .failed(reason: "写不进 Claude 的输入"))
        XCTAssertNoThrow(try failed.submitApproval(requestId: "r1", approve: true), "can be answered again")

        let unsure = try answered(#"{"accepted":false,"outcome":"unconfirmed","reason":"回到 Mac 上看"}"#)
        XCTAssertEqual(unsure.pendingApprovals.first?.state, .unknown(reason: "回到 Mac 上看"))

        let elsewhere = try answered(#"{"accepted":false,"outcome":"answeredElsewhere"}"#)
        XCTAssertEqual(elsewhere.pendingApprovals.first?.state, .answeredElsewhere)
        XCTAssertThrowsError(try elsewhere.submitApproval(requestId: "r1", approve: false))
    }
}
