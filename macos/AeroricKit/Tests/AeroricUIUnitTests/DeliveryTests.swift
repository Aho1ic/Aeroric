import AeroricProtocol
@testable import AeroricUI
import XCTest

/// R1-2: the delivery bar says nothing while things arrive, and one line for
/// the most pressing group otherwise.
final class DeliveryTests: XCTestCase {
    private func item(_ id: String, _ state: String, kind: String = "delegation", reason: String? = nil,
                      at: UInt64 = 1) -> LiveDelivery {
        LiveDelivery(id: id, taskId: "t", kind: kind, label: "委派 · 结果", state: state, reason: reason,
                     createdMs: at, updatedMs: at)
    }

    func testNothingIsSaidWhileEverythingArrives() {
        XCTAssertNil(DeliveryText.notice([item("a", "delivered"), item("b", "sending"), item("c", "waiting")]))
    }

    func testFailedComesFirstThenUnknownThenPaused() {
        let failed = DeliveryText.notice([item("p", "paused"), item("f", "failed", reason: "Claude 未登录")])
        XCTAssertEqual(failed?.text, "没送达：Claude 未登录")
        XCTAssertEqual(failed?.ids, ["f"])
        XCTAssertEqual(failed?.actions.map(\.title), ["重试", "丢弃"])
        let unknown = DeliveryText.notice([item("p", "paused"), item("u", "unknown", reason: "会话记录里没找到")])
        XCTAssertEqual(unknown?.text, "委派 · 结果 不确定是否送达 · 会话记录里没找到")
        let paused = DeliveryText.notice([item("p", "paused"), item("q", "paused", at: 2)])
        XCTAssertEqual(paused?.text, "2 条未发出（你按了停止）")
        XCTAssertEqual(paused?.ids, ["p", "q"])
        XCTAssertEqual(DeliveryText.method(.resume), "delivery.resume")
    }

    func testAUserMessageWaitingForTheTurnIsSaid() {
        let notice = DeliveryText.notice([item("w", "waiting", kind: "user")])
        XCTAssertEqual(notice?.text, "1 条消息等这轮结束后发送")
        XCTAssertEqual(notice?.tone, .quiet)
    }

    func testDeliveryUpdatesDecodeAndTheLatestWins() throws {
        let update = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("3"), "atMs": .number("5"), "type": .string("delivery"),
            "delivery": .object(["id": .string("dv-1"), "taskId": .string("t"), "state": .string("delivered"),
                                 "evidence": .string("echo"), "label": .string("审查 · 意见"),
                                 "updatedMs": .number("9")]),
        ]))
        guard case let .delivery(delivery) = update.kind else { return XCTFail("\(update.kind)") }
        XCTAssertEqual(DeliveryText.evidence(delivery), "Claude 回显")
        XCTAssertEqual(DeliveryText.state(delivery), "已送达")
        let receipt = try LiveSessionUpdate.decode(.object([
            "taskId": .string("t"), "seq": .number("4"), "atMs": .number("6"), "type": .string("deliveryReceipt"),
            "id": .string("dv-1"), "outcome": .string("delivered"),
        ]))
        guard case .deliveryReceipt = receipt.kind else { return XCTFail("\(receipt.kind)") }
    }
}
