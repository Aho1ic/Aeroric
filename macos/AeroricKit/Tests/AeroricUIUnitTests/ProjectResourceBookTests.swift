import AeroricClient
import XCTest
@testable import AeroricUI

/// In-memory, controllable fake reader. No windows, files, agents or API/UDS.
private actor ResourceReaderFixture: ProjectResourceReading {
    enum Failure: Error { case denied }
    private var pending: [String: [CheckedContinuation<String, Error>]] = [:]
    private let suspendLists: Bool
    private let suspendBranches: Bool
    private(set) var branchCalls = 0
    init(suspendLists: Bool = false, suspendBranches: Bool = true) {
        self.suspendLists = suspendLists; self.suspendBranches = suspendBranches
    }
    var count: Int { pending.values.reduce(0) { $0 + $1.count } }
    func directory(projectID: String, path: String) async throws -> [FilesClient.FileEntry] {
        if suspendLists { _ = try await text(projectID: projectID, path: "directory:\(path)") }
        return []
    }
    func changes(projectPath: String) async throws -> [ResourceGitChange] {
        if suspendLists { _ = try await text(projectID: projectPath, path: "changes") }
        return []
    }
    func branches(projectPath: String) async throws -> [ResourceGitBranch] {
        branchCalls += 1
        if suspendLists && suspendBranches { _ = try await text(projectID: projectPath, path: "branches") }
        return [.init(name: "fixture", current: true)]
    }
    func diff(projectPath: String, path: String, staged: Bool) async throws -> String {
        try await text(projectID: projectPath, path: path)
    }
    func text(projectID: String, path: String) async throws -> String {
        try await withCheckedThrowingContinuation { pending[path, default: []].append($0) }
    }
    func complete(_ path: String, index: Int = 0, result: Result<String, Error>) {
        pending[path]![index].resume(with: result)
    }
}

@MainActor final class ProjectResourceBookTests: XCTestCase {
    private let a = ResourcePreviewTarget(projectID: "a", path: "one.swift", kind: .file)
    private let b = ResourcePreviewTarget(projectID: "a", path: "two.swift", kind: .file)

