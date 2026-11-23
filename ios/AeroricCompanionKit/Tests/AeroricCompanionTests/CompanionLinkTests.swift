@testable import AeroricMobileTransport
import Foundation
import XCTest

/// A Mac on the other end of a fake socket, answering the way
/// `core/.../remote/server.rs` does: plaintext `hello` → `hello_ack`, then
/// encrypted frames; `auth` in v2, everything after in v3.
private final class FakeMac: CompanionSocket, @unchecked Sendable {
    struct Config {
        var knownToken = "token-1"
        var hostEpoch = "epoch-1"
        var latestSeq: UInt64 = 0
        var projects: [[String: Any]] = [["id": "p1", "name": "Demo", "target": ["kind": "local", "path": "/demo"]]]
        var tasks: [String: [[String: Any]]] = [:]
        /// What `task.respond` answers; nil leaves it unanswered.
        var respondResult: [String: Any]? = ["outcome": "resolved"]
        /// Pushed while `projects.list` is being answered.
        var pushDuringSnapshot: [UInt64] = []
    }

    private let lock = NSLock()
    private let responder: ServerHandshakeResponder
    private var config: Config
    private var session: E2eeSession?
    private var queue: [CompanionSocketMessage] = []
    private var waiter: CheckedContinuation<CompanionSocketMessage, Error>?
    private var results: [String: [String: Any]] = [:]
    private(set) var commands: [(method: String, commandId: String)] = []
    private(set) var executed: [String] = []
    private(set) var closedByPhone = false

    init(responder: ServerHandshakeResponder, config: Config = Config()) {
        self.responder = responder
        self.config = config
    }

    func setRespondResult(_ result: [String: Any]?) {
        lock.lock(); config.respondResult = result; lock.unlock()
    }

    var receivedCommands: [(method: String, commandId: String)] {
        lock.lock(); defer { lock.unlock() }
        return commands
    }

    var executions: [String] {
        lock.lock(); defer { lock.unlock() }
        return executed
    }

    // MARK: CompanionSocket

    func send(text: String) async throws {
        deliver(.text(try acceptHello(text)))
    }

    func send(binary: Data) async throws {
        for reply in answer(binary) { deliver(reply) }
    }

    private func acceptHello(_ text: String) throws -> String {
        let (ack, session) = try responder.respond(helloJSON: text)
        lock.withLock { self.session = session }
        return ack
    }

    private func answer(_ binary: Data) -> [CompanionSocketMessage] {
        lock.withLock {
            guard let session, let decoded = try? session.decryptFrame(binary) else {
                XCTFail("the phone sent a frame the Mac cannot open")
                return []
            }
            let request = (try? JSONSerialization.jsonObject(with: decoded.plaintext)) as? [String: Any] ?? [:]
            return handle(request)
        }
    }

    func receive() async throws -> CompanionSocketMessage {
        try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if !queue.isEmpty {
                let message = queue.removeFirst()
                lock.unlock()
                continuation.resume(returning: message)
            } else {
                waiter = continuation
                lock.unlock()
            }
        }
    }

    func close() async {
        lock.withLock { closedByPhone = true }
        deliver(.closed(reason: nil))
    }

    // MARK: The Mac's side

    func push(seq: UInt64) {
        lock.lock()
        let frame = encrypted(["v": 3, "type": "push", "event": "task-status", "seq": seq, "data": [:]])
        lock.unlock()
        deliver(frame)
    }

    func closeFromMac(reason: String?) {
        deliver(.closed(reason: reason))
    }

    private func deliver(_ message: CompanionSocketMessage) {
        lock.lock()
        if let waiter {
            self.waiter = nil
            lock.unlock()
            waiter.resume(returning: message)
        } else {
            queue.append(message)
            lock.unlock()
        }
    }

    /// Called with the lock held.
    private func handle(_ request: [String: Any]) -> [CompanionSocketMessage] {
        let method = request["method"] as? String ?? ""
        let params = request["params"] as? [String: Any] ?? [:]
        if method == "auth" {
            let id = request["id"] as? Int ?? 0
            guard (request["v"] as? Int) == 2 else { return [encrypted(["v": 2, "id": id, "ok": false, "error": "auth must be v2"])] }
            if params["deviceToken"] as? String == config.knownToken {
                let v3 = (params["supportedRpcVersions"] as? [Int])?.contains(3) == true
                return [encrypted(["v": 2, "id": id, "ok": true,
                                   "result": ["deviceId": "dev-1", "rpcVersion": v3 ? 3 : 2]])]
            }
            return [encrypted(["v": 2, "id": id, "ok": false, "error": "Unknown device token"])]
        }
        let id = request["id"] as? String ?? ""
        XCTAssertEqual(request["type"] as? String, "request", "after auth the phone speaks v3")
        func ok(_ result: Any) -> CompanionSocketMessage {
            encrypted(["v": 3, "type": "response", "id": id, "ok": true, "result": result])
        }
        switch method {
        case "hello":
            return [ok(["name": "aeroric", "hostEpoch": config.hostEpoch])]
        case "events.since":
            return [ok(["events": [], "latestSeq": config.latestSeq, "reset": false])]
        case "projects.list":
            let pushes = config.pushDuringSnapshot.map {
                encrypted(["v": 3, "type": "push", "event": "task-status", "seq": $0, "data": [:]])
            }
            return pushes + [ok(config.projects)]
        case "tasks.list":
            return [ok(config.tasks[params["projectId"] as? String ?? ""] ?? [])]
        case "task.respond", "task.create":
            let commandId = params["commandId"] as? String ?? ""
            commands.append((method, commandId))
            if params["hostEpoch"] as? String != config.hostEpoch {
                return [encrypted(["v": 3, "type": "response", "id": id, "ok": false,
                                   "error": ["code": "remote_error", "message": "STALE_HOST_EPOCH: restarted"]])]
            }
            // D4: the same commandId runs once and repeats its first result.
            if let first = results[commandId] { return [ok(first)] }
            let result: [String: Any]
            if method == "task.respond" {
                guard let answer = config.respondResult else { return [] }
                result = answer
            } else {
                result = ["taskId": "t-new"]
            }
            executed.append(commandId)
            results[commandId] = result
            return [ok(result)]
        default:
            return [encrypted(["v": 3, "type": "response", "id": id, "ok": false,
                               "error": ["code": "remote_error", "message": "Unknown method \(method)"]])]
        }
    }

    private func encrypted(_ object: [String: Any]) -> CompanionSocketMessage {
        let data = try! JSONSerialization.data(withJSONObject: object)
        return .binary(try! session!.encryptFrame(kind: .control, plaintext: data))
    }
}

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
