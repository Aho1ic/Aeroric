import XCTest
@testable import AeroricUI

@MainActor
final class CapsuleSliderTests: XCTestCase {
    private typealias Slider = CapsuleSlider<String>

    private let segments = [
        Slider.Segment(value: "codex", title: "Codex", asset: "HarnessCodex"),
        Slider.Segment(value: "claude", title: "Claude Code", systemImage: "sparkle"),
        Slider.Segment(value: "plain", title: "纯文字"),
    ]

    func testIconsWithSelectedTitleShowsOnlyTheSelectedName() {
        let faces = Slider.faces(segments, selection: "claude", mode: .iconsWithSelectedTitle)
        XCTAssertEqual(faces, [
            Slider.Face(title: "", showsImage: true),
            Slider.Face(title: "Claude Code", showsImage: true),
            // No image: the title stays so the segment is never blank.
            Slider.Face(title: "纯文字", showsImage: false),
        ])
    }

    func testIconAndTitleAndTitleOnly() {
        XCTAssertEqual(Slider.faces(segments, selection: "codex", mode: .iconAndTitle).map(\.title),
                       ["Codex", "Claude Code", "纯文字"])
        XCTAssertEqual(Slider.faces(segments, selection: "codex", mode: .iconAndTitle).map(\.showsImage),
                       [true, true, false])
        XCTAssertEqual(Slider.faces(segments, selection: "codex", mode: .titleOnly).map(\.showsImage),
                       [false, false, false])
    }

    func testAnUnknownSelectionHidesEveryOptionalTitle() {
        let faces = Slider.faces(segments, selection: "gone", mode: .iconsWithSelectedTitle)
        XCTAssertEqual(faces.map(\.title), ["", "", "纯文字"])
    }
}
