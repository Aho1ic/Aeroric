import AeroricProtocol
import XCTest

/// H4 (2026-11-21): a page whose cited files a session changed reads as possibly out of date.
final class WikiChangedTests: XCTestCase {
    func testAChangedPageSaysWhichFilesUntilTheNextCompile() {
        var page = LiveWikiPage(slug: "module-core-b", kind: "module", title: "b-cli")
        XCTAssertNil(page.changedNote)
        page.changed = ["core/b/src/main.rs"]
        XCTAssertEqual(page.changedNote, "可能过时：生成后 core/b/src/main.rs 在会话里改过，重新编译后更新")
        page.changed.append("core/b/src/lib.rs")
        XCTAssertEqual(page.changedNote, "可能过时：生成后 core/b/src/main.rs 等 2 个文件在会话里改过，重新编译后更新")
    }
}
