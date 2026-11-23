import Foundation

/// D4 step 4 (2026-11-23): one frame off the wire, as the link sees it.
public enum CompanionSocketMessage: Equatable, Sendable {
    case text(String)
    case binary(Data)
    /// The other end closed; `reason` is the WebSocket close reason, if any.
    case closed(reason: String?)
}

/// What `CompanionLink` needs from the network: a WebSocket to the Mac's
/// listener or to the relay's client URL. The real one is
/// `URLSessionCompanionSocket`; tests pass a fake that plays the Mac.
public protocol CompanionSocket: AnyObject, Sendable {
    func send(text: String) async throws
    func send(binary: Data) async throws
    func receive() async throws -> CompanionSocketMessage
    func close() async
}

public enum CompanionLinkError: Error, Equatable, Sendable {
    /// No Mac public key on record: pair first.
    case notPaired
    /// The handshake failed (wrong key, the Mac refused, a malformed answer).
    case handshake(String)
    /// The Mac refused this device's token.
    case authRejected(String)
    /// The connection ended before the step finished.
    case closed(String)
    /// The Mac answered something this side cannot read.
    case unexpected(String)
    /// The Mac answered a read with an error.
    case remote(method: String, message: String)
}

/// D4 step 4 (2026-11-23): the phone's side of one connection to the Mac,
/// speaking what `core/.../remote/server.rs` speaks — the v2 handshake, a v2
/// `auth`, then v3 requests — and writing everything it learns into
/// `CompanionStore`, which stays the only state. See the note
/// `2026-11-23-d4-companion-link`.
@MainActor
public final class CompanionLink {
    public let store: CompanionStore
    public private(set) var isConnected = false

    private var socket: CompanionSocket?
    private var session: E2eeSession?
    private var version: RemoteRpcVersion = .v3
    /// Bumped per connection, so a loop or a reply from an old one is ignored.
    private var generation = 0
    private var nextRequest = 1
    private enum Waiter {
        case read(method: String, CheckedContinuation<String, Error>)
        case command(commandId: String, method: String, requestId: String?)
    }
    private var waiting: [String: Waiter] = [:]
    private var sentThisConnection: Set<String> = []
    /// Pushes that came while the snapshot was read: handed to the store
    /// after `handleReconnect`, which resets the watermark to `latestSeq`.
    private var syncing = false
    private var heldPushes: [UInt64] = []

    public init(store: CompanionStore) {
        self.store = store
    }

    /// Handshake, authenticate, read the Mac's state into the store, then
    /// send what is waiting. Throws if any step fails; the connection is
    /// closed and the store says why.
    public func connect(
        socket: CompanionSocket,
        deviceToken: String,
        mode: CompanionConnectionState.TransportMode
    ) async throws {
        if self.socket != nil { await disconnect() }
        guard let staticPub = store.pairedStaticPubB64 else { throw CompanionLinkError.notPaired }
        generation += 1
        let current = generation
        self.socket = socket
        sentThisConnection = []
        heldPushes = []
        nextRequest = 1
        do {
            // 1. The plaintext handshake, pinned to the key from pairing.
            let pending = try PendingClientHandshake.start(serverStaticPubBase64Url: staticPub)
            try await socket.send(text: pending.helloJSON)
            guard case let .text(ack) = try await socket.receive() else {
                throw CompanionLinkError.handshake("Mac 没有回握手")
            }
            do {
                session = try pending.finish(ackJSON: ack)
            } catch let error as E2eeError {
                throw CompanionLinkError.handshake(Self.describe(error))
            }

            // 2. `auth` is always v2; it says which RPC version follows.
            let auth = try RemoteRpcCodec.encodeRequest(
                version: .v2, id: "1", method: "auth",
                params: ["deviceToken": deviceToken, "supportedRpcVersions": [3]])
            try await sendControl(auth)
            switch try await receiveControlDirectly() {
            case let .responseSuccess(_, result):
                let reply = Self.object(result)
                version = (reply["rpcVersion"] as? Int) == 3 ? .v3 : .v2
            case let .responseFailure(_, _, message):
                if message == "Unknown device token" {
                    store.handleDeviceRevoked(reason: "Mac 不再认这台设备，需要重新配对")
                }
                throw CompanionLinkError.authRejected(message)
            default:
                throw CompanionLinkError.unexpected("认证没有得到回答")
            }
            nextRequest = 2

            // 3. From here the receive loop owns the socket; reads wait on it.
            syncing = true
            Task { [weak self] in await self?.receiveLoop(socket, generation: current) }

            let hello = Self.object(try await read("hello"))
            guard let epoch = hello["hostEpoch"] as? String, !epoch.isEmpty else {
                throw CompanionLinkError.unexpected("hello 没有 hostEpoch")
            }
            let since = Self.object(try await read("events.since", ["after": store.lastEventSeq]))
            let latestSeq = Self.uint(since["latestSeq"]) ?? store.lastEventSeq
            let (projects, tasks, approvals) = try await readSnapshot(epoch: epoch)

            // 4. The snapshot is authoritative; a new host start drops the outbox.
            store.applySnapshotProjects(projects)
            store.handleReconnect(newEpoch: epoch, latestHostSeq: latestSeq, mode: mode,
                                  snapshotTasks: tasks, snapshotApprovals: approvals)
            syncing = false
            for seq in heldPushes.sorted() { store.ingestHostEvent(seq: seq) }
            heldPushes = []
            isConnected = true

            // 5. Same host start: what was not answered goes again, same ids.
            try await flush()
        } catch {
            if generation == current {
                await closeSocket(reason: Self.reason(for: error))
            }
            throw error
        }
    }

