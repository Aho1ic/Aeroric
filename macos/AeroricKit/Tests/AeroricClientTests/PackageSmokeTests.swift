@testable import AeroricClient
import AeroricProtocol
import XCTest

final class PackageSmokeTests: XCTestCase {
    private static let frameA = #"{"v":1,"id":"a"}"#
    private static let frameB = #"{"v":1,"id":"b"}"#

    func testSplitsCoalescedFrames() {
        var splitter = JSONLFrameSplitter()
        let frames = splitter.feed(Array((Self.frameA + "\n" + Self.frameB + "\n").utf8))
        XCTAssertEqual(frames, [Self.frameA, Self.frameB])
        XCTAssertEqual(splitter.pendingByteCount, 0)
    }

    func testPartialFrameIsNotSurfacedAndCompletesLater() {
        var splitter = JSONLFrameSplitter()
        let full = #"{"v":1,"id":"a","method":"t"}"# + "\n"
        let head = Array(full.utf8)
        let cut = head.count / 2
        XCTAssertEqual(splitter.feed(Array(head[0..<cut])), [], "no frame may appear before the newline")
        XCTAssertGreaterThan(splitter.pendingByteCount, 0)
        let frames = splitter.feed(Array(head[cut...]))
        XCTAssertEqual(frames.count, 1)
        XCTAssertEqual(RPCEnvelope.decode(frames[0])?.id, "a")
    }

    func testSplitsMultiByteUTF8AcrossChunks() {
        var splitter = JSONLFrameSplitter()
        let payload = Array((#"{"v":1,"result":{"text":"中文😀"}}"# + "\n").utf8)
        let cut = payload.count / 2
        XCTAssertEqual(splitter.feed(Array(payload[0..<cut])), [])
        let frames = splitter.feed(Array(payload[cut...]))
        XCTAssertEqual(frames.count, 1)
        XCTAssertTrue(frames[0].contains("中文😀"), "multi-byte content must not be dropped or mangled")
    }

    func testClientReadsProtocolEnvelope() {
        let envelope = decodeFrame(#"{"v":1,"ok":true}"#)
        XCTAssertNotNil(envelope)
        XCTAssertEqual(envelope?.ok, true)
        XCTAssertEqual(ClientMarker.protocolVersion, 1)
    }
}
