@testable import AeroricMobileTransport
import Foundation
import XCTest

/// D4 step 4 (2026-11-23): `CompanionLink` against a fake Mac.
final class CompanionLinkTests: XCTestCase {
    private let mac = ServerHandshakeResponder()

    @MainActor
    private func pairedStore(epoch: String = "epoch-1") throws -> CompanionStore {
        let store = CompanionStore()
        try store.pairWithMac(hostName: "Mac", staticPubB64: mac.publicBase64Url,
                              initialEpoch: epoch, mode: .directLAN, deviceID: "dev-1")
        return store
    }

    private func taskRecord(_ id: String, approval: String? = nil) -> [String: Any] {
        var record: [String: Any] = ["id": id, "projectId": "p1", "name": "修 bug", "agent": "codex",
                                     "status": "input_required", "permissionMode": "ask"]
        if let approval { record["approval"] = ["requestId": approval, "kind": "command", "toolName": "Bash"] }
        return record
    }

    @MainActor
    private func eventually(_ what: String, _ condition: @MainActor () -> Bool) async {
        for _ in 0..<300 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }

    @MainActor
    func testConnectingReadsTheMacIntoTheStoreAndSendsWhatWaited() async throws {
        let store = try pairedStore()
        _ = try store.createTask(projectId: "p1", agent: "codex", prompt: "hi")
        let waiting = store.outbox[0].commandId
        store.markMacOffline(relayStillConnected: false)

        var config = FakeMac.Config()
        config.tasks = ["p1": [taskRecord("t1", approval: "r1")]]
        config.latestSeq = 4
        let socket = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)

