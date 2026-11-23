@testable import AeroricMobileTransport
import Foundation
import XCTest

/// A Mac on the other end of a fake socket, answering the way
/// `core/.../remote/server.rs` does: plaintext `hello` → `hello_ack`, then
/// encrypted frames; `auth` in v2, everything after in v3.
final class FakeMac: CompanionSocket, @unchecked Sendable {
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
        /// D4 step 5: the invite this Mac accepts, and what it does with
        /// `pairing.confirm`.
        var invite = "invite-1"
        var confirmation: Confirmation = .confirm
        var issuedToken = "token-1"
    }

    enum Confirmation { case confirm, refuse, silent, legacy }

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
    private(set) var confirmations = 0

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
        if method == "pairing.confirm" {
            let id = request["id"] as? Int ?? 0
            confirmations += 1
            switch config.confirmation {
            case .confirm, .legacy:
                XCTAssertEqual(params["pairingId"] as? String, "pairing-1")
                XCTAssertEqual(params["pairingConfirmationVersion"] as? Int, 1)
                config.knownToken = config.issuedToken
                return [encrypted(["v": 2, "id": id, "ok": true, "result": ["confirmed": true]])]
            case .refuse:
                return [encrypted(["v": 2, "id": id, "ok": false, "error": "Pairing expired"])]
            case .silent:
                return [.closed(reason: nil)]
            }
        }
        if method == "auth" {
            let id = request["id"] as? Int ?? 0
            guard (request["v"] as? Int) == 2 else { return [encrypted(["v": 2, "id": id, "ok": false, "error": "auth must be v2"])] }
            if let invite = params["invite"] as? String {
                guard invite == config.invite else {
                    return [encrypted(["v": 2, "id": id, "ok": false, "error": "Invalid or expired invite"])]
                }
                var result: [String: Any] = ["deviceId": "dev-1", "deviceToken": config.issuedToken,
                                             "host": ["name": "Studio Mac"], "rpcVersion": 3]
                if config.confirmation == .legacy {
                    config.knownToken = config.issuedToken
                } else {
                    result["pairingId"] = "pairing-1"
                    result["pairingConfirmationVersion"] = 1
                }
                return [encrypted(["v": 2, "id": id, "ok": true, "result": result])]
            }
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

