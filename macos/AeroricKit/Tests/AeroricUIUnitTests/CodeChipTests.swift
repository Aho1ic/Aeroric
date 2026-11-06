import AppKit
@testable import AeroricUI
import XCTest

/// 2026-10-29: code blocks are rounded boxes with even top and bottom
/// padding; inline code is a chip with room kerned in on both sides.
final class CodeChipTests: XCTestCase {
    func testCodeBlockIsRoundedWithEvenPadding() throws {
        let text = MarkdownAttributed.build("看：\n\n```\nCoreSimulator is out of date\n```", size: 15)
        let index = (text.string as NSString).range(of: "CoreSimulator").location
        let style = try XCTUnwrap(text.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle)
        let block = try XCTUnwrap(style.textBlocks.first as? RoundedCodeBlock)
        XCTAssertEqual(block.width(for: .padding, edge: .minY), block.width(for: .padding, edge: .maxY))
    }

    func testInlineCodeIsAChipWithRoomAround() {
        let text = MarkdownAttributed.build("服务 `nexusmail:local`，映射", size: 15)
        let string = text.string as NSString
        let code = string.range(of: "nexusmail:local")
        XCTAssertNotNil(text.attribute(.inlineCode, at: code.location, effectiveRange: nil))
        XCTAssertNil(text.attribute(.backgroundColor, at: code.location, effectiveRange: nil))
        let before = text.attribute(.kern, at: code.location - 1, effectiveRange: nil) as? CGFloat
        let last = text.attribute(.kern, at: NSMaxRange(code) - 1, effectiveRange: nil) as? CGFloat
        XCTAssertEqual(before, MarkdownLayoutManager.chipKern)
        XCTAssertEqual(last, MarkdownLayoutManager.chipKern)
        XCTAssertNil(text.attribute(.inlineCode, at: NSMaxRange(code), effectiveRange: nil))
    }

    /// 2026-11-06: a Markdown table is laid out as a grid, each cell in its
    /// row and column, the header bold; text around it stays paragraphs.
    func testATableIsLaidOutAsAGrid() throws {
        let markdown = "实测结果：\n\n| 检查 | 结果 |\n|---|---|\n| 直连 Gmail `993` | 均超时 |\n| 连接 QQ 邮箱 | 正常 |\n\n原因如下。"
        let text = MarkdownAttributed.build(markdown, size: 15)
        func cell(_ needle: String) throws -> (NSTextTableBlock, NSFont?) {
            let index = (text.string as NSString).range(of: needle, options: .backwards).location
            XCTAssertNotEqual(index, NSNotFound, needle)
            let style = try XCTUnwrap(text.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle)
            let block = try XCTUnwrap(style.textBlocks.first as? NSTextTableBlock, needle)
            return (block, text.attribute(.font, at: index, effectiveRange: nil) as? NSFont)
        }
        let (head, headFont) = try cell("检查")
        XCTAssertEqual(head.table.numberOfColumns, 2)
        XCTAssertEqual(head.startingRow, 0)
        XCTAssertEqual(head.startingColumn, 0)
        XCTAssertTrue(headFont?.fontDescriptor.symbolicTraits.contains(.bold) == true
                      || (headFont?.fontDescriptor.object(forKey: .traits) as? [NSFontDescriptor.TraitKey: Any])?[.weight] != nil)
        let (result, _) = try cell("结果")
        XCTAssertEqual(result.startingColumn, 1)
        XCTAssertTrue(result.table === head.table)
        let (slow, _) = try cell("均超时")
        XCTAssertEqual(slow.startingRow, 1)
        XCTAssertEqual(slow.startingColumn, 1)
        let (ok, _) = try cell("正常")
        XCTAssertEqual(ok.startingRow, 2)
        let after = (text.string as NSString).range(of: "原因如下").location
        let style = text.attribute(.paragraphStyle, at: after, effectiveRange: nil) as? NSParagraphStyle
        XCTAssertTrue(style?.textBlocks.isEmpty ?? true, "the paragraph after the table is not in it")
        XCTAssertGreaterThan(style?.paragraphSpacingBefore ?? 0, 0, "and keeps a gap from it")
    }
}
