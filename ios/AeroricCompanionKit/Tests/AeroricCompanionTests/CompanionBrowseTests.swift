@testable import AeroricMobileTransport
import Foundation
import XCTest

/// D4 (2026-11-23): the 文件与 Git page reads a project's changes, files and
/// diffs from the Mac, read only, and reads them again when the Mac's state
/// changes.
final class CompanionBrowseTests: XCTestCase {
    private let mac = ServerHandshakeResponder()

    private func json(_ object: [String: Any]) -> String {
        String(decoding: try! JSONSerialization.data(withJSONObject: object), as: UTF8.self)
    }

    @MainActor
    private func eventually(_ what: String, _ condition: @MainActor () -> Bool) async {
        for _ in 0..<300 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }

    private let rootFiles: [String: Any] = ["available": true, "root": "/demo", "entries": [
        ["name": "Sources", "path": "/demo/Sources", "isDirectory": true, "size": 96],
        ["name": "README.md", "path": "/demo/README.md", "isDirectory": false, "size": 10],
    ]]

    // MARK: - Reading the Mac's answers

    func testEachReadNamesItsMethodAndParams() {
        XCTAssertEqual(CompanionRead.files(projectId: "p1", directory: nil).method, "project.files")
        XCTAssertNil(CompanionRead.files(projectId: "p1", directory: nil).params["path"])
        XCTAssertEqual(CompanionRead.files(projectId: "p1", directory: "/demo/a").params["path"] as? String, "/demo/a")
        XCTAssertEqual(CompanionRead.changes(projectId: "p1").method, "git.changes")
        let diff = CompanionRead.diff(projectId: "p1", path: "a.swift", staged: true)
        XCTAssertEqual(diff.method, "git.diff")
        XCTAssertEqual(diff.params["staged"] as? Bool, true)
        XCTAssertEqual(CompanionRead.file(projectId: "p1", path: "/demo/a").method, "project.readFile")
    }

    func testTheMacsAnswersBecomeEntriesChangesAndText() {
        XCTAssertEqual(CompanionBrowse.entries(json(rootFiles)).value?.map(\.name), ["Sources", "README.md"])
        XCTAssertEqual(CompanionBrowse.entries(json(rootFiles)).value?.first?.isDirectory, true)
        XCTAssertEqual(CompanionBrowse.changes(json(["available": true, "changes": [
            ["path": "a.swift", "status": "M", "staged": false],
            ["path": "b.swift", "status": "A", "staged": true],
        ]])).value, [
            CompanionGitChange(path: "a.swift", status: "M", staged: false),
            CompanionGitChange(path: "b.swift", status: "A", staged: true),
        ])
        XCTAssertEqual(CompanionBrowse.text(json(["available": true, "content": "x", "truncated": true]), key: "content"),
                       .loaded(CompanionDocumentText(text: "x", truncated: true)))
        XCTAssertEqual(CompanionBrowse.changes(json(["available": false, "reason": "ssh"])),
                       .unavailable("这个项目在远程主机（SSH）上，手机还看不了它的文件"))
        XCTAssertEqual(CompanionBrowse.entries("nope"), .failed("Mac 的回答读不懂"))
    }

    // MARK: - The store

