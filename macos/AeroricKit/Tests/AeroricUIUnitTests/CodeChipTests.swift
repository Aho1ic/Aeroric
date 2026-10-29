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
}
