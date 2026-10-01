@testable import AeroricClient
@testable import AeroricProtocol
import XCTest
import Darwin
import os

/// In-process UDS server used as the host stand-in: POSIX listen/accept on a
/// background thread, one scripted response per received request line.
/// One accepted connection is live at a time; the state keeps the fds and the
/// lines the fake host has received so far.
private struct ScriptedServerState {
    var listenFD: Int32 = -1
    var clientFD: Int32 = -1
    var received: [String] = []
    var stopped = false
}

final class ScriptedServer: @unchecked Sendable {
    struct Sender: Sendable {
        /// Sends the text plus a newline (a complete frame line).
        let full: @Sendable (String) -> Void
        /// Sends raw bytes without a newline (fragment/coalesce tests).
        let partial: @Sendable (String) -> Void
        /// Closes the accepted connection.
        let close: @Sendable () -> Void
    }

    let path: String
    private let state: OSAllocatedUnfairLock<ScriptedServerState>

    init() {
        let dir = NSTemporaryDirectory() + "hc-" + UUID().uuidString
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        path = (dir as NSString).appendingPathComponent("host.sock")
        state = OSAllocatedUnfairLock(initialState: ScriptedServerState())
    }

    func start(respond: @escaping @Sendable (String, Sender) -> Void) {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        precondition(fd >= 0, "socket() failed: errno \(errno)")
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        precondition(bytes.count < MemoryLayout.size(ofValue: address.sun_path), "test socket path too long")
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
        let bound = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
                Darwin.bind(fd, sockaddrPointer, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        precondition(bound == 0, "bind(\(path)) failed: errno \(errno)")
        precondition(Darwin.listen(fd, 4) == 0, "listen failed: errno \(errno)")
        state.withLock {
            $0.listenFD = fd
        }
        Thread.detachNewThread { [weak self] in
            while let self, !self.isStopped() {
                let client = Darwin.accept(fd, nil, nil)
                if client < 0 { break }
                self.setClient(client)
                self.readLoop(client: client, respond: respond)
            }
        }
    }

    func stop() {
        let fds = state.withLock { fdState -> (listen: Int32, client: Int32, closeClient: Bool) in
            let alreadyStopped = fdState.stopped
            fdState.stopped = true
            let listen = fdState.listenFD
            fdState.listenFD = -1
            let client = fdState.clientFD
            fdState.clientFD = -1
            return (listen, client, !alreadyStopped)
        }
        guard fds.closeClient else { return }
        if fds.listen >= 0 {
            Darwin.shutdown(fds.listen, Int32(SHUT_RDWR))
            Darwin.close(fds.listen)
        }
        if fds.client >= 0 {
            Darwin.shutdown(fds.client, Int32(SHUT_RDWR))
            Darwin.close(fds.client)
        }
        unlink(path)
    }

    func recordedLines() -> [String] {
        state.withLock { $0.received }
    }

    // MARK: - Internals

    private func isStopped() -> Bool {
        state.withLock { $0.stopped }
    }

    private func setClient(_ fd: Int32) {
        state.withLock { $0.clientFD = fd }
    }

    private func record(_ line: String) {
        state.withLock { $0.received.append(line) }
    }

    private func readLoop(client: Int32, respond: @escaping @Sendable (String, Sender) -> Void) {
        var buffer: [UInt8] = []
        var chunk = [UInt8](repeating: 0, count: 65_536)
        while !isStopped() {
            let n = chunk.withUnsafeMutableBytes { raw in
                Darwin.read(client, raw.baseAddress, 65_536)
            }
            if n <= 0 {
                if n < 0, errno == EINTR { continue }
                break
            }
            buffer.append(contentsOf: chunk[0 ..< n])
            while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                let line = String(decoding: buffer[0 ..< newline], as: UTF8.self)
                buffer.removeSubrange(0 ... newline)
                record(line)
                let sender = Sender(
                    full: { [weak self] text in self?.send(text: text, newline: true, fd: client) },
                    partial: { [weak self] text in self?.send(text: text, newline: false, fd: client) },
                    close: { [weak self] in self?.closeClient() }
                )
                respond(line, sender)
            }
        }
    }

    private func send(text: String, newline: Bool, fd: Int32) {
        guard !isStopped() else { return }
        var payload = Array(text.utf8)
        if newline { payload.append(UInt8(ascii: "\n")) }
        var offset = 0
        while offset < payload.count {
            let n = payload.withUnsafeBytes { raw -> Int in
                let base = raw.baseAddress!.advanced(by: offset)
                return Darwin.write(fd, base, payload.count - offset)
            }
            if n > 0 { offset += n; continue }
            if errno == EINTR { continue }
            return
        }
    }