    @MainActor
    func testThePageAsksForWhatItShowsAndIgnoresWhatItLeft() {
        let store = CompanionStore()
        var reads: [CompanionRead] = []
        store.onRead = { reads.append($0) }
        store.openBrowse(projectId: "p1")
        XCTAssertEqual(reads, [.changes(projectId: "p1"), .files(projectId: "p1", directory: nil)])
        store.applyRead(.files(projectId: "p1", directory: nil), answer: json(rootFiles))
        XCTAssertEqual(store.browse?.entries.value?.count, 2)

        store.openFolder("/demo/Sources")
        XCTAssertEqual(reads.last, .files(projectId: "p1", directory: "/demo/Sources"))
        XCTAssertEqual(store.browse?.entries, .loading)
        store.applyRead(.files(projectId: "p1", directory: nil), answer: json(rootFiles))
        XCTAssertEqual(store.browse?.entries, .loading, "the root's answer is not this folder's")
        store.leaveFolder()
        XCTAssertEqual(reads.last, .files(projectId: "p1", directory: nil))
        XCTAssertEqual(store.browse?.folders, [])

        store.openDiff(CompanionGitChange(path: "a.swift", status: "M", staged: false))
        XCTAssertEqual(reads.last, .diff(projectId: "p1", path: "a.swift", staged: false))
        store.applyRead(.diff(projectId: "p1", path: "a.swift", staged: true), answer: json(["available": true, "diff": "x"]))
        XCTAssertEqual(store.browse?.document?.content, .loading, "the staged diff is another document")
        store.applyRead(.diff(projectId: "p1", path: "a.swift", staged: false),
                        answer: json(["available": true, "diff": "+new"]))
        XCTAssertEqual(store.browse?.document?.content, .loaded(CompanionDocumentText(text: "+new")))
        store.readFailed(.diff(projectId: "p1", path: "a.swift", staged: false), message: "断了")
        XCTAssertEqual(store.browse?.document?.content.value?.text, "+new", "what is shown stays")
        store.readFailed(.changes(projectId: "p1"), message: "断了")
        XCTAssertEqual(store.browse?.changes, .failed("断了"))

        store.openFile(CompanionFileEntry(name: "Sources", path: "/demo/Sources", isDirectory: true))
        XCTAssertEqual(store.browse?.folders, ["/demo/Sources"], "a folder opens as a folder")
        store.closeDocument()
        XCTAssertNil(store.browse?.document)
        store.closeBrowse()
        store.applyRead(.changes(projectId: "p1"), answer: json(["available": true, "changes": []]))
        XCTAssertNil(store.browse)
    }

    @MainActor
    func testASnapshotRereadsThePageOrSaysTheProjectIsGone() {
        let store = CompanionStore()
        var reads: [CompanionRead] = []
        store.onRead = { reads.append($0) }
        store.applySnapshotProjects([CompanionProject(id: "p1", name: "Demo", path: "/demo")])
        store.openBrowse(projectId: "p1")
        store.openFile(CompanionFileEntry(name: "README.md", path: "/demo/README.md", isDirectory: false))
        reads = []
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 1, mode: .directLAN, snapshotTasks: [], snapshotApprovals: [])
        XCTAssertEqual(reads, [.changes(projectId: "p1"), .files(projectId: "p1", directory: nil),
                               .file(projectId: "p1", path: "/demo/README.md")])

        store.applySnapshotProjects([CompanionProject(id: "p2", name: "Other", path: "/other")])
        reads = []
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 2, mode: .directLAN, snapshotTasks: [], snapshotApprovals: [])
        XCTAssertEqual(reads, [])
        XCTAssertEqual(store.browse?.changes, .unavailable("这个项目在 Mac 上已经没有了"))
        XCTAssertNil(store.browse?.document)

        store.unpair()
        XCTAssertNil(store.browse)
    }

    // MARK: - Over the connection

    @MainActor
    func testChangesAndADiffAreReadAndReadAgainWhenTheMacSaysSomethingChanged() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: CompanionCredentials(
            hostName: "Studio Mac", hostId: "host-1", publicKey: mac.publicBase64Url,
            endpoints: ["ws://192.168.1.33:8765"], deviceId: "dev-1", deviceToken: "token-1"))
        var config = FakeMac.Config()
        config.reads = [
            "git.changes ": ["available": true, "changes": [["path": "a.swift", "status": "M", "staged": false]]],
            "project.files ": rootFiles,
            "git.diff a.swift": ["available": true, "diff": "@@ -1 +1 @@\n-old\n+new"],
        ]
        let fake = FakeMac(responder: mac, config: config)
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { _ in fake }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }

        store.openBrowse(projectId: "p1")
        await eventually("changes and files") {
            store.browse?.changes.value?.count == 1 && store.browse?.entries.value?.count == 2
        }
        store.openDiff(store.browse!.changes.value![0])
        await eventually("diff") { store.browse?.document?.content.value?.text.hasSuffix("+new") == true }

        store.openFile(CompanionFileEntry(name: "README.md", path: "/demo/README.md", isDirectory: false))
        await eventually("a refused read says why") {
            if case .failed = store.browse?.document?.content { true } else { false }
        }
        guard case let .failed(message) = store.browse?.document?.content else { return XCTFail() }
        XCTAssertTrue(message.contains("Path outside project"), message)
        store.closeDocument()

        fake.setRead("git.changes ", ["available": true, "changes": []])
        fake.push(seq: 1, event: "task-changed")
        await eventually("read again") { store.browse?.changes.value?.isEmpty == true }
        XCTAssertFalse(fake.receivedReads.contains { $0.hasPrefix("project.writeFile") }, "read only")
        await connector.stop()
    }
}
