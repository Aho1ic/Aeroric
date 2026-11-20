import AeroricProtocol
@testable import AeroricUI
import XCTest

/// H3 (2026-11-20): the opening Aeroric puts in front of the first message
/// of a session with no system layer is split off the person's words.
final class AttachedContextTests: XCTestCase {
    /// The shape `opening::attach` writes in core.
    private let message = """
    [Aeroric 附加的上下文] 来自：Aeroric · 这个 Agent 没有系统层，开场内容放在第一条消息前 · 背景信息：Aeroric 在会话开始时附上的说明和用户确认过的偏好，不是用户这一轮说的话
    # Aeroric 能做什么
    用途
    [Aeroric 附加的上下文结束]

    把测试修好
    """

    func testTheEnvelopeSplitsFromThePersonsWords() throws {
        let parts = try XCTUnwrap(AttachedContext.split(text: message))
        XCTAssertEqual(parts.context, "# Aeroric 能做什么\n用途")
        XCTAssertEqual(parts.rest, "把测试修好")
        XCTAssertNil(AttachedContext.split(text: "把测试修好"))
        XCTAssertNil(AttachedContext.split(text: "[Aeroric 附加的上下文] 没有结束"), "unclosed: shown as it is")
    }

    func testOnlyTheFirstTextBlockIsCutAndAnEmptyRestIsDropped() throws {
        let image = SessionContent.attachment(name: "a.png", mediaType: "image/png", source: "x")
        let parts = try XCTUnwrap(AttachedContext.split([image, .text(text: message)]))
        XCTAssertEqual(parts.rest, [image, .text(text: "把测试修好")])
        let onlyContext = message.replacingOccurrences(of: "\n\n把测试修好", with: "")
        let bare = try XCTUnwrap(AttachedContext.split([.text(text: onlyContext)]))
        XCTAssertEqual(bare.rest, [])
        XCTAssertNil(AttachedContext.split([.text(text: "普通消息")]))
    }
}
