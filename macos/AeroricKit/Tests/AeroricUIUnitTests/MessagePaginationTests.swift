import XCTest
@testable import AeroricUI

/// P5-04 验收：MessagePaginationTests——分页游标守卫、合并不跳动、
/// 大量历史（10,000 条 fixture）可滚动的纯逻辑前提。
@MainActor
final class MessagePaginationTests: XCTestCase {

    func testCursorGuardRejectsNonAdvancingCursor() {
        XCTAssertTrue(SessionHistoryLogic.isValidCursorAdvance(cursor: nil, next: 100))
        XCTAssertTrue(SessionHistoryLogic.isValidCursorAdvance(cursor: 500, next: nil))
        XCTAssertTrue(SessionHistoryLogic.isValidCursorAdvance(cursor: 500, next: 200), "更早页游标必须更小")
        XCTAssertFalse(
            SessionHistoryLogic.isValidCursorAdvance(cursor: 200, next: 200),
            "nextCursor == cursor 是坏游标（React 同款）"
        )
        XCTAssertFalse(SessionHistoryLogic.isValidCursorAdvance(cursor: 200, next: 300))
    }

    func testIngestPageMergesAndAdvances() {
        let paginator = SessionPaginator()
        paginator.reset(sessionKey: "s1", isCodex: false)
        paginator.setLoadingPage(false)
        let first = SessionMessagePage(
            messages: [SessionMessage(role: "user", content: [.text(text: "新")])],
            nextCursor: 400,
            hasMore: true
        )
        XCTAssertTrue(paginator.ingest(page: first))
        XCTAssertEqual(paginator.cursor, 400)
        XCTAssertTrue(paginator.hasMore)
        XCTAssertTrue(paginator.canLoadMore)
    }

    func testIngestRejectsBadCursorAndStops() {
        let paginator = SessionPaginator()
        paginator.reset(sessionKey: "s1", isCodex: false)
        paginator.setLoadingPage(false)
        paginator.ingest(page: SessionMessagePage(messages: [], nextCursor: 300, hasMore: true))
        let bad = SessionMessagePage(messages: [], nextCursor: 300, hasMore: true)
        XCTAssertFalse(paginator.ingest(page: bad), "坏游标拒绝并停止")
        XCTAssertFalse(paginator.hasMore)
    }

    func testResetClearsAcrossSessions() {
        let paginator = SessionPaginator()
        paginator.reset(sessionKey: "s1", isCodex: false)
        paginator.setLoadingPage(false)
        paginator.ingest(page: SessionMessagePage(
            messages: [SessionMessage(role: "user", content: [.text(text: "a")])],
            nextCursor: nil, hasMore: false
        ))
        paginator.reset(sessionKey: "s2", isCodex: true)
        XCTAssertTrue(paginator.messages.isEmpty, "跨会话整表重置（稳定 ID diff 不跨会话）")
        XCTAssertTrue(paginator.isCodex)
    }

    func testTenThousandMessageFixtureMergesAndDedups() {
        // 10,000 条 fixture：合并纯函数在线性时间内完成、顺序保持。
        var fixture: [SessionMessage] = []
        for index in 0..<10_000 {
            fixture.append(SessionMessage(
                role: index.isMultiple(of: 2) ? "user" : "assistant",
                content: [.text(text: "msg-\(index)")]
            ))
        }
        let start = Date()
        let deduped = SessionHistoryLogic.deduplicateToolBlocks(fixture)
        let elapsed = Date().timeIntervalSince(start)
        XCTAssertEqual(deduped.count, 10_000)
        XCTAssertEqual(deduped[0].content[0], .text(text: "msg-0"))
        XCTAssertEqual(deduped[9_999].content[0], .text(text: "msg-9999"))
        XCTAssertLessThan(elapsed, 2.0, "纯逻辑合并不重排全部消息（每字重排防呆）")
    }
}