    private func waitFor(_ count: Int, reader: ResourceReaderFixture) async {
        for _ in 0..<10000 {
            if await reader.count >= count { return }
            await Task.yield()
        }
        XCTFail("fixture request did not arrive")
    }
    func testWindowAndProjectScenesDoNotOverwriteEachOther() {
        let first = ProjectResourceBook(), second = ProjectResourceBook()
        first.select(a)
        first.edit("a") { $0.expandedPaths.insert("src"); $0.section = .git }
        first.select(.init(projectID: "b", path: "readme.md", kind: .file))
        XCTAssertEqual(first.scene("a").target, a)
        XCTAssertEqual(first.scene("a").section, .git)
        XCTAssertTrue(first.scene("a").expandedPaths.contains("src"))
        XCTAssertNil(second.scene("a").target)
        first.showTask(projectID: "a")
        XCTAssertFalse(first.scene("a").previewActive)
        XCTAssertEqual(first.scene("a").target, a, "return does not create another tab or erase the target")
    }
    func testLateOldPreviewCannotReplaceNewTarget() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        book.select(a)
        let old = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        book.select(b)
        let current = Task { await book.loadPreview(b, projectPath: "/fixture/a", reader: reader) }
        await waitFor(2, reader: reader)
        await reader.complete(b.path, result: .success("new\n<script>literal</script>"))
        await current.value
        await reader.complete(a.path, result: .success("old"))
        await old.value
        XCTAssertEqual(book.scene("a").target, b)
        XCTAssertEqual(book.scene("a").previewText, "new\n<script>literal</script>")
        XCTAssertEqual(book.scene("a").previewState, .loaded)
        XCTAssertEqual(book.scene("a").previewLines.map(\.text).joined(separator: "\n"), book.scene("a").previewText)
    }
    func testRetryGenerationRejectsOldFailureAndKeepsCacheOnNewFailure() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        book.select(a)
        let old = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        let new = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(2, reader: reader)
        await reader.complete(a.path, index: 1, result: .success("verified fixture"))
        await new.value
        await reader.complete(a.path, result: .failure(ResourceReaderFixture.Failure.denied))
        await old.value
        XCTAssertEqual(book.scene("a").previewState, .loaded)
        let failedRefresh = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(3, reader: reader)
        await reader.complete(a.path, index: 2, result: .failure(ResourceReaderFixture.Failure.denied))
        await failedRefresh.value
        XCTAssertEqual(book.scene("a").previewText, "verified fixture")
        guard case .failed = book.scene("a").previewState else { return XCTFail("not a successful empty result") }
    }
    func testReconnectInvalidatesInFlightReadWithoutDiscardingIdentity() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        book.select(a)
        let pending = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        book.invalidateSnapshots()
        await reader.complete(a.path, result: .success("stale"))
        await pending.value
        XCTAssertEqual(book.scene("a").target, a)
        XCTAssertNil(book.scene("a").previewText)
        guard case .cached = book.scene("a").previewState else { return XCTFail("must not revive a stale read") }
    }
    func testSuccessfulEmptyAndUnavailableRemainDistinct() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        XCTAssertNil(book.scene("a").directoryStates[""])
        await book.loadDirectory(projectID: "a", path: "", reader: nil)
        XCTAssertNil(book.scene("a").directories[""])
        guard case .unavailable = book.scene("a").directoryStates[""] else { return XCTFail() }
        await book.loadDirectory(projectID: "a", path: "", reader: reader)
        XCTAssertEqual(book.scene("a").directoryStates[""], .loaded)
        XCTAssertEqual(book.scene("a").directories[""], [])
        await book.loadGit(projectID: "a", projectPath: "/fixture/a", reader: reader)
        XCTAssertEqual(book.scene("a").gitState, .loaded)
        XCTAssertTrue(book.scene("a").changes.isEmpty)
        XCTAssertEqual(book.scene("a").branches.first?.name, "fixture")
    }
    func testCancelledDirectoryDoesNotPublishLateSuccessOrBlockRetry() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture(suspendLists: true)
        let pending = Task { await book.loadDirectory(projectID: "a", path: "", reader: reader) }
        await waitFor(1, reader: reader)
        pending.cancel()
        await reader.complete("directory:", result: .success("late"))
        await pending.value
        XCTAssertNil(book.scene("a").directories[""], "cancelled work must not publish a successful empty snapshot")
        XCTAssertNotEqual(book.scene("a").directoryStates[""], .loaded)
        if case .failed = book.scene("a").directoryStates[""] { XCTFail("navigation cancellation is not a read failure") }

        let retry = Task { await book.loadDirectory(projectID: "a", path: "", reader: reader) }
        await waitFor(2, reader: reader)
        await reader.complete("directory:", index: 1, result: .success(""))
        await retry.value
        XCTAssertEqual(book.scene("a").directoryStates[""], .loaded)
        XCTAssertEqual(book.scene("a").directories[""], [])

        let cancelledRefresh = Task { await book.loadDirectory(projectID: "a", path: "", reader: reader) }
        await waitFor(3, reader: reader)
        await reader.complete("directory:", index: 2, result: .failure(ConnectionError.cancelled))
        await cancelledRefresh.value
        XCTAssertEqual(book.scene("a").directories[""], [], "a cancelled refresh retains even an empty prior snapshot")
        if case .failed = book.scene("a").directoryStates[""] { XCTFail("transport cancellation is not a read failure") }
    }
    func testCancelledGitStopsBeforeDispatchingBranches() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture(suspendLists: true, suspendBranches: false)
        let pending = Task { await book.loadGit(projectID: "a", projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        pending.cancel()
        await reader.complete("changes", result: .failure(ConnectionError.cancelled))
        // An already-cancelled operation must not start its second RPC.
        await pending.value
        let branchCalls = await reader.branchCalls
        XCTAssertEqual(branchCalls, 0)
        XCTAssertNotEqual(book.scene("a").gitState, .loaded)
        if case .failed = book.scene("a").gitState { XCTFail("cancelled status must remain retryable") }
        if case .failed = book.scene("a").branchState { XCTFail("an unrequested branch is not a read failure") }
    }
    func testNewDirectoryReadCanSupersedeCancelledTaskBeforeItsReaderResumes() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture(suspendLists: true)
        let old = Task { await book.loadDirectory(projectID: "a", path: "", reader: reader) }
        await waitFor(1, reader: reader)
        old.cancel()
        XCTAssertEqual(book.scene("a").directoryStates[""], .loading)
        XCTAssertTrue(book.scene("a").directoryStates[""]?.needsAutomaticLoad == true)
        let current = Task { await book.loadDirectory(projectID: "a", path: "", reader: reader) }
        await waitFor(2, reader: reader)
        await reader.complete("directory:", index: 1, result: .success(""))
        await current.value
        await reader.complete("directory:", result: .failure(CancellationError()))
        await old.value
        XCTAssertEqual(book.scene("a").directoryStates[""], .loaded)
        XCTAssertEqual(book.scene("a").directories[""], [])
    }
    func testBranchCancellationKeepsCompletedStatusAndAllowsAutomaticRetry() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture(suspendLists: true)
        let pending = Task { await book.loadGit(projectID: "a", projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        await reader.complete("changes", result: .success(""))
        await waitFor(2, reader: reader)
        pending.cancel()
        await reader.complete("branches", result: .success("late"))
        await pending.value
        XCTAssertEqual(book.scene("a").gitState, .loaded)
        XCTAssertTrue(book.scene("a").changes.isEmpty)
        XCTAssertEqual(book.scene("a").branchState, .cancelled)
        XCTAssertTrue(book.scene("a").branchState.needsAutomaticLoad)
        XCTAssertTrue(book.scene("a").branches.isEmpty, "do not publish the cancelled branch reply")
    }
    func testCancelledPreviewKeepsPriorSourceInsteadOfPublishingLateReply() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        book.select(a)
        let initial = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        await reader.complete(a.path, result: .success("original"))
        await initial.value
        let refresh = Task { await book.loadPreview(a, projectPath: "/fixture/a", reader: reader) }
        await waitFor(2, reader: reader)
        refresh.cancel()
        await reader.complete(a.path, index: 1, result: .success("cancelled"))
        await refresh.value
        XCTAssertEqual(book.scene("a").previewText, "original")
        XCTAssertNotEqual(book.scene("a").previewState, .loaded)
        if case .failed = book.scene("a").previewState { XCTFail("cancelled refresh is not a read failure") }
    }
    func testAutomaticLoadCanReplaceCancelledOrStillSuspendedTaskButNotRealFailure() {
        for status: ResourceReadStatus in [.notLoaded, .loading, .cancelled] {
            XCTAssertTrue(status.needsAutomaticLoad)
        }
        for status: ResourceReadStatus in [
            .loaded, .failed("denied"), .cached("reconnect"), .unavailable("no host"), .unsupported("binary"),
        ] {
            XCTAssertFalse(status.needsAutomaticLoad, "a layout change must not retry failures or replace explicit refresh")
        }
    }
    func testUnsupportedPreviewDoesNotInvokeFileOrGitFallback() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        for kind: ResourcePreviewKind in [.file] {
            let target = ResourcePreviewTarget(projectID: "a", path: "photo.png", kind: kind)
            book.select(target)
            await book.loadPreview(target, projectPath: "/fixture/a", reader: reader)
            guard case .unsupported = book.scene("a").previewState else { return XCTFail() }
            XCTAssertNil(book.scene("a").previewText)
        }
        let count = await reader.count
        XCTAssertEqual(count, 0)
    }
    func testOnlyBoundedLightweightPositionsSurviveTargetReplacement() {
        let book = ProjectResourceBook()
        for index in 0..<40 {
            let target = ResourcePreviewTarget(projectID: "a", path: "\(index)", kind: .file)
            book.select(target)
            book.rememberPosition(index, for: target)
        }
        XCTAssertEqual(book.scene("a").positions.count, 32)
        XCTAssertEqual(book.scene("a").target?.path, "39")
        XCTAssertTrue(book.scene("a").previewLines.isEmpty)
    }
    func testUnifiedDiffPreservesEveryLineWithBothCounters() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -4,2 +4,2 @@\n keep\n-old\n+new\n\\ No newline at end of file"
        let rows = ResourcePreviewLine.lines(diff, diff: true)
        XCTAssertEqual(rows.map(\.text).joined(separator: "\n"), diff)
        XCTAssertEqual(rows[4].oldNumber, 4)
        XCTAssertEqual(rows[4].newNumber, 4)
        XCTAssertEqual(rows[5].oldNumber, 5)
        XCTAssertNil(rows[5].newNumber)
        XCTAssertEqual(rows[6].newNumber, 5)
        XCTAssertNil(rows[6].oldNumber)
        XCTAssertEqual(rows[7].kind, .metadata)
    }
    func testUntrackedPreviewLoadsDiffInsteadOfUnsupported() async {
        let book = ProjectResourceBook(), reader = ResourceReaderFixture()
        let target = ResourcePreviewTarget(projectID: "a", path: "new.txt", kind: .untracked)
        book.select(target)
        let load = Task { await book.loadPreview(target, projectPath: "/fixture/a", reader: reader) }
        await waitFor(1, reader: reader)
        await reader.complete(target.path, result: .success("diff --git a/new.txt b/new.txt\n+hello\n"))
        await load.value
        XCTAssertEqual(book.scene("a").previewState, .loaded)
        XCTAssertEqual(book.scene("a").previewText, "diff --git a/new.txt b/new.txt\n+hello\n")
    }
}