    /// Send every command in the store's outbox this connection has not sent
    /// yet. Call it after `submitApproval` / `createTask`.
    public func flush() async throws {
        guard isConnected, let epoch = store.currentEpoch else { return }
        for command in store.outbox where command.hostEpoch == epoch && !sentThisConnection.contains(command.commandId) {
            let id = takeRequestId()
            waiting[id] = .command(commandId: command.commandId, method: command.method,
                                   requestId: command.method == "task.respond" ? command.params["requestId"] : nil)
            sentThisConnection.insert(command.commandId)
            try await sendControl(try RemoteRpcCodec.encodeCommand(version: version, id: id, command: command))
        }
    }

    /// Close on purpose (the user left, the app went to the background).
    public func disconnect() async {
        await closeSocket(reason: "已断开和 Mac 的连接")
    }

    // MARK: - Reading the Mac's state

    private func readSnapshot(epoch: String) async throws
        -> ([CompanionProject], [CompanionTask], [CompanionApproval]) {
        let projectList = Self.array(try await read("projects.list"))
        var projects: [CompanionProject] = []
        var tasks: [CompanionTask] = []
        var approvals: [CompanionApproval] = []
        for item in projectList {
            guard let id = item["id"] as? String else { continue }
            let target = item["target"] as? [String: Any]
            projects.append(CompanionProject(id: id, name: (item["name"] as? String) ?? id,
                                             path: (target?["path"] as? String) ?? ""))
            for record in Self.array(try await read("tasks.list", ["projectId": id])) {
                guard let taskId = record["id"] as? String else { continue }
                tasks.append(CompanionTask(
                    id: taskId,
                    projectId: (record["projectId"] as? String) ?? id,
                    title: (record["name"] as? String) ?? (record["title"] as? String) ?? taskId,
                    agent: (record["agent"] as? String) ?? "",
                    status: (record["status"] as? String) ?? "",
                    permissionMode: (record["permissionMode"] as? String) ?? "ask"))
                if let approval = record["approval"] as? [String: Any],
                   let requestId = approval["requestId"] as? String {
                    let summary = (approval["toolName"] as? String) ?? (approval["kind"] as? String) ?? "待审批"
                    approvals.append(CompanionApproval(requestId: requestId, taskId: taskId,
                                                       summary: summary, hostEpoch: epoch))
                }
            }
        }
        return (projects, tasks, approvals)
    }

    private func read(_ method: String, _ params: [String: Any] = [:]) async throws -> String {
        let id = takeRequestId()
        let text = try RemoteRpcCodec.encodeRequest(version: version, id: id, method: method, params: params)
        return try await withCheckedThrowingContinuation { continuation in
            waiting[id] = .read(method: method, continuation)
            Task { [weak self] in
                do {
                    try await self?.sendControl(text)
                } catch {
                    self?.settleRead(id, .failure(CompanionLinkError.closed(Self.reason(for: error))))
                }
            }
        }
    }

    private func settleRead(_ id: String, _ result: Result<String, Error>) {
        guard case let .read(_, continuation) = waiting.removeValue(forKey: id) else { return }
        continuation.resume(with: result)
    }

    private func takeRequestId() -> String {
        defer { nextRequest += 1 }
        return String(nextRequest)
    }

    // MARK: - The wire

    private func sendControl(_ text: String) async throws {
        guard let socket, let session else { throw CompanionLinkError.closed("没有连接") }
        let frame = try session.encryptFrame(kind: .control, plaintext: Data(text.utf8))
        try await socket.send(binary: frame)
    }

    /// Before the loop runs (the `auth` answer): read one control frame here.
    private func receiveControlDirectly() async throws -> DecodedRemoteEnvelope? {
        guard let socket, let session else { throw CompanionLinkError.closed("没有连接") }
        switch try await socket.receive() {
        case let .binary(frame):
            let (kind, plain) = try session.decryptFrame(frame)
            guard kind == .control else { return nil }
            return RemoteRpcCodec.decodeEnvelope(String(decoding: plain, as: UTF8.self))
        case let .closed(reason):
            throw CompanionLinkError.closed(reason ?? "Mac 关闭了连接")
        case .text:
            throw CompanionLinkError.unexpected("握手之后收到了明文")
        }
    }

