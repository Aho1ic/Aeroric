import XCTest
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

    func testFormTitles() {
        XCTAssertEqual(FileEditText.formTitle(.createFile(path: ""), parentName: ""), "在 项目根目录 里新建文件")
        XCTAssertEqual(FileEditText.formTitle(.createDirectory(path: "docs"), parentName: "docs"), "在 docs 里新建文件夹")
        XCTAssertEqual(FileEditText.formTitle(.rename(path: "docs/a.txt"), parentName: ""), "重命名「a.txt」")
    }

    @MainActor
    func testSavingOverAChangedFileNeedsADecision() {
        XCTAssertFalse(ProjectResourceBook.saveConflicts(disk: "same", base: "same"))
        XCTAssertTrue(ProjectResourceBook.saveConflicts(disk: "agent edit", base: "same"))
    }

    @MainActor
    func testExpandOpensAClosedFolderOnce() {
        let store = FileTreeStore()
        store.reset(projectID: "p")
        XCTAssertTrue(store.expand("docs"))
        XCTAssertFalse(store.expand("docs"))
        XCTAssertFalse(store.expand(""))
    }
}
