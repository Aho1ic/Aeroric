@testable import AeroricUI
import AeroricProtocol
import XCTest

@MainActor
final class SessionPolishTests: XCTestCase {
    func testOneMessageBecomesOneSelectableTextWithItsCodeBlock() {
        let markdown = "第一段\n\n第二段 **重点**\n\n- 甲\n- 乙\n\n```swift\nlet x = 1\n```\n\n[链接](https://example.com)"
        let text = MarkdownAttributed.build(markdown, size: 15)
        let plain = text.string
        XCTAssertTrue(plain.contains("第一段\n第二段 重点"), plain)
        XCTAssertTrue(plain.contains("•\t甲\n•\t乙"), plain)
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

    /// An item with several paragraphs and a code block keeps ONE number
    /// (2026-10-29: each paragraph and the code's first line read "1.").
    func testListItemsAreNumberedOnce() {
        let markdown = """
        建议顺序：

        1. **配置 token**

           在 `gateway.env` 写入：

           ```bash
           python3 - <<'PY'
           print(1)
           PY
           ```
        2. **确认模式**

           当前示例是：
           - self_hosted
           - official
        """
        let text = MarkdownAttributed.build(markdown, size: 15).string
        XCTAssertEqual(text.components(separatedBy: "1.\t").count - 1, 1, text)
        XCTAssertEqual(text.components(separatedBy: "2.\t").count - 1, 1, text)
        XCTAssertEqual(text.components(separatedBy: "•\t").count - 1, 2, text)
        XCTAssertTrue(text.contains("python3 - <<'PY'"), text)
        XCTAssertFalse(text.contains("1.\tpython3"), text)
    }

    /// An ended or reopened session fills the 统计栏 from its transcript, and
    /// a chip with nothing known reads "—" instead of disappearing (2026-10-29).
    func testTranscriptStatsFillTheBarWhenNothingIsLive() {
        let decoded = SessionTranscriptStats.decode(.object([
            "inputTokens": .number("1000"), "cachedInputTokens": .number("800"), "outputTokens": .number("50"),
            "contextUsed": .number("600"), "contextLimit": .number("258000"),
            "modelMs": .number("6000"), "toolMs": .number("4000"),
            "firstTokenMsTotal": .number("2000"), "firstTokenSamples": .number("1"),
        ]))
        let transcript = try! XCTUnwrap(decoded)
        let usage = UsageSummary(transcript: transcript)
        XCTAssertEqual(usage.totalTokens, 1050)
        XCTAssertEqual(usage.cacheHitRate, 0.8)
        let stats = SessionStats(transcript: transcript)
        XCTAssertTrue(stats.wholeSession)
        XCTAssertEqual(stats.averageFirstTokenMs, 2000)
        XCTAssertEqual(SessionStatsText.tokensChip(total: nil, hitRate: nil), "— tok")
        XCTAssertEqual(SessionStatsText.contextChip(fraction: nil, used: nil), "—")
        XCTAssertEqual(SessionStatsText.contextChip(fraction: 0.17, used: 43_600), "17%")
    }

    /// A preset uses its first installed family; one not installed, or an
    /// unknown custom name, falls back to the system font (2026-10-29).
    func testChineseFontChoiceResolvesToAnInstalledFamily() {
        let installed: (String) -> Bool = { ["Maple Mono NF CN", "LXGW WenKai"].contains($0) }
        XCTAssertNil(ConversationCJKFont.resolve(.pingFang, custom: "", installed: installed))
        XCTAssertEqual(ConversationCJKFont.resolve(.maple, custom: "", installed: installed), "Maple Mono NF CN")
        XCTAssertNil(ConversationCJKFont.resolve(.mono, custom: "", installed: installed))
        XCTAssertEqual(ConversationCJKFont.resolve(.custom, custom: " LXGW WenKai ", installed: installed), "LXGW WenKai")
        XCTAssertNil(ConversationCJKFont.resolve(.custom, custom: "Nope", installed: installed))
    }

    /// Chinese is drawn in the chosen family, Latin stays in the system font.
    func testChineseGlyphsUseTheChosenFamilyOnly() throws {
        let family = "Songti SC"
        try XCTSkipUnless(ConversationCJKFont.isInstalled(family))
        let text = MarkdownAttributed.build("Deploy 部署", size: 15, cjkFamily: family)
        let line = CTLineCreateWithAttributedString(text)
        let runs = CTLineGetGlyphRuns(line) as! [CTRun]
        let families = runs.map { run -> String in
            let attributes = CTRunGetAttributes(run) as NSDictionary
            let font = attributes[kCTFontAttributeName] as! CTFont
            return CTFontCopyFamilyName(font) as String
        }
        XCTAssertTrue(families.contains(family), "\(families)")
        XCTAssertFalse(families.first?.contains("Songti") ?? true, "Latin keeps the system font: \(families)")
    }

    /// The reference layout (2026-10-29): no blank line between list items,
    /// wrapped list text hangs past the marker, paragraphs get half a line.
    func testListsAreTightAndHangPastTheirMarkers() throws {
        let text = MarkdownAttributed.build("开头\n\n1. 第一项\n2. 第二项\n\n结尾", size: 16)
        let plain = text.string as NSString
        XCTAssertEqual(text.string, "开头\n1.\t第一项\n2.\t第二项\n结尾")
        let item = try XCTUnwrap(text.attribute(.paragraphStyle, at: plain.range(of: "第一项").location,
                                                effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(item.firstLineHeadIndent, 0)
        XCTAssertGreaterThan(item.headIndent, 0, "wrapped lines hang at the text")
        XCTAssertEqual(item.tabStops.first?.location, item.headIndent)
        let last = try XCTUnwrap(text.attribute(.paragraphStyle, at: plain.range(of: "第二项").location,
                                                effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertLessThan(item.paragraphSpacing, last.paragraphSpacing, "tight inside, normal after the list")
        let body = try XCTUnwrap(text.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(body.lineSpacing, 16 * 0.35, accuracy: 0.01)
    }

    /// History splits into turns at each user message; the last reply with
    /// text is the answer, the rest folds as process; timing attaches from
    /// the newest turn back.
    func testHistoryGroupsIntoTurnsWithTheirTiming() {
        let messages: [AeroricUI.SessionMessage] = [
            AeroricUI.SessionMessage(role: "user", content: [.text(text: "前情")], messageId: "u0"),
            AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "好")], messageId: "a0"),
            AeroricUI.SessionMessage(role: "user", content: [.text(text: "你能做什么")], messageId: "u1"),
            AeroricUI.SessionMessage(role: "assistant", content: [.thinking(thinking: "想"), .toolUse(id: "t1", name: "Read", input: "{}")], messageId: "a1"),
            AeroricUI.SessionMessage(role: "user", content: [.toolResult(id: "t1", output: "ok")], messageId: "r1"),
            AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "我可以……")], messageId: "a2"),
        ]
        let turns = TranscriptTurns.group(TranscriptBuilder.items(from: messages, taskRunning: false))
        XCTAssertEqual(turns.count, 2)
        XCTAssertEqual(TranscriptTurns.text(of: turns[1].answer), "我可以……")
        XCTAssertEqual(TranscriptTurns.processSummary(turns[1].process), "思考 · 1 步工具调用")
        let timed = TranscriptTurns.attach([.init(startedAt: 1_000, endedAt: 9_000, tokens: 12_500)], to: turns)
        XCTAssertNil(timed[0].stat, "only the newest turn has a transcript entry here")
        XCTAssertEqual(TranscriptTurns.header(timed[1].stat), "已完成，用时 8 秒")
        XCTAssertEqual(TranscriptTurns.header(nil), "已完成")
    }
}
