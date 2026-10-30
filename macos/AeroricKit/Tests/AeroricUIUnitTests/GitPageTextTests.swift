import XCTest
@testable import AeroricUI

final class GitPageTextTests: XCTestCase {
    func testTheCommitButtonSaysHowManyFilesGoIn() {
        XCTAssertEqual(GitPageText.commitTitle(stagedCount: 2), "提交 2 个文件")
        XCTAssertEqual(GitPageText.commitTitle(stagedCount: 0), "提交")
    }

    func testTabsCarryTheirCountOnlyWhenThereIsOne() {
        XCTAssertEqual(GitPageText.tabTitle("更改", count: 5), "更改 5")
        XCTAssertEqual(GitPageText.tabTitle("贮藏", count: 0), "贮藏")
    }

    func testStatusMarks() {
        XCTAssertEqual(GitPageText.mark("M").tone, .modified)
        XCTAssertEqual(GitPageText.mark("A").tone, .added)
        XCTAssertEqual(GitPageText.mark("D").tone, .deleted)
        XCTAssertEqual(GitPageText.mark("?").letter, "U")
        XCTAssertEqual(GitPageText.mark("?").tone, .untracked)
        XCTAssertEqual(GitPageText.mark("U").tone, .conflicted)
    }

    func testDiscardingAnUntrackedFileWarnsItIsDeleted() {
        XCTAssertTrue(GitPageText.discardMessage(path: "a.txt", untracked: true).contains("删掉"))
        XCTAssertTrue(GitPageText.discardMessage(path: "a.txt", untracked: false).contains("上次提交"))
    }

    func testPathsSplitIntoFolderAndName() {
        XCTAssertEqual(GitPageText.split("Sources/App/Main.swift").directory, "Sources/App/")
        XCTAssertEqual(GitPageText.split("Sources/App/Main.swift").name, "Main.swift")
        XCTAssertEqual(GitPageText.split("README.md").directory, "")
    }

    @MainActor
    func testAChangeIDGivesBackItsPath() {
        let change = GitStore.Change(path: "dir/a:b.txt", status: "M", staged: true)
        XCTAssertEqual(GitPageText.path(ofChangeID: change.id), "dir/a:b.txt")
    }
}
