import XCTest
@testable import AeroricUI

final class GitPushPullTextTests: XCTestCase {
    func testConfirmationsNameTheCountAndTheUpstream() {
        XCTAssertEqual(GitPageText.pushQuestion(count: 2, upstream: "origin/main"), "推送 2 个提交到 origin/main？")
        XCTAssertEqual(GitPageText.pullQuestion(count: 1, upstream: "origin/main"), "从 origin/main 拉取 1 个提交？")
        XCTAssertEqual(GitPageText.publishQuestion(branch: "feature"), "发布分支 feature？")
        XCTAssertTrue(GitPageText.publishDetail(branch: "feature").contains("origin"))
    }

    @MainActor
    func testAPushListsTheNewestCommitsItSends() {
        let commits = (1...4).map {
            GitStore.Commit(hash: "h\($0)", shortHash: "s\($0)", author: "a", date: "d", message: "m\($0)")
        }
        XCTAssertEqual(GitPageText.outgoing(commits, ahead: 2).map(\.shortHash), ["s1", "s2"])
        XCTAssertTrue(GitPageText.outgoing(commits, ahead: 0).isEmpty)
        XCTAssertEqual(GitPageText.outgoing(commits, ahead: 9).count, 4)
    }
}
