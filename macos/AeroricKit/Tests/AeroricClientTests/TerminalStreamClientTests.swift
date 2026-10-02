import XCTest
import AeroricProtocol
@testable import AeroricClient

/// TerminalStreamClient 的应答解析（纯函数面）。wire 面的形状由
/// `contracts` 逐字节门禁与 smoke e2e 双向守护；这里钉住的是
/// 「解析不猜」：缺失字段/base64 坏形一律显式报错。
final class TerminalStreamClientTests: XCTestCase {

    private func seamlessJSON(bytes: [UInt8], from: UInt64 = 0, resumeAfter: UInt64) -> JSONValue {
        .object([
            "kind": .string("seamless"),
            "from": .number(String(from)),
            "bytes": .string(Data(bytes).base64EncodedString()),
            "reason": .null,
            "resumeAfter": .number(String(resumeAfter)),
        ])
    }

    func testParseSeamlessTail() throws {
        let tail = try TerminalStreamClient.parseReplayTail(
            seamlessJSON(bytes: [104, 105], resumeAfter: 2)
        )
        XCTAssertEqual(tail.kind, .seamless)
        XCTAssertEqual(tail.from, 0)
        XCTAssertEqual(tail.bytes, [104, 105])
        XCTAssertEqual(tail.resumeAfter, 2)
        XCTAssertNil(tail.reasonKind)
        XCTAssertEqual(tail.nextCursor, .init(bytes: 2), "下一轮记字节游标（ADR-038）")
    }

    func testParseResetWithEvictedReason() throws {
        let tail = try TerminalStreamClient.parseReplayTail(.object([
            "kind": .string("reset"),
            "from": .number(String(0)),
            "bytes": .string(""),
            "reason": .object(["kind": .string("evicted")]),
            "resumeAfter": .number(String(0)),
        ]))
        XCTAssertEqual(tail.kind, .reset)
        XCTAssertEqual(tail.reasonKind, "evicted")
        XCTAssertTrue(tail.bytes.isEmpty)
    }

    func testParseResetCarriesCompactedEarliestAndCursorBeyondNextOffset() throws {
        let compacted = try TerminalStreamClient.parseReplayTail(.object([
            "kind": .string("reset"),
            "from": .number(String(100)),
            "bytes": .string("AA=="),
            "reason": .object(["kind": .string("compacted"), "earliest": .number(String(100))]),
            "resumeAfter": .number(String(101)),
        ]))
        XCTAssertEqual(compacted.reasonKind, "compacted")
        XCTAssertEqual(compacted.reasonDetail, 100)

        let beyond = try TerminalStreamClient.parseReplayTail(.object([
            "kind": .string("reset"),
            "from": .number(String(5)),
            "bytes": .string(""),
            "reason": .object(["kind": .string("cursorBeyondStream"), "nextOffset": .number(String(5))]),
            "resumeAfter": .number(String(5)),
        ]))
        XCTAssertEqual(beyond.reasonKind, "cursorBeyondStream")
        XCTAssertEqual(beyond.reasonDetail, 5)
    }

    func testParseRejectsMalformedReplies() {
        // 缺 resumeAfter
        XCTAssertThrowsError(try TerminalStreamClient.parseReplayTail(.object([
            "kind": .string("seamless"),
            "from": .number(String(0)),
            "bytes": .string(""),
        ])))
        // base64 坏形
        XCTAssertThrowsError(try TerminalStreamClient.parseReplayTail(.object([
            "kind": .string("seamless"),
            "from": .number(String(0)),
            "bytes": .string("!!!not-base64!!!"),
            "resumeAfter": .number(String(0)),
        ])))
    }

    func testCursorKindsMapToWireShape() {
        XCTAssertEqual(
            TerminalStreamClient.Cursor(bytes: 7).json,
            .object(["kind": .string("bytes"), "value": .number(String(7))])
        )
        XCTAssertEqual(
            TerminalStreamClient.Cursor(unitsFromStart: 9).json,
            .object(["kind": .string("unitsFromStart"), "value": .number(String(9))])
        )
    }
}
