import XCTest
@testable import AeroricClient
@testable import AeroricUI

final class FileEditTests: XCTestCase {
    func testChildAndParentPaths() {
        XCTAssertEqual(FileEditText.child(of: "", name: "a.txt"), "a.txt")
        XCTAssertEqual(FileEditText.child(of: "docs", name: "a.txt"), "docs/a.txt")
        XCTAssertEqual(FileEditText.parent(of: "docs/sub/a.txt"), "docs/sub")
        XCTAssertEqual(FileEditText.parent(of: "a.txt"), "")
        XCTAssertEqual(FileEditText.name(of: "docs/sub/a.txt"), "a.txt")
    }

    func testTheFolderThatReloadsAfterAnEdit() {
        XCTAssertEqual(FileEdit.create(parent: "docs", name: "a", directory: false).parent, "docs")
        XCTAssertEqual(FileEdit.rename(path: "docs/a.txt", newName: "b.txt").parent, "docs")
        XCTAssertEqual(FileEdit.trash(path: "a.txt").parent, "")
    }

    func testLinkedFoldersAreNotWritable() {
        XCTAssertTrue(FileEditText.writable("docs/a.txt"))
        XCTAssertFalse(FileEditText.writable(LinkedFolderPath.node(folder: "/Users/me/ref", relative: "a.txt")))
    }

    @MainActor
    func testSavingOverAChangedFileNeedsADecision() {
        XCTAssertFalse(ProjectResourceBook.saveConflicts(disk: "same", base: "same"))
        XCTAssertTrue(ProjectResourceBook.saveConflicts(disk: "agent edit", base: "same"))
    }

    @MainActor
    private func book(tabs paths: [String], front: String) -> ProjectResourceBook {
        let book = ProjectResourceBook()
        book.defaults = UserDefaults(suiteName: "FileEditTests.\(UUID().uuidString)")!
        for path in paths { book.open(.init(projectID: "p", path: path, kind: .file)) }
        book.open(.init(projectID: "p", path: front, kind: .file))
        return book
    }

    /// 2026-10-30: renaming a file or folder carries its open tabs and drafts along.
    @MainActor
    func testTabsFollowARename() {
        let book = book(tabs: ["docs/a.txt", "docs/sub/b.txt", "docsx/c.txt"], front: "docs/sub/b.txt")
        book.setDraft("edited", for: .init(projectID: "p", path: "docs/sub/b.txt", kind: .file))
        let front = book.followEdit(projectID: "p", edit: .rename(path: "docs", newName: "notes"))
        XCTAssertEqual(book.scene("p").tabs.map(\.path), ["notes/a.txt", "notes/sub/b.txt", "docsx/c.txt"])
        XCTAssertEqual(front?.path, "notes/sub/b.txt")
        XCTAssertTrue(book.isDirty(.init(projectID: "p", path: "notes/sub/b.txt", kind: .file)))
    }

    @MainActor
    func testRenamingAnUnopenedFileLeavesTabsAlone() {
        let book = book(tabs: ["a.txt"], front: "a.txt")
        XCTAssertNil(book.followEdit(projectID: "p", edit: .rename(path: "b.txt", newName: "c.txt")))
        XCTAssertEqual(book.scene("p").tabs.map(\.path), ["a.txt"])
    }

    /// Moving a folder to the Trash closes every tab inside it.
    @MainActor
    func testTrashClosesTheTabsInside() {
        let book = book(tabs: ["keep.txt", "old/a.txt", "old/b.txt"], front: "old/b.txt")
        let front = book.followEdit(projectID: "p", edit: .trash(path: "old"))
        XCTAssertEqual(book.scene("p").tabs.map(\.path), ["keep.txt"])
        XCTAssertEqual(front?.path, "keep.txt")
    }

    func testLinkedTreePathsMapToTheirFolder() {
        let node = LinkedFolderPath.node(folder: "/Users/me/ref", relative: "src/a.txt")
        let host = ProjectResourceBook.hostRequest(projectID: "p", path: node)
        XCTAssertEqual(host.projectID, "p#linked:/Users/me/ref")
        XCTAssertEqual(host.path, "src/a.txt")
        XCTAssertEqual(LinkedFolderPath.absolute(node), "/Users/me/ref/src/a.txt")
        XCTAssertEqual(LinkedFolderPath.absolute(LinkedFolderPath.node(folder: "/Users/me/ref")), "/Users/me/ref")
        XCTAssertNil(LinkedFolderPath.absolute("src/a.txt"))
        XCTAssertEqual(ProjectResourceBook.hostRequest(projectID: "p", path: "src/a.txt").projectID, "p")
    }

    @MainActor
    func testLinkedFoldersFollowTheProjectsOwnEntries() {
        let entries = ProjectResourceBook.linkedEntries([
            LinkedFolder(path: "/Users/me/ref", name: "ref", exists: true),
            LinkedFolder(path: "/Users/me/gone", name: "gone", exists: false),
        ])
        XCTAssertEqual(entries.map(\.name), ["ref", "gone · 找不到"])
        XCTAssertEqual(entries.map(\.isDir), [true, false])
        XCTAssertFalse(FileEditText.writable(entries[0].path))
    }
}