    private func receiveLoop(_ socket: CompanionSocket, generation current: Int) async {
        while generation == current {
            let message: CompanionSocketMessage
            do {
                message = try await socket.receive()
            } catch {
                if generation == current { await closeSocket(reason: "和 Mac 的连接断开了") }
                return
            }
            guard generation == current else { return }
            switch message {
            case let .binary(frame):
                guard let session, let decoded = try? session.decryptFrame(frame) else {
                    await closeSocket(reason: "收到了解不开的帧，已断开")
                    return
                }
                switch decoded.kind {
                case .control:
                    handleControl(String(decoding: decoded.plaintext, as: UTF8.self))
                case .terminal:
                    if let frame = TerminalFrame.decode(decoded.plaintext) { store.ingestTerminalFrame(frame) }
                }
            case .text:
                // After the handshake every frame is encrypted (server.rs).
                await closeSocket(reason: "握手之后收到了明文，已断开")
                return
            case let .closed(reason):
                await closeSocket(reason: reason == "signed-out" ? "电脑端已退出远程连接" : "和 Mac 的连接断开了",
                                  remoteClosed: true)
                return
            }
        }
    }

    private func handleControl(_ text: String) {
        switch RemoteRpcCodec.decodeEnvelope(text) {
        case let .responseSuccess(id, result):
            switch waiting.removeValue(forKey: id) {
            case let .read(_, continuation):
                continuation.resume(returning: result)
            case let .command(_, _, requestId?):
                // Takes the command out of the outbox too.
                store.applyApprovalReply(requestId: requestId, resultJSON: result)
            case let .command(commandId, _, nil):
                store.applyCommandReply(commandId: commandId)
            case nil:
                break
            }
        case let .responseFailure(id, _, message):
            switch waiting.removeValue(forKey: id) {
            case let .read(method, continuation):
                continuation.resume(throwing: CompanionLinkError.remote(method: method, message: message))
            case let .command(commandId, _, requestId):
                store.applyCommandReply(commandId: commandId, failure: message)
                if let requestId { store.applyApprovalOutcome(requestId: requestId, .sendFailed(reason: message)) }
            case nil:
                break
            }
        case let .push(_, seq?, _):
            if syncing { heldPushes.append(seq) } else { store.ingestHostEvent(seq: seq) }
        case .push, nil:
            break
        }
    }

    /// End this connection: every read waiting fails, every answer waiting
    /// is not known to have arrived (its command stays in the outbox).
    private func closeSocket(reason: String, remoteClosed: Bool = false) async {
        guard let socket else { return }
        generation += 1
        self.socket = nil
        session = nil
        isConnected = false
        syncing = false
        heldPushes = []
        let pending = waiting
        waiting = [:]
        for (_, waiter) in pending {
            switch waiter {
            case let .read(_, continuation):
                continuation.resume(throwing: CompanionLinkError.closed(reason))
            case let .command(_, _, requestId?):
                store.applyApprovalOutcome(requestId: requestId, .timedOut)
            case .command:
                break
            }
        }
        if case .revoked = store.connectionState {} else if store.pairedStaticPubB64 != nil {
            store.markMacOffline(relayStillConnected: false, reason: reason)
        }
        if !remoteClosed { await socket.close() }
    }

    // MARK: - Helpers

    private static func object(_ json: String) -> [String: Any] {
        (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any] ?? [:]
    }

    private static func array(_ json: String) -> [[String: Any]] {
        (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [[String: Any]] ?? []
    }

    private static func uint(_ value: Any?) -> UInt64? {
        if let number = value as? NSNumber { return number.uint64Value }
        return nil
    }

    private static func describe(_ error: E2eeError) -> String {
        switch error {
        case .hostIdentityVerificationFailed: "Mac 的身份对不上配对时的公钥"
        case let .serverRejected(reason): "Mac 拒绝了连接：\(reason)"
        default: "握手失败：\(error)"
        }
    }

    private static func reason(for error: Error) -> String {
        switch error as? CompanionLinkError {
        case let .handshake(reason): reason
        case let .authRejected(reason): "Mac 拒绝了这台设备：\(reason)"
        case let .closed(reason): reason
        case let .unexpected(reason): reason
        case let .remote(method, message): "读取 \(method) 失败：\(message)"
        case .notPaired: "还没配对"
        case nil: "和 Mac 的连接出错：\(error)"
        }
    }
}

/// D4 step 4: the real socket, a `URLSessionWebSocketTask` to the Mac's
/// `ws://…` endpoint or the relay's client URL.
public final class URLSessionCompanionSocket: CompanionSocket, @unchecked Sendable {
    private let task: URLSessionWebSocketTask

    public init(url: URL, session: URLSession = .shared) {
        task = session.webSocketTask(with: url)
        // server.rs caps a message at 1 MB.
        task.maximumMessageSize = 1024 * 1024
        task.resume()
    }

    public func send(text: String) async throws { try await task.send(.string(text)) }
    public func send(binary: Data) async throws { try await task.send(.data(binary)) }

    public func receive() async throws -> CompanionSocketMessage {
        do {
            switch try await task.receive() {
            case let .string(text): return .text(text)
            case let .data(data): return .binary(data)
            @unknown default: return .closed(reason: nil)
            }
        } catch {
            if task.closeCode != .invalid {
                return .closed(reason: task.closeReason.map { String(decoding: $0, as: UTF8.self) })
            }
            throw error
        }
    }

    public func close() async { task.cancel(with: .normalClosure, reason: nil) }
}
