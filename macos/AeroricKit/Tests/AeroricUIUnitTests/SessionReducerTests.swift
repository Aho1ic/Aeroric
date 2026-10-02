import XCTest
@testable import AeroricUI

/// P5-04 验收：SessionReducerTests——会话消息解码/合并/去重纯逻辑。
/// 未知事件不使整个会话解码失败；重复 tool_end 不重复 row/usage。
final class SessionReducerTests: XCTestCase {

    func testDecodeMessageFromCoreStyleLine() {
        let line = #"{"role":"assistant","content":[{"type":"text","text":"你好"}],"messageId":"m1"}"#
        let message = SessionMessageDecoder.decodeMessage(fromLine: line)
        XCTAssertEqual(message?.role, "assistant")
        XCTAssertEqual(message?.messageId, "m1")
        XCTAssertEqual(message?.content.count, 1)
        XCTAssertEqual(message?.content[0], .text(text: "你好"))
    }

    func testUnknownEventDoesNotFailWholeSessionDecode() {
        // 未知 type 保留为 raw 块；坏行返回 nil 跳过，不让整批失败。
        let unknown = #"{"role":"assistant","content":[{"type":"mystery_kind","x":1}]}"#
        let decoded = SessionMessageDecoder.decodeMessage(fromLine: unknown)
        XCTAssertNotNil(decoded, "未知 type 保留，不丢弃")
        if case let .text(raw) = decoded?.content[0] {
            XCTAssertTrue(raw.contains("mystery_kind"), "原始形状保留（source ID）")
        } else {
            XCTFail("未知 type 应保留为 raw 块")
        }
        XCTAssertNil(SessionMessageDecoder.decodeMessage(fromLine: "{broken"), "坏行跳过")
    }

    func testDecodeAllKnownContentBlocks() {
        let line = "{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"read_file\",\"input\":\"{\\\"path\\\":\\\"a\\\"}\"},{\"type\":\"tool_result\",\"id\":\"t1\",\"output\":\"ok\"},{\"type\":\"thinking\",\"thinking\":\"hmm\"},{\"type\":\"attachment\",\"name\":\"a.png\",\"mediaType\":\"image/png\",\"source\":\"src\"}]}"
        let message = SessionMessageDecoder.decodeMessage(fromLine: line)
        XCTAssertEqual(message?.content.count, 4)
        XCTAssertEqual(message?.content[0], .toolUse(id: "t1", name: "read_file", input: #"{"path":"a"}"#))
        XCTAssertEqual(message?.content[1], .toolResult(id: "t1", output: "ok"))
        XCTAssertEqual(message?.content[2], .thinking(thinking: "hmm"))
        XCTAssertEqual(message?.content[3], .attachment(name: "a.png", mediaType: "image/png", source: "src"))
    }

    func testMergePagesCodexBoundaryDedup() {
        let earlier = [
            SessionMessage(role: "assistant", content: [.text(text: "共同事件")]),
            SessionMessage(role: "assistant", content: [.text(text: "更早页独有")]),
        ]
        let later = [
            SessionMessage(role: "assistant", content: [.text(text: "共同事件")]),
            SessionMessage(role: "assistant", content: [.text(text: "新内容")]),
        ]
        let merged = SessionHistoryLogic.mergePages(earlier, later, isCodex: true)
        XCTAssertEqual(merged.count, 3, "Codex 边界重复文本去重")
        XCTAssertEqual(merged[0].content[0], .text(text: "共同事件"))
        XCTAssertEqual(merged[2].content[0], .text(text: "新内容"))
    }

    func testMergePagesNonCodexAppends() {
        let earlier = [SessionMessage(role: "user", content: [.text(text: "a")])]
        let later = [SessionMessage(role: "assistant", content: [.text(text: "a")])]
        let merged = SessionHistoryLogic.mergePages(earlier, later, isCodex: false)
        XCTAssertEqual(merged.count, 2, "非 Codex 不去重，直接追加")
    }

    func testDuplicateToolEndDoesNotDuplicateRow() {
        let messages = [
            SessionMessage(role: "assistant", content: [
                .toolUse(id: "t1", name: "run", input: "{}"),
                .toolResult(id: "t1", output: "第一次"),
            ]),
            SessionMessage(role: "assistant", content: [
                .toolResult(id: "t1", output: "第一次（重复 tool_end）"),
            ]),
        ]
        let deduped = SessionHistoryLogic.deduplicateToolBlocks(messages)
        let toolResults = deduped.flatMap { $0.content }.filter {
            if case .toolResult = $0 { return true }
            return false
        }
        XCTAssertEqual(toolResults.count, 1, "重复 tool_end 不重复 row")
    }

    func testLinkSchemeGuard() {
        XCTAssertTrue(ContentBlock.isAllowedLinkScheme(URL(string: "https://example.com")!))
        XCTAssertTrue(ContentBlock.isAllowedLinkScheme(URL(string: "mailto:a@b.c")!))
        XCTAssertFalse(ContentBlock.isAllowedLinkScheme(URL(string: "javascript:alert(1)")!))
        XCTAssertFalse(ContentBlock.isAllowedLinkScheme(URL(string: "file:///etc/passwd")!))
    }
}
