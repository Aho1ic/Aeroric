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
        socket.push(seq: 9)
        await eventually("seq 9 arrives") { store.lastEventSeq == 9 }
        XCTAssertTrue(store.resyncNeeded, "seq 8 was missed")
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
}
