import Foundation
import AeroricProtocol

/// Client-side connection errors. `authDenied` carries the host's message —
/// never the token, which never leaves the token-file read path.
public enum ConnectionError: Error, Equatable, Sendable {
    case disconnected
    case timedOut
    case cancelled
    case authDenied(String)
    case notConnected
    /// A well-formed refusal from the host (`ok:false`). Carries the frame so
    /// the caller can branch on `code` (e.g. `unauthenticated` → re-auth).
    case refused(FailureFrame)
    case io(String)
    case transport(TransportFault)
}

/// Blocking UDS transport. Reads run on a dedicated serial queue (the loop is
/// allowed to block on `read`); writes go through a second serial queue so a
/// blocked reader can never starve a request. `close` is thread-safe and
/// unblocks the reader (shutdown + close from another thread is the documented
/// way out of a blocking `read` on a socket fd).
final class SocketTransport: @unchecked Sendable {
    private let lock = NSLock()
    private var fd: Int32?
    private let readQueue: DispatchQueue
    private let writeQueue: DispatchQueue

    init(readQueue: DispatchQueue = DispatchQueue(label: "aeroric.client.read", qos: .userInitiated),
         writeQueue: DispatchQueue = DispatchQueue(label: "aeroric.client.write", qos: .userInitiated)) {
        self.readQueue = readQueue
        self.writeQueue = writeQueue
    }

    static func connect(to path: String) throws -> Int32 {
        guard FileManager.default.fileExists(atPath: path) else {
            throw ConnectionError.io("socket does not exist: \(path)")
        }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw ConnectionError.io("socket() failed: errno \(errno)") }
        // The host can close its end between our connect and a write (a
        // SIGTERMed host drops its connections before its listener, so a
        // reconnect can land in the backlog of a dying process). Neither the app
        // nor XCTest ignores SIGPIPE and Darwin has no MSG_NOSIGNAL, so without
        // this a write to the dead peer kills the whole process instead of
        // returning EPIPE to `send`'s fault path.
        var noSigPipe: Int32 = 1
        guard setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSigPipe, socklen_t(MemoryLayout<Int32>.size)) == 0 else {
            let errnoValue = errno
            Darwin.close(fd)
            throw ConnectionError.io("setsockopt(SO_NOSIGPIPE) failed: errno \(errnoValue)")
        }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        let sunPathCapacity = MemoryLayout.size(ofValue: address.sun_path)
        // -1 keeps room for the NUL terminator.
        guard pathBytes.count < sunPathCapacity else {
            Darwin.close(fd)
            throw ConnectionError.io("socket path exceeds sun_path: \(path.count) bytes")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { destination in
            destination.copyBytes(from: pathBytes)
        }
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
                Darwin.connect(fd, sockaddrPointer, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            let errnoValue = errno
            Darwin.close(fd)
            throw ConnectionError.io("connect(\(path)) failed: errno \(errnoValue)")
        }
        return fd
    }

    /// Starts the read pump. `onFault` fires exactly once when the stream ends
    /// for any reason (peer close, read error, close from our side).
    func startReading(onBytes: @escaping @Sendable ([UInt8]) -> Void,
                      onFault: @escaping @Sendable (ConnectionError) -> Void) {
        readQueue.async { [weak self] in
            guard let self, let fd = self.currentFD() else { return }
            let chunkSize = 65_536
            var chunk = [UInt8](repeating: 0, count: chunkSize)
            while true {
                let n = chunk.withUnsafeMutableBytes { raw in
                    read(fd, raw.baseAddress, chunkSize)
                }
                if n > 0 {
                    onBytes(Array(chunk[0 ..< n]))
                    continue
                }
                if n == 0 { onFault(.disconnected); return }
                if errno == EINTR { continue }
                onFault(.io("read failed: errno \(errno)"))
                return
            }
        }
    }

    /// Serializes writes; never blocks the caller.
    func send(_ bytes: [UInt8], onFault: @escaping @Sendable (ConnectionError) -> Void) {
        writeQueue.async { [weak self] in
            guard let self, let fd = self.currentFD() else { return }
            var offset = 0
            while offset < bytes.count {
                let n = bytes.withUnsafeBytes { raw -> Int in
                    let base = raw.baseAddress!.advanced(by: offset)
                    return write(fd, base, bytes.count - offset)
                }
                if n > 0 { offset += n; continue }
                if errno == EINTR { continue }
                onFault(.io("write failed: errno \(errno)"))
                return
            }
        }
    }

    func close() {
        lock.lock()
        let fdToClose = fd
        fd = nil
        lock.unlock()
        guard let fdToClose, fdToClose >= 0 else { return }
        shutdown(fdToClose, Int32(SHUT_RDWR))
        Foundation.close(fdToClose)
    }

    private func currentFD() -> Int32? {
        lock.lock()
        defer { lock.unlock() }
        return fd
    }
}

