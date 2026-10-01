@testable import AeroricClient
import AeroricProtocol
import XCTest

final class EventReducerTests: XCTestCase {
    private func frame(_ seq: UInt64, _ event: String = "task.updated",
                       epoch: String = "e1", data: JSONValue = .null) -> EventFrame {
        EventFrame(event: event, epoch: epoch, seq: Quantity(seq), data: data)
    }

    func testFirstLiveEventAppliesAndSetsWatermark() {
        var reducer = EventReducer()
        let effects = reducer.apply(frame(1))
        // First contact also announces the epoch — with the empty origin.
        XCTAssertEqual(effects.last, .applied(frame(1)))
        XCTAssertTrue(effects.contains(.epochChanged(from: "", to: "e1")))
        XCTAssertEqual(reducer.watermark.uint64Value, 1)
    }

    func testUnknownEventNameStillApplies() {
        var reducer = EventReducer()
        _ = reducer.apply(frame(1))
        let effects = reducer.apply(frame(2, "brand.new.event.nobody.knows"))
        XCTAssertEqual(effects, [.applied(frame(2, "brand.new.event.nobody.knows"))],
                       "unknown event names must reach the upper layer, never be dropped")
        XCTAssertEqual(reducer.watermark.uint64Value, 2)
    }

    func testDuplicateLiveEventIsDroppedWithoutProgress() {
        var reducer = EventReducer()
        _ = reducer.apply(frame(1))
        let effects = reducer.apply(frame(1))
        XCTAssertEqual(effects, [.duplicateDropped(seq: Quantity(1))])
        XCTAssertEqual(reducer.watermark.uint64Value, 1)
    }

    func testLiveGapDoesNotAdvanceWatermark() {
        var reducer = EventReducer()
        _ = reducer.apply(frame(1))
        let effects = reducer.apply(frame(3))
        XCTAssertEqual(effects, [.replayGap(expected: Quantity(2), received: Quantity(3))])
        XCTAssertEqual(reducer.watermark.uint64Value, 1)
    }

    func testZeroSeqIsMalformedOrderingNotAnEvent() {
        var reducer = EventReducer()
        let effects = reducer.apply(frame(0))
        XCTAssertEqual(effects, [.epochChanged(from: "", to: "e1"),
                                 .replayGap(expected: Quantity(1), received: Quantity(0))])
        XCTAssertEqual(reducer.watermark.uint64Value, 0)
    }

    func testEpochChangeResetsWatermark() {
        var reducer = EventReducer()
        _ = reducer.apply(frame(1))
        _ = reducer.apply(frame(2))
        let effects = reducer.apply(frame(1, epoch: "e2"))
        XCTAssertEqual(effects.first, .epochChanged(from: "e1", to: "e2"))
        XCTAssertEqual(effects.last, .applied(frame(1, epoch: "e2")))
        XCTAssertEqual(reducer.watermark.uint64Value, 1, "new epoch starts the watermark at zero again")
    }

    func testBackfillBatchAppliesInOrder() {
        var reducer = EventReducer()
        let batch = ReplayBatch(
            events: [ReplayEvent(seq: Quantity(1), event: "a", data: .null),
                     ReplayEvent(seq: Quantity(2), event: "b", data: .null)],
            latestSeq: Quantity(2),
            reset: false
        )
        let effects = reducer.apply(batch, epoch: "e1")
        XCTAssertEqual(effects[0], .epochChanged(from: "", to: "e1"))
        XCTAssertEqual(effects.count, 3, "epoch change + two applied events")
        XCTAssertEqual(reducer.watermark.uint64Value, 2)
        XCTAssertEqual(reducer.epoch, "e1")
    }

    func testResetBatchLiftsWatermarkAndDemandsFullRefresh() {
        var reducer = EventReducer()
        _ = reducer.apply(frame(1, epoch: "e1"))
        let batch = ReplayBatch(events: [], latestSeq: Quantity(9), reset: true)
        let effects = reducer.apply(batch, epoch: "e1")
        XCTAssertEqual(effects, [.needsFullRefresh(reason: "replay ring wrapped past the requested hole")])
        XCTAssertEqual(reducer.watermark.uint64Value, 9)
    }

    func testBatchDecodeAcceptsWellFormedResult() throws {
        let result: JSONValue = .object([
            "events": .array([
                .object(["seq": .number("1"), "event": .string("a"), "data": .null]),
                .object(["seq": .number("2"), "event": .string("b")]), // `data` defaults to null
            ]),
            "latestSeq": .number("2"),
            "reset": .bool(false),
        ])
        let batch = ReplayBatch.decode(result)
        XCTAssertNotNil(batch)
        XCTAssertEqual(batch?.events.count, 2)
        XCTAssertEqual(batch?.events[1].data, .null, "missing `data` decodes as null")
        XCTAssertEqual(batch?.latestSeq.uint64Value, 2)
        XCTAssertEqual(batch?.reset, false)
    }

    func testBatchDecodeRejectsNonObject() {
        XCTAssertNil(ReplayBatch.decode(.string("nope")))
        XCTAssertNil(ReplayBatch.decode(.object(["events": .array([])])), "missing latestSeq is not a batch")
    }
}
