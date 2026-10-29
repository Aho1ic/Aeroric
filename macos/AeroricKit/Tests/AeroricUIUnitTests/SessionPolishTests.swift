@testable import AeroricUI
import AeroricProtocol
import XCTest

@MainActor
final class SessionPolishTests: XCTestCase {
    func testOneMessageBecomesOneSelectableTextWithItsCodeBlock() {
        let markdown = "第一段\n\n第二段 **重点**\n\n- 甲\n- 乙\n\n```swift\nlet x = 1\n```\n\n[链接](https://example.com)"
        let text = MarkdownAttributed.build(markdown, size: 15)
        let plain = text.string
        XCTAssertTrue(plain.contains("第一段\n\n第二段 重点"), plain)
        XCTAssertTrue(plain.contains("• 甲"), plain)
        XCTAssertTrue(plain.contains("let x = 1"), "the code block is part of the same text")
        let code = (plain as NSString).range(of: "let x = 1")
        let style = text.attribute(.paragraphStyle, at: code.location, effectiveRange: nil) as? NSParagraphStyle
        XCTAssertEqual(style?.textBlocks.count, 1, "code blocks keep their background box")
        let strong = (plain as NSString).range(of: "重点")
        let font = text.attribute(.font, at: strong.location, effectiveRange: nil) as? NSFont
        XCTAssertEqual(font?.fontDescriptor.symbolicTraits.contains(.bold), false, "重点 is medium, not bold")
        let link = (plain as NSString).range(of: "链接")
        XCTAssertNotNil(text.attribute(.link, at: link.location, effectiveRange: nil))
    }

    func testStatsTimeTurnsToolsAndFirstTokens() {
        var stats = SessionStats(outputBaseline: 100)
        stats.observe(.turnStarted(turnId: "t"), atMs: 1_000)
        stats.observe(.messageChunk(messageId: "m", role: "assistant", text: "好"), atMs: 3_000)
        stats.observe(.toolCall(id: "c", kind: .command, title: "ls", input: .null), atMs: 4_000)
        stats.observe(.toolCallUpdate(id: "c", status: .completed, outputDelta: nil, result: nil), atMs: 6_000)
        stats.observe(.turnEnded(turnId: "t", reason: .completed, error: nil), atMs: 11_000)
        stats.outputTokens = 1_100
        XCTAssertEqual(stats.turns, 1)
        XCTAssertEqual(stats.steps, 1)
        XCTAssertEqual(stats.toolMs, 2_000)
        XCTAssertEqual(stats.modelMs, 8_000, "the turn minus its tool time")
        XCTAssertEqual(stats.averageFirstTokenMs, 2_000)
        XCTAssertEqual(stats.tokensPerSecond ?? 0, 125, accuracy: 0.01)
    }

    func testStatsTextAndCacheShare() {
        XCTAssertEqual(SessionStatsText.statsChip(turns: 4, steps: 19, tokensPerSecond: 128.2), "4 轮 19 步 · 128 tok/s")
        XCTAssertEqual(SessionStatsText.duration(196_000), "3 分 16 秒")
        XCTAssertEqual(SessionStatsText.duration(38_400), "38.4 秒")
        let usage = UsageSummary(contextUsed: 43_600, contextLimit: 262_000, inputTokens: 485_319,
                                 outputTokens: 19_074, cachedInputTokens: 425_870)
        XCTAssertEqual(usage.totalTokens, 504_393)
        XCTAssertEqual(usage.uncachedInputTokens, 59_449)
        XCTAssertEqual(Int(((usage.cacheHitRate ?? 0) * 100).rounded()), 88)
        XCTAssertEqual(SessionStatsText.tokensChip(total: 504_393, hitRate: usage.cacheHitRate), "504.4K tok · 缓存命中 88%")
    }

    /// A short message measures to its own width, a long one wraps to the
    /// offered width; measuring twice gives the same answer (2026-10-29).
    func testSelectableTextHugsShortMessages() {
        let short = MarkdownAttributed.build("今天星期几", size: 15)
        let long = MarkdownAttributed.build(String(repeating: "很长的一段话", count: 30), size: 15)
        for width in [700.0, 300.0, 700.0] as [CGFloat] {
            XCTAssertLessThan(SelectableMarkdown.measure(short, width: width).width, 120)
            XCTAssertGreaterThan(SelectableMarkdown.measure(long, width: width).width, width - 20)
        }
    }
}
