import AeroricClient
import XCTest
@testable import AeroricUI

/// Fixed disk for the save check: `texts` reads, `folders` lists; anything
/// else fails.
private struct DiskFixture: ProjectResourceReading {
    struct Unreadable: LocalizedError { var errorDescription: String? { "permission denied" } }
    var texts: [String: String] = [:]
    var folders: [String: [String]] = [:]
    func directory(projectID: String, path: String) async throws -> [FilesClient.FileEntry] {
        guard let names = folders[path] else { throw Unreadable() }
        return names.map { .init(name: $0, path: $0, isDir: false, isSymlink: false, modifiedAtMs: nil) }
    }
    func text(projectID: String, path: String) async throws -> String {
        guard let text = texts[path] else { throw Unreadable() }
        return text
    }
    func changes(projectPath: String) async throws -> [ResourceGitChange] { [] }
    func branches(projectPath: String) async throws -> [ResourceGitBranch] { [] }
    func diff(projectPath: String, path: String, staged: Bool) async throws -> String { "" }
}

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

    /// B4-20: typing while the write is in flight survives the save.
    func testTextTypedDuringASaveStaysAndKeepsTheTabDirty() {
        let book = book()
        book.open(file("a"))
        book.edit("p1") { $0.previewText = "one" }
        book.setDraft("two", for: file("a"))
        book.setDraft("two and more", for: file("a"))
        book.markSaved(file("a"), text: "two")
        XCTAssertEqual(book.scene("p1").drafts[file("a")], "two and more")
        XCTAssertEqual(book.draftBase(file("a")), "two")
        XCTAssertTrue(book.isDirty(file("a")))
    }

    /// B4-21: the Trash closes clean tabs; a dirty one stays, marked deleted,
    /// and shows its draft until saved again.
    func testTheTrashKeepsADirtyTabAndClosesCleanOnes() async {
        let book = book()
        book.open(file("dir/clean"))
        book.open(file("dir/dirty"))
        book.edit("p1") { $0.previewText = "base" }
        book.setDraft("edited", for: file("dir/dirty"))
        let front = book.followEdit(projectID: "p1", edit: .trash(path: "dir"))
        XCTAssertEqual(front, file("dir/dirty"))
        XCTAssertEqual(book.scene("p1").tabs, [file("dir/dirty")])
        XCTAssertTrue(book.isDeleted(file("dir/dirty")))
        await book.loadPreview(file("dir/dirty"), projectPath: "/p1", reader: DiskFixture())
        XCTAssertEqual(book.scene("p1").previewState, .loaded)
        XCTAssertEqual(book.scene("p1").previewText, "base")
        XCTAssertEqual(book.scene("p1").drafts[file("dir/dirty")], "edited")
        book.markSaved(file("dir/dirty"), text: "edited")
        XCTAssertFalse(book.isDeleted(file("dir/dirty")))
    }

    /// B4-35: a rename onto an open tab closes a clean one and is refused
    /// over a dirty one, so no two tabs share a path.
    func testARenameOntoAnOpenTabClosesItOrIsRefused() {
        let book = book()
        book.open(file("old"))
        book.open(file("new"))
        XCTAssertNil(book.prepareRename(projectID: "p1", path: "old", newName: "new"))
        XCTAssertEqual(book.scene("p1").tabs, [file("old")])
        book.open(file("new"))
        book.edit("p1") { $0.previewText = "x" }
        book.setDraft("unsaved", for: file("new"))
        XCTAssertEqual(book.prepareRename(projectID: "p1", path: "old", newName: "new"),
                       "「new」有未保存的修改，先保存或关闭它")
        XCTAssertEqual(book.scene("p1").tabs, [file("old"), file("new")])
    }

    /// B4-19: a failed read before a save never lets the save through.
    func testTheSaveCheckNeverTreatsAFailedReadAsUnchanged() async {
        let target = file("dir/a.txt")
        let same = DiskFixture(texts: ["dir/a.txt": "base"])
        let changed = DiskFixture(texts: ["dir/a.txt": "other"])
        let missing = DiskFixture(folders: ["dir": ["b.txt"]])
        let unreadable = DiskFixture(folders: ["dir": ["a.txt"]])
        let check = { (reader: DiskFixture) in await ProjectResourceBook.saveCheck(target, base: "base", reader: reader) }
        let results = [await check(same), await check(changed), await check(missing), await check(unreadable)]
        XCTAssertEqual(results, [.unchanged, .changed, .missing, .failed("permission denied")])
    }

    func testARenameNameIsOneComponent() {
        XCTAssertEqual(FileEditText.nameProblem("a/b"), "名称不能包含 /")
        XCTAssertNil(FileEditText.nameProblem("a.b"))
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
