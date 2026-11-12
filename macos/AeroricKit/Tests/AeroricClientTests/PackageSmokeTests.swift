@testable import AeroricClient
import AeroricProtocol
import XCTest

final class PackageSmokeTests: XCTestCase {
    private static let frameA = #"{"v":1,"id":"a"}"#
    private static let frameB = #"{"v":1,"id":"b"}"#

    func testSplitsCoalescedFrames() {
        var splitter = JSONLFrameSplitter()
        let batch = splitter.feed(Array((Self.frameA + "\n" + Self.frameB + "\n").utf8))
        XCTAssertEqual(batch.lines, [Self.frameA, Self.frameB])
        XCTAssertNil(batch.fault)
        XCTAssertEqual(splitter.pendingByteCount, 0)
    }

    func testPartialFrameIsNotSurfacedAndCompletesLater() {
        var splitter = JSONLFrameSplitter()
        let full = #"{"v":1,"id":"a","method":"t"}"# + "\n"
        let head = Array(full.utf8)
        let cut = head.count / 2
        XCTAssertEqual(splitter.feed(Array(head[0..<cut])).lines, [], "no frame may appear before the newline")
        XCTAssertGreaterThan(splitter.pendingByteCount, 0)
        let frames = splitter.feed(Array(head[cut...]))
        XCTAssertEqual(frames.lines.count, 1)
        XCTAssertNil(frames.fault)
        XCTAssertEqual(RPCEnvelope.decode(frames.lines[0])?.id, "a")
    }

    func testSplitsMultiByteUTF8AcrossChunks() {
        var splitter = JSONLFrameSplitter()
        let payload = Array((#"{"v":1,"result":{"text":"中文😀"}}"# + "\n").utf8)
        let cut = payload.count / 2
        XCTAssertEqual(splitter.feed(Array(payload[0..<cut])).lines, [])
        let frames = splitter.feed(Array(payload[cut...]))
        XCTAssertEqual(frames.lines.count, 1)
        XCTAssertTrue(frames.lines[0].contains("中文😀"), "multi-byte content must not be dropped or mangled")
    }

    /// 2026-11-12: a frame read in many chunks is found once its newline
    /// lands, wherever earlier scans stopped, with the next frame's head kept.
    func testLargeFrameInManyChunksSplitsOnceComplete() {
        var splitter = JSONLFrameSplitter()
        let big = #"{"v":1,"result":{"text":""# + String(repeating: "x", count: 300_000) + #""}}"#
        let stream = Array((big + "\n" + Self.frameA + "\n" + #"{"v":1,"#).utf8)
        var lines: [String] = []
        var start = 0
        while start < stream.count {
            let end = min(start + 4096, stream.count)
            let batch = splitter.feed(Array(stream[start..<end]))
            XCTAssertNil(batch.fault)
            lines += batch.lines
            start = end
        }
        XCTAssertEqual(lines, [big, Self.frameA])
        XCTAssertEqual(splitter.pendingByteCount, #"{"v":1,"#.utf8.count)
        XCTAssertEqual(splitter.feed(Array("\"id\":\"b\"}\n".utf8)).lines, [#"{"v":1,"id":"b"}"#])
        XCTAssertEqual(splitter.pendingByteCount, 0)
    }

    /// A tail past the cap faults with the frames before it, and the splitter
    /// starts clean afterwards.
    func testTailPastCapFaultsAfterEarlierFramesAndResets() {
        var splitter = JSONLFrameSplitter()
        XCTAssertEqual(splitter.feed(Array((Self.frameA + "\n" + "xx").utf8)).lines, [Self.frameA])
        let chunk = [UInt8](repeating: UInt8(ascii: "x"), count: 1 << 16)
        var fault: TransportFault?
        for _ in 0...(ProtocolCodec.maxFrameBytes >> 16) where fault == nil {
            fault = splitter.feed(chunk).fault
        }
        guard case .overlong? = fault else { return XCTFail("expected overlong, got \(String(describing: fault))") }
        XCTAssertEqual(splitter.pendingByteCount, 0)
        XCTAssertEqual(splitter.feed(Array((Self.frameB + "\n").utf8)).lines, [Self.frameB])
    }

    /// Prints the time for a 1MB frame read in 16KB chunks (a socket read
    /// often returns less than the reader's 64KB); quadratic before 2026-11-12.
    func testLargeFrameChunkedSplitTime() {
        let stream = Array((String(repeating: "y", count: 1_000_000) + "\n").utf8)
        measure {
            var splitter = JSONLFrameSplitter()
            var count = 0
            var start = 0
            while start < stream.count {
                let end = min(start + 16_384, stream.count)
                count += splitter.feed(Array(stream[start..<end])).lines.count
                start = end
            }
            XCTAssertEqual(count, 1)
        }
    }

    func testClientReadsProtocolEnvelope() {
        let envelope = decodeFrame(#"{"v":1,"ok":true}"#)
        XCTAssertNotNil(envelope)
        XCTAssertEqual(envelope?.ok, true)
        XCTAssertEqual(ClientMarker.protocolVersion, 1)
    }
}
