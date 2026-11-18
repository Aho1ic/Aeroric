import AeroricProtocol
import XCTest

/// A6 (2026-11-17): the first history page says what of the transcript did
/// not read, in the host's shape.
final class SessionUnreadableTests: XCTestCase {
    private func page(_ unreadable: JSONValue?) -> SessionPage? {
        var fields: [String: JSONValue] = [
            "sessionId": .string("s"), "family": .string("codex"), "messages": .array([]),
            "hasMore": .bool(true), "nextCursor": .string("40"),
        ]
        if let unreadable { fields["unreadable"] = unreadable }
        return SessionPage.decode(.object(fields))
    }

    func testLinesThatDidNotReadAreDecodedAndSaid() {
        let decoded = page(.object(["count": .number("2"), "firstLine": .number("17")]))?.unreadable
        XCTAssertEqual(decoded, SessionUnreadable(count: 2, firstLine: 17))
        XCTAssertEqual(decoded?.text, "历史文件里有 2 行读不出来（第一处在第 17 行）")
        let tail = SessionUnreadable(count: 1)
        XCTAssertEqual(tail.text, "历史文件里有 1 行读不出来（在读到的末尾部分里）")
    }

    func testAFileRefusedWholeSaysWhy() {
        let decoded = page(.object(["count": .number("0"), "reason": .string("DSH 会话版本 9 太新")]))?.unreadable
        XCTAssertEqual(decoded?.reason, "DSH 会话版本 9 太新")
        XCTAssertEqual(decoded?.text, "历史读不出来：DSH 会话版本 9 太新")
    }

    func testNothingToSayIsNil() {
        XCTAssertNil(page(nil)?.unreadable)
        XCTAssertNil(page(.object(["count": .number("0")]))?.unreadable)
        XCTAssertEqual(page(nil)?.hasMore, true)
    }
}