/// One authenticated request/reply/event channel to a host over UDS.
///
/// Concurrency model: the actor owns all state; blocking I/O lives in
/// [`SocketTransport`] on its own queues. Inbound frames are routed back into
/// the actor via Tasks; replies resolve the registry exactly once (see
/// [`RequestRegistry`]); events are handed to the event sink.
public actor HostConnection {
    private let socketPath: String
    private let tokenProvider: @Sendable () throws -> String
    private let registry = RequestRegistry()
    private var splitter = JSONLFrameSplitter()
    private var transport: SocketTransport?
    private var clientID = UUID().uuidString
    private var eventSink: (@Sendable (EventFrame) -> Void)?
    private var faultSink: (@Sendable (ConnectionError) -> Void)?
    private var isClosed = false
    /// Capabilities the host advertised in the handshake reply (name → enabled).
    /// Empty until a handshake succeeds; replaced on every reconnect. UI gates on
    /// this, never on probing a method.
    public private(set) var advertisedCapabilities: [String: Bool] = [:]

    public init(socketPath: String, tokenProvider: @escaping @Sendable () throws -> String) {
        self.socketPath = socketPath
        self.tokenProvider = tokenProvider
    }

    public func setEventSink(_ sink: (@Sendable (EventFrame) -> Void)?) {
        eventSink = sink
    }

    public func setFaultSink(_ sink: (@Sendable (ConnectionError) -> Void)?) {
        faultSink = sink
    }

    /// Connects, then performs the pre-auth handshake (`handshake` is the only
    /// method the host accepts before auth). Token is read from the provider —
    /// the data-dir token file — and never logged or stored beyond the frame.
    public func connect() async throws {
        guard transport == nil, !isClosed else {
            if isClosed { throw ConnectionError.disconnected }
            // Already connected: connect is idempotent.
            return
        }
        let socket = try SocketTransport.connect(to: socketPath)
        let io = SocketTransport()
        io.open(fd: socket)
        transport = io
        io.startReading(
            onBytes: { [weak self] bytes in
                guard let self else { return }
                Task { await self.ingest(bytes) }
            },
            onFault: { [weak self] fault in
                guard let self else { return }
                Task { await self.faulted(fault) }
            }
        )

        let token = try tokenProvider()
        let params: JSONValue = .object([
            "token": .string(token),
            "clientId": .string(clientID),
        ])
        do {
            // The registry routes the reply by the id `request()` minted; a
            // reply for a different id never fulfils and surfaces as a timeout.
            let reply = try await request("handshake", params: params, timeout: .seconds(30))
            advertisedCapabilities = Self.parseCapabilities(reply.result)
        } catch let error as ConnectionError {
            switch error {
            case let .refused(failure):
                // Pre-auth the host refuses everything but a valid token.
                throw ConnectionError.authDenied(failure.error.message)
            default:
                throw error
            }
        }
    }

    /// The handshake reply's `capabilities` array (`[{name, enabled, since?, note?}]`)
    /// as name → enabled. Malformed entries are skipped, not guessed.
    public static func parseCapabilities(_ result: JSONValue) -> [String: Bool] {
        var out: [String: Bool] = [:]
        for item in result["capabilities"]?.arrayValue ?? [] {
            guard let name = item["name"]?.stringValue, !name.isEmpty,
                  let enabled = item["enabled"]?.boolValue else { continue }
            out[name] = enabled
        }
        return out
    }

    /// Sends a request and awaits its reply. Each request gets its own id;
    /// resolution is exactly once (timeout race included).
    public func request(_ method: String,
                        params: JSONValue = .null,
                        timeout: Duration = .seconds(30)) async throws -> SuccessFrame {
        let transport = self.transport
        guard let transport else { throw ConnectionError.notConnected }

        let id = newRequestID()
        let requestFrame = RequestFrame(
            id: id,
            method: method,
            idempotencyKey: nil,
            params: params,
            additional: [:]
        )
        // `encodeLine` already appends the newline delimiter.
        let payload = Array((try ProtocolCodec.encodeLine(.request(requestFrame))).utf8)
        transport.send(payload) { [weak self] fault in
            guard let self else { return }
            Task { await self.faulted(fault) }
        }

        // Arm the timeout FIRST: the reply may beat the `awaitReply`
        // suspension, and the race must already be running when it does.
        let timeoutTask = Task { [weak self, id] in
            try await Task.sleep(for: timeout)
            guard !Task.isCancelled else { return }
            await self?.registry.timeOut(id: id)
        }
        let resolution: RequestResolution
        do {
            resolution = try await registry.awaitReply(id: id)
        } catch {
            timeoutTask.cancel()
            throw error
        }
        timeoutTask.cancel()

        switch resolution {
        case let .success(frame):
            return frame
        case let .failure(failure):
            throw ConnectionError.refused(failure)
        case .timedOut:
            throw ConnectionError.timedOut
        case .cancelled:
            throw ConnectionError.cancelled
        case .disconnected:
            throw ConnectionError.disconnected
        }
    }

    /// Closes the transport, fails pending requests and fires the fault sink.
    public func close() async {
        guard !isClosed else { return }
        isClosed = true
        transport?.close()
        transport = nil
        await registry.failAll(.disconnected)
        faultSink?(.disconnected)
    }

    // MARK: - Routing (private)

    private func ingest(_ bytes: [UInt8]) async {
        guard !isClosed else { return }
        // Splitter state is per-connection: a read boundary is never a frame
        // boundary, so the buffer must survive across `ingest` calls.
        let batch = splitter.feed(bytes)
        for line in batch.lines {
            switch ProtocolCodec.decodeLine(Data(line.utf8)) {
            case let .success(frame):
                await route(frame)
            case let .failure(frameError):
                await faulted(.transport(.line(frameError)))
                return
            }
        }
        if let fault = batch.fault {
            await faulted(.transport(fault))
        }
    }

    private func route(_ frame: Frame) async {
        switch frame {
        case .event, .request:
            // Events are not replies; host→client requests do not exist yet
            // and are dropped rather than crashing on an unknown shape.
            if case let .event(eventFrame) = frame {
                eventSink?(eventFrame)
            }
        case .success, .failure:
            await registry.fulfill(id: replyID(of: frame), frame: frame)
        }
    }

    private func replyID(of frame: Frame) -> String {
        switch frame {
        case let .success(frame): frame.id
        case let .failure(frame): frame.id
        case .event, .request: ""
        }
    }

    private func faulted(_ fault: ConnectionError) async {
        guard !isClosed else { return }
        isClosed = true
        transport?.close()
        transport = nil
        await registry.failAll(.disconnected)
        faultSink?(fault)
    }

    private func newRequestID() -> String {
        UUID().uuidString
    }
}

extension SocketTransport {
    /// Adopts an already-connected fd (created by the static `connect`).
    func open(fd: Int32) {
        lock.lock()
        self.fd = fd
        lock.unlock()
    }
}
