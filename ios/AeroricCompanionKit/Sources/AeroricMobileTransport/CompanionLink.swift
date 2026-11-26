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
    /// D4 step 6: a re-read is running (one at a time).
    private var resyncing = false
    /// D4 step 8: the Mac pushed that a task changed; read again.
    private var refreshWanted = false
    /// D4 step 8: the pushes that mean "read again".
    public static let refreshEvents: Set<String> = ["task-status", "task-changed"]

    public init(store: CompanionStore) {
        self.store = store
    }

    /// Called whenever a connection ends, with the reason the store shows.
    public var onClose: (@MainActor (String) -> Void)?

    /// 1124e: the Mac's `hello` names its id and the addresses it can be
    /// reached at now; the connector keeps them for the next reconnect.
    public var onHostEndpoints: (@MainActor (_ hostId: String, _ endpoints: [String]) -> Void)?

    /// Handshake, authenticate, read the Mac's state into the store, then
    /// send what is waiting. Throws if any step fails; the connection is
    /// closed and the store says why.
    public func connect(
        socket: CompanionSocket,
        deviceToken: String,
        mode: CompanionConnectionState.TransportMode
    ) async throws {
        guard let staticPub = store.pairedStaticPubB64 else { throw CompanionLinkError.notPaired }
        let current = await begin(socket)
        do {
            try await handshake(staticPub: staticPub)
            _ = try await authenticate(["deviceToken": deviceToken], revokeOnUnknownToken: true)
            try await synchronize(mode: mode, generation: current)
        } catch {
            await fail(current, error)
            throw error
        }
    }

    /// D4 step 5: pair with an invite from the Mac's pairing link. The
    /// credentials are staged durably before `pairing.confirm` (the Mac keeps
    /// the device only after it), committed on `confirmed`, dropped if the
    /// Mac refuses, and left staged if the answer never came. Then the same
    /// state read as `connect`.
    @discardableResult
    public func pair(
        socket: CompanionSocket,
        offer: CompanionPairingOffer,
        deviceName: String,
        credentials: CompanionCredentialStore,
        mode: CompanionConnectionState.TransportMode
    ) async throws -> CompanionCredentials {
        let current = await begin(socket)
        do {
            try await handshake(staticPub: offer.publicKey)
            let reply = try await authenticate(
                ["invite": offer.invite, "deviceName": deviceName,
                 "pairingConfirmationVersion": CompanionPairingOffer.confirmationVersion],
                revokeOnUnknownToken: false)
            guard let deviceId = reply["deviceId"] as? String, !deviceId.isEmpty,
                  let token = reply["deviceToken"] as? String, !token.isEmpty else {
                throw CompanionLinkError.unexpected("配对的回答里没有设备令牌")
            }
            let host = reply["host"] as? [String: Any]
            let paired = CompanionCredentials(
                hostName: (host?["name"] as? String) ?? offer.hostName, hostId: offer.hostId,
                publicKey: offer.publicKey, endpoints: offer.endpoints, deviceId: deviceId, deviceToken: token)
            try credentials.stagePending(paired)
            if let pairingId = reply["pairingId"] as? String {
                let confirm = try RemoteRpcCodec.encodeRequest(
                    version: .v2, id: "2", method: "pairing.confirm",
                    params: ["pairingId": pairingId,
                             "pairingConfirmationVersion": CompanionPairingOffer.confirmationVersion])
                try await sendControl(confirm)
                switch try await receiveControlDirectly() {
                case let .responseSuccess(_, result) where (Self.object(result)["confirmed"] as? Bool) == true:
                    try credentials.commitPending()
                case let .responseFailure(_, _, message):
                    try? credentials.discardPending()
                    throw CompanionLinkError.authRejected(message)
                default:
                    // Whether the Mac kept the device is unknown: stay staged.
                    throw CompanionLinkError.unexpected("Mac 没有确认这次配对")
                }
                nextRequest = 3
            } else {
                // An older Mac keeps the device at once.
                try credentials.commitPending()
            }
            store.restorePairing(hostName: paired.hostName, staticPubB64: paired.publicKey,
                                 deviceID: paired.deviceId)
            try await synchronize(mode: mode, generation: current)
            return paired
        } catch {
            await fail(current, error)
            throw error
        }
    }

    /// D4 step 5: one v3 request on the authenticated channel (`push.register`).
    public func call(_ method: String, _ params: [String: Any] = [:]) async throws -> String {
        guard isConnected else { throw CompanionLinkError.closed("没有连上 Mac") }
        return try await read(method, params)
    }

    // MARK: - Connection steps

    private func begin(_ socket: CompanionSocket) async -> Int {
        if self.socket != nil { await disconnect() }
        generation += 1
        self.socket = socket
        sentThisConnection = []
        heldPushes = []
        nextRequest = 1
        return generation
    }

    private func fail(_ current: Int, _ error: Error) async {
        if generation == current { await closeSocket(reason: Self.reason(for: error)) }
    }

    /// The plaintext handshake, pinned to `staticPub`.
    private func handshake(staticPub: String) async throws {
        guard let socket else { throw CompanionLinkError.closed("没有连接") }
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
    }

    /// `auth` is always v2; its answer says which RPC version follows.
    private func authenticate(_ params: [String: Any], revokeOnUnknownToken: Bool) async throws -> [String: Any] {
        var params = params
        params["supportedRpcVersions"] = [3]
        try await sendControl(try RemoteRpcCodec.encodeRequest(version: .v2, id: "1", method: "auth", params: params))
        nextRequest = 2
        switch try await receiveControlDirectly() {
        case let .responseSuccess(_, result):
            let reply = Self.object(result)
            version = (reply["rpcVersion"] as? Int) == 3 ? .v3 : .v2
            return reply
        case let .responseFailure(_, _, message):
            if revokeOnUnknownToken, message == "Unknown device token" {
                store.handleDeviceRevoked(reason: "Mac 不认这台设备了，需要重新配对")
            }
            throw CompanionLinkError.authRejected(message)
        default:
            throw CompanionLinkError.unexpected("认证没有得到回答")
        }
    }

    /// From here the receive loop owns the socket: read the Mac's state into
    /// the store, then send what is waiting.
    private func synchronize(mode: CompanionConnectionState.TransportMode, generation current: Int) async throws {
        guard let socket else { throw CompanionLinkError.closed("没有连接") }
        syncing = true
        Task { [weak self] in await self?.receiveLoop(socket, generation: current) }

        let hello = Self.object(try await read("hello"))
        guard let epoch = hello["hostEpoch"] as? String, !epoch.isEmpty else {
            throw CompanionLinkError.unexpected("hello 没有 hostEpoch")
        }
        if let hostId = hello["hostId"] as? String, let endpoints = hello["endpoints"] as? [String] {
            onHostEndpoints?(hostId, endpoints)
        }
        let since = Self.object(try await read("events.since", ["after": store.lastEventSeq]))
        let latestSeq = Self.uint(since["latestSeq"]) ?? store.lastEventSeq
        let (projects, tasks, approvals) = try await readSnapshot(epoch: epoch)

        // 1124c: connected before the snapshot lands. Applying it asks the
        // open pages (文件与 Git, the task page) to read again, and the
        // connector drops a read while the link is not connected: a page that
        // failed when the last connection dropped stayed failed.
        isConnected = true
        // The snapshot is authoritative; a new host start drops the outbox.
        store.applySnapshotProjects(projects)
        store.handleReconnect(newEpoch: epoch, latestHostSeq: latestSeq, mode: mode,
                              snapshotTasks: tasks, snapshotApprovals: approvals)
        syncing = false
        for seq in heldPushes.sorted() { store.ingestHostEvent(seq: seq) }
        heldPushes = []

        // Same host start: what was not answered goes again, same ids.
        try await flush()
        // D4 step 8: something changed while the snapshot was read.
        resyncIfNeeded()
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
                    title: CompanionTask.title(of: record, fallback: taskId),
                    agent: (record["agent"] as? String) ?? "",
                    status: (record["status"] as? String) ?? "",
                    permissionMode: (record["permissionMode"] as? String) ?? "ask",
                    generation: (record["generation"] as? String) ?? (record["generation"] as? NSNumber)?.stringValue,
                    activityMs: CompanionTaskList.activity(of: record)))
                if let approval = record["approval"] as? [String: Any],
                   let requestId = approval["requestId"] as? String {
                    let summary = (approval["toolName"] as? String) ?? (approval["kind"] as? String) ?? "待审批"
                    approvals.append(CompanionApproval(requestId: requestId, taskId: taskId,
                                                       summary: summary, hostEpoch: epoch))
                }
                // D4 step 8: a live session's open requests (its driver takes the answer).
                for request in record["pendingApprovals"] as? [[String: Any]] ?? [] {
                    guard let requestId = request["requestId"] as? String,
                          !approvals.contains(where: { $0.requestId == requestId }) else { continue }
                    var summary = (request["summary"] as? String).flatMap { $0.isEmpty ? nil : $0 }
                        ?? (request["kind"] as? String) ?? "待审批"
                    if let subagent = request["subagent"] as? String { summary = "\(subagent) · \(summary)" }
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
                    resyncIfNeeded()
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

    /// D4 step 6: the store no longer trusts what it shows (a stale refusal,
    /// a gap, an overflow): read the Mac's state again on this connection.
    private func resyncIfNeeded() {
        guard store.resyncNeeded || refreshWanted, isConnected, !syncing, !resyncing,
              let epoch = store.currentEpoch, case let .macOnline(_, mode) = store.connectionState else { return }
        resyncing = true
        refreshWanted = false
        let current = generation
        Task { [weak self] in
            guard let self else { return }
            var succeeded = false
            defer {
                self.resyncing = false
                // D4 step 8: a push that came while reading asks for one more.
                if succeeded { self.resyncIfNeeded() }
            }
            do {
                self.syncing = true
                let since = Self.object(try await self.read("events.since", ["after": self.store.lastEventSeq]))
                let latestSeq = Self.uint(since["latestSeq"]) ?? self.store.lastEventSeq
                let (projects, tasks, approvals) = try await self.readSnapshot(epoch: epoch)
                guard self.generation == current else { return }
                self.store.applySnapshotProjects(projects)
                self.store.handleReconnect(newEpoch: epoch, latestHostSeq: max(latestSeq, self.store.lastEventSeq),
                                           mode: mode, snapshotTasks: tasks, snapshotApprovals: approvals)
                self.syncing = false
                for seq in self.heldPushes.sorted() { self.store.ingestHostEvent(seq: seq) }
                self.heldPushes = []
                succeeded = true
            } catch {
                // Left for the next frame or the next connection.
                if self.generation == current { self.syncing = false }
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
        case let .push(event, seq, _):
            if let seq {
                if syncing { heldPushes.append(seq) } else { store.ingestHostEvent(seq: seq) }
            }
            if Self.refreshEvents.contains(event) { refreshWanted = true }
        case nil:
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
        onClose?(reason)
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
        case nil: "和 Mac 的连接出错：\(networkReason(error))"
        }
    }

    /// 1124e: a network error in words, not the NSError dictionary (the
    /// offline banner showed `Error Domain=NSURLErrorDomain Code=-1001 …`).
    public nonisolated static func networkReason(_ error: Error) -> String {
        guard let error = error as? URLError else { return error.localizedDescription }
        switch error.code {
        case .timedOut: return "连接超时，Mac 的地址可能变了"
        case .cannotConnectToHost: return "连不上，Mac 的远程访问可能没开"
        case .networkConnectionLost: return "网络断了"
        case .notConnectedToInternet: return "这台手机没有联网"
        case .cannotFindHost, .dnsLookupFailed: return "找不到这个地址"
        default: return "网络出错（\(error.code.rawValue)）"
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