        XCTAssertTrue(link.isConnected)
        XCTAssertEqual(store.connectionState, .macOnline(hostEpoch: "epoch-1", transportMode: .directLAN))
        XCTAssertEqual(store.projects, [CompanionProject(id: "p1", name: "Demo", path: "/demo")])
        XCTAssertEqual(store.tasks.map(\.id), ["t1"])
        XCTAssertEqual(store.tasks.first?.title, "修 bug")
        XCTAssertEqual(store.pendingApprovals.map(\.requestId), ["r1"])
        XCTAssertEqual(store.pendingApprovals.first?.summary, "Bash")
        XCTAssertEqual(store.lastEventSeq, 4)
        // Same host start: the command goes again with its id, and its answer empties the outbox.
        XCTAssertEqual(socket.receivedCommands.map(\.commandId), [waiting])
        await eventually("the outbox empties") { store.outbox.isEmpty }
    }

    @MainActor
    func testPushesDuringTheSnapshotAreKeptAndLaterGapsAskForResync() async throws {
        let store = try pairedStore()
        var config = FakeMac.Config()
        config.latestSeq = 5
        config.pushDuringSnapshot = [6]
        let socket = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .selfHostedRelay)

        XCTAssertEqual(store.lastEventSeq, 6, "the push that came while reading is not lost")
        XCTAssertFalse(store.resyncNeeded)
        socket.push(seq: 7)
        await eventually("seq 7 arrives") { store.lastEventSeq == 7 }
        XCTAssertFalse(store.resyncNeeded)
        // Seq 8 was missed: the phone reads the Mac again (D4 step 6).
        let snapshots = store.snapshotResyncCount
        socket.push(seq: 9)
        await eventually("read again after the gap") { store.snapshotResyncCount == snapshots + 1 }
        XCTAssertEqual(store.lastEventSeq, 9)
        XCTAssertFalse(store.resyncNeeded)
    }

    @MainActor
    func testAnApprovalFromThePhoneComesBackSettled() async throws {
        let store = try pairedStore()
        var config = FakeMac.Config()
        config.tasks = ["p1": [taskRecord("t1", approval: "r1")]]
        let socket = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)

        try store.submitApproval(requestId: "r1", approve: true)
        try await link.flush()
        await eventually("the card leaves") { store.pendingApprovals.isEmpty }
        XCTAssertTrue(store.outbox.isEmpty)
        XCTAssertEqual(socket.receivedCommands.map(\.method), ["task.respond"])
        try await link.flush()
        XCTAssertEqual(socket.receivedCommands.count, 1, "nothing is sent twice on one connection")
    }

    @MainActor
    func testADropBeforeTheAnswerIsUnknownAndTheSameIdGoesAgainOnce() async throws {
        let store = try pairedStore()
        var config = FakeMac.Config()
        config.tasks = ["p1": [taskRecord("t1", approval: "r1")]]
        config.respondResult = nil
        let first = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: first, deviceToken: "token-1", mode: .directLAN)

        try store.submitApproval(requestId: "r1", approve: false)
        try await link.flush()
        await eventually("the Mac got it") { first.receivedCommands.count == 1 }
        let commandId = first.receivedCommands[0].commandId
        first.closeFromMac(reason: nil)
        await eventually("offline") { !store.connectionState.isMacOnline }
        XCTAssertFalse(link.isConnected)
        XCTAssertEqual(store.pendingApprovals.first?.state,
                       .unknown(reason: "没等到 Mac 的回复，不知道送到没有"))
        XCTAssertEqual(store.outbox.map(\.commandId), [commandId], "kept for the same host start")

        config.respondResult = ["outcome": "resolved"]
        let second = FakeMac(responder: mac, config: config)
        try await link.connect(socket: second, deviceToken: "token-1", mode: .directLAN)
        XCTAssertEqual(second.receivedCommands.map(\.commandId), [commandId])
        await eventually("settled") { store.pendingApprovals.isEmpty && store.outbox.isEmpty }
    }

    @MainActor
    func testANewHostStartDropsWhatWasDecidedBefore() async throws {
        let store = try pairedStore()
        _ = try store.createTask(projectId: "p1", agent: "codex", prompt: "hi")
        store.markMacOffline(relayStillConnected: false)
        var config = FakeMac.Config()
        config.hostEpoch = "epoch-2"
        let socket = FakeMac(responder: mac, config: config)
        try await CompanionLink(store: store).connect(socket: socket, deviceToken: "token-1", mode: .directLAN)

        XCTAssertTrue(store.outbox.isEmpty)
        XCTAssertEqual(store.droppedCommands.map(\.method), ["task.create"])
        XCTAssertTrue(socket.receivedCommands.isEmpty, "never sent to the restarted Mac")
    }

    @MainActor
    func testAnUnknownTokenRevokesTheDevice() async throws {
        let store = try pairedStore()
        let socket = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        do {
            try await link.connect(socket: socket, deviceToken: "stolen", mode: .directLAN)
            XCTFail("connected with a token the Mac does not know")
        } catch let error as CompanionLinkError {
            XCTAssertEqual(error, .authRejected("Unknown device token"))
        }
        guard case .revoked = store.connectionState else { return XCTFail("\(store.connectionState)") }
        XCTAssertNil(store.pairedStaticPubB64)
        XCTAssertTrue(socket.closedByPhone)
    }

    @MainActor
    func testAMacWithAnotherKeyIsNotTrusted() async throws {
        let store = try pairedStore()
        let impostor = FakeMac(responder: ServerHandshakeResponder())
        let link = CompanionLink(store: store)
        do {
            try await link.connect(socket: impostor, deviceToken: "token-1", mode: .directLAN)
            XCTFail("trusted a Mac whose key differs from pairing")
        } catch let error as CompanionLinkError {
            XCTAssertEqual(error, .handshake("Mac 的身份对不上配对时的公钥"))
        }
        XCTAssertEqual(store.connectionState, .disconnected(reason: "Mac 的身份对不上配对时的公钥"))
        XCTAssertTrue(impostor.receivedCommands.isEmpty)
    }

    @MainActor
    func testTheMacSigningOutSaysSo() async throws {
        let store = try pairedStore()
        let socket = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)
        socket.closeFromMac(reason: "signed-out")
        await eventually("offline") { !store.connectionState.isMacOnline }
        XCTAssertEqual(store.connectionState, .disconnected(reason: "电脑端已退出远程连接"))
    }

    /// 1126a: the Mac rejecting a frame says so instead of "连接断开了".
    @MainActor
    func testAFrameTheMacRejectsSaysSo() async throws {
        let store = try pairedStore()
        let socket = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)
        socket.closeFromMac(reason: "frame-rejected")
        await eventually("offline") { !store.connectionState.isMacOnline }
        XCTAssertEqual(store.connectionState,
                       .disconnected(reason: "Mac 拒收了这台手机发来的一条消息（顺序不对或已损坏），已断开"))
    }

    /// 1126a: requests sent at once reach the Mac in the order of their
    /// sequence numbers. The socket holds every other frame back a little, as
    /// the hop off the main actor could; before the fix the next request,
    /// already numbered, overtook it and the Mac closed the connection.
    @MainActor
    func testRequestsSentAtOnceReachTheMacInOrder() async throws {
        let store = try pairedStore()
        let fake = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        try await link.connect(socket: StallingSocket(fake), deviceToken: "token-1", mode: .directLAN)
        var answered = 0
        for _ in 0..<12 {
            Task { @MainActor in
                if (try? await link.call("hello")) != nil { answered += 1 }
            }
        }
        await eventually("every request answered") { answered == 12 }
        XCTAssertTrue(link.isConnected)
        XCTAssertTrue(store.connectionState.isMacOnline)
    }

    /// D4 step 6 (2026-11-23): an answer names the run it was decided on; a
    /// rerun on the Mac refuses it, and the phone reads the Mac again at once.
    @MainActor
    func testAnAnswerToAnEarlierRunIsRefusedAndThePhoneResyncs() async throws {
        let store = try pairedStore()
        func record(_ generation: String, _ approval: String) -> [String: Any] {
            ["id": "t1", "projectId": "p1", "name": "修 bug", "agent": "codex", "status": "input_required",
             "generation": generation, "approval": ["requestId": approval, "kind": "command", "toolName": "Bash"]]
        }
        var config = FakeMac.Config()
        config.tasks = ["p1": [record("3", "r1")]]
        let socket = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)
        XCTAssertEqual(store.tasks.first?.generation, "3")
        let snapshots = store.snapshotResyncCount

        // The task is stopped and run again on the Mac; the phone still shows r1.
        socket.setTasks(["p1": [record("4", "r2")]])
        try store.submitApproval(requestId: "r1", approve: true)
        try await link.flush()
        XCTAssertEqual(socket.receivedParams.first?["basedOnGeneration"], "3")
        XCTAssertEqual(socket.receivedParams.first?["projectId"], "p1")

        await eventually("read again") { store.snapshotResyncCount == snapshots + 1 }
        XCTAssertEqual(store.pendingApprovals.map(\.requestId), ["r2"])
        XCTAssertEqual(store.tasks.first?.generation, "4")
        XCTAssertFalse(store.resyncNeeded)
        XCTAssertTrue(store.outbox.isEmpty)
        XCTAssertTrue(link.isConnected, "on the same connection")
    }

    /// D4 step 6: a gap in the pushes is read again on the spot too.
    @MainActor
    func testAGapInThePushesIsReadAgainOnTheSameConnection() async throws {
        let store = try pairedStore()
        let socket = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)
        let snapshots = store.snapshotResyncCount
        socket.push(seq: 3)
        await eventually("read again") { store.snapshotResyncCount == snapshots + 1 }
        XCTAssertFalse(store.resyncNeeded)
        XCTAssertEqual(store.lastEventSeq, 3)
    }

    /// D4 step 8 (2026-11-23): a live session's open requests come in the
    /// snapshot, and a `task-changed` push makes the phone read again.
    @MainActor
    func testALiveSessionsRequestsShowAndATaskPushReadsAgain() async throws {
        let store = try pairedStore()
        var config = FakeMac.Config()
        config.tasks = ["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex", "status": "running"]]]
        let socket = FakeMac(responder: mac, config: config)
        let link = CompanionLink(store: store)
        try await link.connect(socket: socket, deviceToken: "token-1", mode: .directLAN)
        XCTAssertTrue(store.pendingApprovals.isEmpty)

        // The agent asks; the Mac pushes `task-changed`.
        socket.setTasks(["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex",
                                 "status": "input_required",
                                 "pendingApprovals": [
                                     ["requestId": "r1", "kind": "command", "summary": "cargo test"],
                                     ["requestId": "r2", "kind": "fileChange", "summary": "",
                                      "subagent": "reviewer"],
                                 ]]]])
        socket.push(seq: 1, event: "task-changed")
        await eventually("read again") { store.pendingApprovals.count == 2 }
        XCTAssertEqual(store.pendingApprovals.map(\.summary), ["cargo test", "reviewer · fileChange"])
        XCTAssertEqual(store.tasks.first?.status, "input_required")

        // Answered on the Mac: the next push takes the card away.
        socket.setTasks(["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex", "status": "running"]]])
        socket.push(seq: 2, event: "task-status")
        await eventually("gone") { store.pendingApprovals.isEmpty }
    }
}

/// Forwards to a fake Mac, holding every other binary frame back for a
/// moment before it reaches the Mac.
private final class StallingSocket: CompanionSocket, @unchecked Sendable {
    private let inner: FakeMac
    private let lock = NSLock()
    private var frames = 0

    init(_ inner: FakeMac) { self.inner = inner }

    func send(text: String) async throws { try await inner.send(text: text) }

    func send(binary: Data) async throws {
        let stall = lock.withLock { () -> Bool in
            frames += 1
            return frames.isMultiple(of: 2)
        }
        if stall { try await Task.sleep(nanoseconds: 20_000_000) }
        try await inner.send(binary: binary)
    }

    func receive() async throws -> CompanionSocketMessage { try await inner.receive() }
    func close() async { await inner.close() }
}