    private func closeClient() {
        let fd = state.withLock { currentState -> Int32 in
            let previous = currentState.clientFD
            currentState.clientFD = -1
            return previous
        }
        guard fd >= 0 else { return }
        Darwin.shutdown(fd, Int32(SHUT_RDWR))
        Darwin.close(fd)
    }
}

final class HostConnectionTests: XCTestCase {
    // MARK: - Script helpers

    private func makeServer() -> ScriptedServer {
        let server = ScriptedServer()
        addTeardownBlock { server.stop() }
        return server
    }

    private static func requestID(of line: String) -> String? {
        guard let data = line.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) as? [String: Any]
        else { return nil }
        return object["id"] as? String
    }

    private static func successLine(id: String, result: JSONValue) -> String {
        // `encodeLine` already terminates with a newline; `Sender.full` adds
        // its own, so strip the trailing one to avoid an empty phantom frame.
        (try? ProtocolCodec.encodeLine(.success(SuccessFrame(id: id, result: result, additional: [:]))))?
            .trimmingCharacters(in: CharacterSet.newlines) ?? ""
    }

    private static func failureLine(id: String, code: WireErrorCode, message: String) -> String {
        (try? ProtocolCodec.encodeLine(.failure(FailureFrame(
            id: id,
            error: RpcError(code: code, message: message, retryable: false),
            additional: [:]
        ))))?.trimmingCharacters(in: CharacterSet.newlines) ?? ""
    }

    private static func echoHandshake(_ line: String, _ sender: ScriptedServer.Sender) -> Bool {
        guard line.contains("handshake"), let id = requestID(of: line) else { return false }
        sender.full(successLine(id: id, result: .object(["clientId": .string("test-client")])))
        return true
    }

    private func makeConnected(_ server: ScriptedServer) async throws -> HostConnection {
        let connection = HostConnection(socketPath: server.path, tokenProvider: { "test-token" })
        try await connection.connect()
        return connection
    }

    private func waitUntil(
        _ condition: @autoclosure () async -> Bool,
        timeout: TimeInterval = 3
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await condition() { return true }
            try? await Task.sleep(for: .milliseconds(50))
        }
        return await condition()
    }

    // MARK: - Tests

    func testHandshakeCarriesTokenAndRoundtripResolves() async throws {
        let server = makeServer()
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard let id = Self.requestID(of: line) else { return }
            sender.full(Self.successLine(id: id, result: .object(["pong": .bool(true)])))
        }
        let connection = try await makeConnected(server)
        let reply = try await connection.request("ping", timeout: .seconds(3))
        XCTAssertEqual(reply.result, .object(["pong": .bool(true)]))
        let lines = server.recordedLines()
        XCTAssertEqual(lines.count, 2)
        XCTAssertTrue(lines[0].contains("handshake"))
        XCTAssertTrue(lines[0].contains("test-token"), "the token rides the handshake params")
        XCTAssertTrue(lines[1].contains("ping"))
    }

    func testFragmentedReplyIsAssembledAcrossReads() async throws {
        let server = makeServer()
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard let id = Self.requestID(of: line) else { return }
            let full = Self.successLine(id: id, result: .object(["pong": .bool(true)]))
            let bytes = Array((full + "\n").utf8)
            let cut = bytes.count / 2
            sender.partial(String(decoding: bytes[0 ..< cut], as: UTF8.self))
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.1) {
                sender.partial(String(decoding: bytes[cut...], as: UTF8.self))
            }
        }
        let connection = try await makeConnected(server)
        let reply = try await connection.request("ping", timeout: .seconds(3))
        XCTAssertEqual(reply.result, .object(["pong": .bool(true)]))
    }

    func testCoalescedDuplicateResponsesResolveOnce() async throws {
        let server = makeServer()
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard let id = Self.requestID(of: line) else { return }
            let same = Self.successLine(id: id, result: .object(["n": .number("1")]))
            sender.full(same)
            sender.full(same) // duplicate in the same read batch
        }
        let connection = try await makeConnected(server)
        let reply = try await connection.request("ping", timeout: .seconds(3))
        XCTAssertEqual(reply.result, .object(["n": .number("1")]))
    }

    func testLateDuplicateAfterResolutionIsIgnoredWithoutFault() async throws {
        let server = makeServer()
        let faults = Locked<[ConnectionError]>([])
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard let id = Self.requestID(of: line) else { return }
            let line = Self.successLine(id: id, result: .object(["late": .bool(true)]))
            sender.full(line)
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.15) {
                sender.full(line) // arrives after the request already resolved
            }
        }
        let connection = try await makeConnected(server)
        await connection.setFaultSink { fault in
            faults.mutate { $0.append(fault) }
        }
        _ = try await connection.request("ping", timeout: .seconds(3))
        try await Task.sleep(for: .milliseconds(400))
        let clean = await waitUntil(faults.value.isEmpty, timeout: 0.5)
        XCTAssertTrue(clean, "a late duplicate is protocol-legal; it must not fault the channel")
    }

    func testUnknownIDReplyTimesOutWithoutCrash() async throws {
        let server = makeServer()
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard Self.requestID(of: line) != nil else { return }
            // Reply with an id the client never sent.
            sender.full(Self.successLine(id: "bogus", result: .object(["x": .bool(true)])))
        }
        let connection = try await makeConnected(server)
        do {
            _ = try await connection.request("ping", timeout: .milliseconds(300))
            XCTFail("expected a timeout")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .timedOut)
        }
    }

    func testUnansweredRequestTimesOut() async throws {
        let server = makeServer()
        server.start { line, sender in
            _ = Self.echoHandshake(line, sender) // ping is never answered
        }
        let connection = try await makeConnected(server)
        do {
            _ = try await connection.request("ping", timeout: .milliseconds(300))
            XCTFail("expected a timeout")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .timedOut)
        }
    }

    func testCancelledRequestResolvesCancelled() async throws {
        let server = makeServer()
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard let id = Self.requestID(of: line) else { return }
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.8) {
                sender.full(Self.successLine(id: id, result: .null))
            }
        }
        let connection = try await makeConnected(server)
        let task = Task {
            try await connection.request("ping", timeout: .seconds(10))
        }
        try await Task.sleep(for: .milliseconds(100))
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .cancelled)
        } catch is CancellationError {
            // Registry resumed with .cancelled → surfaced as ConnectionError.
            XCTFail("cancellation must map to ConnectionError.cancelled, not CancellationError")
        }
    }

    func testDisconnectFailsPendingAndFiresFaultSink() async throws {
        let server = makeServer()
        let faults = Locked<[ConnectionError]>([])
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard Self.requestID(of: line) != nil else { return }
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.1) {
                sender.close()
            }
        }
        let connection = try await makeConnected(server)
        await connection.setFaultSink { fault in
            faults.mutate { $0.append(fault) }
        }
        do {
            _ = try await connection.request("ping", timeout: .seconds(5))
            XCTFail("expected a disconnect")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .disconnected)
        }
        let faulted = await waitUntil(!faults.value.isEmpty, timeout: 2)
        XCTAssertTrue(faulted)
        XCTAssertEqual(faults.value.first, .disconnected)
    }

    func testOverlongFrameFaultsTheChannel() async throws {
        let server = makeServer()
        let faults = Locked<[ConnectionError]>([])
        server.start { line, sender in
            if Self.echoHandshake(line, sender) { return }
            guard Self.requestID(of: line) != nil else { return }
            sender.partial(String(repeating: "x", count: ProtocolCodec.maxFrameBytes + 1))
        }
        let connection = try await makeConnected(server)
        await connection.setFaultSink { fault in
            faults.mutate { $0.append(fault) }
        }
        do {
            _ = try await connection.request("ping", timeout: .seconds(5))
            XCTFail("expected a transport fault")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .disconnected, "pending requests fail when the channel faults")
        }
        let faulted = await waitUntil(!faults.value.isEmpty, timeout: 2)
        XCTAssertTrue(faulted)
        let overlong = faults.value.contains { fault in
            if case .transport(.overlong) = fault { return true }
            return false
        }
        XCTAssertTrue(overlong, "expected an overlong transport fault, got \(faults.value)")
    }

    func testHandshakeFailureSurfacesAuthDenied() async throws {
        let server = makeServer()
        server.start { line, sender in
            guard line.contains("handshake"), let id = Self.requestID(of: line) else { return }
            sender.full(Self.failureLine(id: id, code: .unauthenticated, message: "bad token"))
        }
        let connection = HostConnection(socketPath: server.path, tokenProvider: { "wrong-token" })
        do {
            try await connection.connect()
            XCTFail("expected auth denial")
        } catch let error as ConnectionError {
            XCTAssertEqual(error, .authDenied("bad token"))
        }
    }
}
