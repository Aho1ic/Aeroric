import XCTest
@testable import AeroricUI

@MainActor
final class FileTabsTests: XCTestCase {
    private func file(_ path: String, project: String = "p1") -> ResourcePreviewTarget {
        ResourcePreviewTarget(projectID: project, path: path, kind: .file)
    }

    private func book() -> ProjectResourceBook {
        let book = ProjectResourceBook()
        let suite = "aeroric.tests.file-tabs.\(UUID().uuidString)"
        book.defaults = UserDefaults(suiteName: suite)!
        addTeardownBlock { UserDefaults().removePersistentDomain(forName: suite) }
        return book
    }

    func testOpeningAgainOnlySwitches() {
        let book = book()
        book.open(file("a.swift"))
        book.open(file("b.swift"))
        book.open(file("a.swift"))
        XCTAssertEqual(book.scene("p1").tabs, [file("a.swift"), file("b.swift")])
        XCTAssertEqual(book.scene("p1").target, file("a.swift"))
        XCTAssertTrue(book.scene("p1").previewActive)
    }

    func testClosingTheFrontTabShowsItsNeighbourThenTheTask() {
        let book = book()
        ["a", "b", "c"].forEach { book.open(file($0)) }
        book.open(file("b"))
        XCTAssertEqual(book.close(file("b")), file("c"))
        XCTAssertEqual(book.close(file("c")), file("a"))
        XCTAssertNil(book.close(file("a")))
        XCTAssertFalse(book.scene("p1").previewActive)
        XCTAssertTrue(book.scene("p1").tabs.isEmpty)
    }

    func testClosingABackgroundTabKeepsTheFrontOne() {
        let book = book()
        ["a", "b"].forEach { book.open(file($0)) }
        XCTAssertEqual(book.close(file("a")), file("b"))
        XCTAssertEqual(book.scene("p1").tabs, [file("b")])
        book.showTask(projectID: "p1")
        XCTAssertNil(book.close(file("b")))
    }

    func testADraftIsDirtyUntilSavedOrReverted() {
        let book = book()
        book.open(file("a"))
        book.edit("p1") { $0.previewText = "one" }
        XCTAssertFalse(book.isDirty(file("a")))
        book.setDraft("two", for: file("a"))
        XCTAssertTrue(book.isDirty(file("a")))
        book.setDraft("one", for: file("a"))
        XCTAssertFalse(book.isDirty(file("a")))
        book.setDraft("three", for: file("a"))
        book.markSaved(file("a"), text: "three")
        XCTAssertFalse(book.isDirty(file("a")))
        XCTAssertEqual(book.scene("p1").previewText, "three")
    }

    func testTabsArePerProjectAndRestoredWithoutMissingFiles() async {
        let book = book()
        book.open(file("keep.md"))
        book.open(file("gone.md"))
        book.open(file("other.md", project: "p2"))
        let restored = ProjectResourceBook()
        restored.defaults = book.defaults
        await restored.restoreTabs(projectID: "p1") { $0.path != "gone.md" }
        XCTAssertEqual(restored.scene("p1").tabs, [file("keep.md")])
        // The task stays in front after a restore.
        XCTAssertFalse(restored.scene("p1").previewActive)
        XCTAssertTrue(restored.scene("p2").tabs.isEmpty)
        await restored.restoreTabs(projectID: "p2") { _ in true }
        XCTAssertEqual(restored.scene("p2").tabs, [file("other.md", project: "p2")])
    }
}
