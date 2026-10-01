import Foundation
import os
import AeroricProtocol

/// Supervisor lifecycle phases — mirrored verbatim by the UI store.
public enum SupervisorPhase: Equatable, Sendable {
    case idle
    case connecting
    case ready
    /// Session-level fault with a short reason (spawn failure, exit status).
    case degraded(String)
    /// Backing off before attempt `attempt` (1-based retry count).
    case reconnecting(attempt: Int, nextDelayMs: UInt64)
    /// Host identity contradicts what the supervisor was configured for.
    case incompatible(String)
    case stopped
}

/// The CLI's ready-pipe line (`ReadyInfo`, camelCase), structurally validated:
/// fields we do not know about are tolerated, fields we depend on are not.
public struct ReadyInfo: Equatable, Sendable {
    public var pid: Int
    public var startedAtMs: UInt64
    public var epoch: String
    public var revision: String
    public var buildHash: String
    public var protocolHash: String
    public var dataDirHash: String
    public var socketPath: String

    public static func decode(_ line: String) -> ReadyInfo? {
        guard let data = line.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]),
              let fields = object as? [String: Any] else { return nil }
        guard let pid = fields["pid"] as? Int,
              let epoch = fields["epoch"] as? String,
              let buildHash = fields["buildHash"] as? String,
              let protocolHash = fields["protocolHash"] as? String,
              let dataDirHash = fields["dataDirHash"] as? String,
              let socketPath = fields["socketPath"] as? String else { return nil }
        let startedAtMs = fields["startedAtMs"] as? UInt64
            ?? (fields["startedAtMs"] as? Int).map(UInt64.init)
            ?? 0
        let revision = fields["revision"] as? String ?? ""
        return ReadyInfo(pid: pid, startedAtMs: startedAtMs, epoch: epoch, revision: revision,
                         buildHash: buildHash, protocolHash: protocolHash,
                         dataDirHash: dataDirHash, socketPath: socketPath)
    }
}

public struct SupervisorConfig: Sendable {
    public var dataDir: URL
    public var socketPath: String
    /// Defaults to `Contents/MacOS/aeroric-cli` of our own bundle — the
    /// supervisor never searches PATH or the working directory.
    public var cliExecutable: URL?
    /// Environment for the child: whitelist, never a blind passthrough.
    public var environment: [String: String]
    public var readyTimeout: Duration
    public var policy: ReconnectPolicy

    public init(dataDir: URL,
                socketPath: String,
                cliExecutable: URL? = nil,
                environment: [String: String] = [:],
                readyTimeout: Duration = .seconds(15),
                policy: ReconnectPolicy = ReconnectPolicy()) {
        self.dataDir = dataDir
        self.socketPath = socketPath
        self.cliExecutable = cliExecutable
        self.environment = environment
        self.readyTimeout = readyTimeout
        self.policy = policy
    }

    public static func defaultCLIURL() -> URL {
        Bundle.main.bundleURL
            .appendingPathComponent("Contents/MacOS/aeroric-cli")
    }
}

/// Lifecycle events of one spawn attempt, funneled into a single stream the
/// run loop awaits. Child death and connection faults converge here so the
/// loop has one await point instead of racing two watchers.
enum SupervisorEvent: Sendable {
    case connectionFault(ConnectionError)
    case childExited(status: Int32)
}

/// Spawns and supervises the headless host.
///
/// - Spawns `aeroric-cli --data-dir <dir> serve --ready-fd 3` from the bundle
///   (never PATH, never the working directory).
/// - Validates the ready line: socket path must match configuration.
/// - Connects and handshakes via [`HostConnection`]; "ready" in the UI means
///   the RPC channel is authenticated, not merely that the process lives.
/// - On fault/exit: degrade, back off per [`ReconnectPolicy`], respawn.
///   Tasks are NEVER restarted by the supervisor — it only owns the host
///   process ("不重复启动 task" is enforced here by construction: no task
///   identifiers exist in this layer).
public actor HostSupervisor {
    public private(set) var phase: SupervisorPhase = .idle
    /// Ready line of the most recent successful spawn attempt (cleared on stop).
    public private(set) var currentReadyInfo: ReadyInfo?
    private let config: SupervisorConfig
    private let onChange: @Sendable (SupervisorPhase) -> Void
    private let configureConnection: @Sendable (HostConnection) async -> Void
    private var runTask: Task<Void, Never>?
    private var child: ChildHandle?
    private var connection: HostConnection?
    private var stopped = false

    public init(config: SupervisorConfig,
                onChange: @escaping @Sendable (SupervisorPhase) -> Void,
                configureConnection: @escaping @Sendable (HostConnection) async -> Void = { _ in }) {
        self.config = config
        self.onChange = onChange
        self.configureConnection = configureConnection
    }

    /// Idempotent start, also the restart path after `stop()`.
    public func start() {
        if runTask != nil { return }
        stopped = false
        runTask = Task { [weak self] in
            await self?.runLoop()
        }
    }

    /// Terminates the child (SIGTERM, then SIGKILL after a grace period) and
    /// closes the connection. The run loop observes `stopped` and exits.
    public func stop() async {
        stopped = true
        runTask?.cancel()
        runTask = nil
        await connection?.close()
        connection = nil
        child?.terminate()
        child = nil
        currentReadyInfo = nil
        setPhase(.stopped)
    }

    // MARK: - Run loop

    private func runLoop() async {
        var consecutiveFailures = 0
        while !stopped, !Task.isCancelled {
            if consecutiveFailures > 0 {
                guard let delay = config.policy.delay(afterAttempt: consecutiveFailures) else {
                    setPhase(.degraded("retry budget exhausted"))
                    return
                }
                setPhase(.reconnecting(attempt: consecutiveFailures, nextDelayMs: delay))
                try? await Task.sleep(for: .milliseconds(delay))
            }
            setPhase(.connecting)
            do {
                try await attemptOnce()
                consecutiveFailures = 0
            } catch {
                consecutiveFailures += 1
                setPhase(.degraded(describe(error)))
            }
        }
    }

    /// One spawn→ready→connect→watch cycle. Throws when the attempt failed.
    private func attemptOnce() async throws {
        let child = try spawnChild()
        self.child = child
        defer { if self.child === child { self.child = nil } }

        let readyLine = await readReadyLine(fd: child.readyReadFD, timeout: config.readyTimeout)
        guard let readyLine else {
            child.terminate()
            throw SupervisorError.noReadyLine
        }
        guard let ready = ReadyInfo.decode(readyLine) else {
            child.terminate()
            throw SupervisorError.readyLineMalformed
        }
        guard ready.socketPath == config.socketPath else {
            child.terminate()
            throw SupervisorError.socketMismatch(expected: config.socketPath, reported: ready.socketPath)
        }
        currentReadyInfo = ready

        let (lifecycle, lifecycleContinuation) = AsyncStream<SupervisorEvent>.makeStream()
        child.lifecycleContinuation = lifecycleContinuation
        let childWatcher = Task.detached { [weak child, lifecycleContinuation] in
            let status = await child?.awaitExit()
            if let status {
                lifecycleContinuation.yield(.childExited(status: status))
            }
            lifecycleContinuation.finish()
        }
        _ = childWatcher

        let connection = HostConnection(
            socketPath: config.socketPath,
            tokenProvider: { [config] in
                let tokenURL = config.dataDir.appendingPathComponent("host.token")
                let token = try String(contentsOf: tokenURL, encoding: .utf8)
                return token.trimmingCharacters(in: .whitespacesAndNewlines)
            }
        )
        await connection.setFaultSink { [weak self] fault in
            guard let self else { return }
            Task { await self.lifecycleNote(fault) }
        }
        await connection.setEventSink { _ in }
        await configureConnection(connection)
        self.connection = connection
        defer { if self.connection === connection { self.connection = nil } }

        do {
            try await connection.connect()
        } catch {
            await connection.close()
            child.terminate()
            throw error
        }

        setPhase(.ready)
        // Watch: first lifecycle event (fault or exit) ends the attempt.
        for await event in lifecycle {
            switch event {
            case let .connectionFault(fault):
                await connection.close()
                child.terminate()
                throw SupervisorError.connectionFailed(describe(fault))
            case let .childExited(status):
                await connection.close()
                throw SupervisorError.hostExited(status: Int(status))
            }
        }
        // Reaching here means the stream finished without an event — only
        // `stop()` does that (terminate → finish). The child was already
        // terminated there; nothing to clean up.
    }

    /// The fault sink hops through here (it cannot `await` directly).
    func lifecycleNote(_ fault: ConnectionError) {
        // Delivered via the active attempt's stream; if no attempt is active
        // (stop raced), it is ignored — the loop is already gone.
        child?.lifecycleContinuation?.yield(.connectionFault(fault))
    }

    private func setPhase(_ phase: SupervisorPhase) {
        guard self.phase != phase else { return }
        self.phase = phase
        onChange(phase)
    }

    // MARK: - Child process

    private func spawnChild() throws -> ChildHandle {
        let executable = config.cliExecutable ?? SupervisorConfig.defaultCLIURL()
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw SupervisorError.cliMissing(path: executable.path)
        }
        let argv: [String] = [
            "--data-dir", config.dataDir.path,
            "serve",
            "--ready-fd", "3",
        ]
        return try ChildHandle.spawn(executable: executable.path, arguments: argv,
                                     workingDirectory: config.dataDir.path,
                                     environment: config.environment)
    }

    private func readReadyLine(fd: Int32, timeout: Duration) async -> String? {
        // Non-blocking poll loop: the pipe has no reader-side timeout, so we
        // poll until a full line arrives or the deadline passes. 20 ms poll —
        // the line is written once at startup; tighter polling buys nothing.
        var flags = fcntl(fd, F_GETFL, 0)
        flags |= O_NONBLOCK
        _ = fcntl(fd, F_SETFL, flags)
        var buffer = Data()
        let deadline = Date().addingTimeInterval(
            Double(timeout.components.seconds) + Double(timeout.components.attoseconds) * 1e-18
        )
        while Date() < deadline, !Task.isCancelled {
            var chunk = [UInt8](repeating: 0, count: 4096)
            let n = chunk.withUnsafeMutableBytes { raw in
                read(fd, raw.baseAddress, 4096)
            }
            if n > 0 {
                buffer.append(contentsOf: chunk[0 ..< n])
                if let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                    return String(decoding: buffer[buffer.startIndex ..< newline], as: UTF8.self)
                }
                continue
            }
            if n == 0 {
                if !buffer.isEmpty {
                    return String(decoding: buffer, as: UTF8.self)
                }
                return nil
            }
            if errno == EAGAIN || errno == EWOULDBLOCK {
                try? await Task.sleep(for: .milliseconds(20))
                continue
            }
            if errno == EINTR { continue }
            return nil
        }
        return nil
    }
}

public enum SupervisorError: Error, Equatable, Sendable {
    case cliMissing(path: String)
    case noReadyLine
    case readyLineMalformed
    case socketMismatch(expected: String, reported: String)
    case connectionFailed(String)
    case hostExited(status: Int)
    case spawnFailed(String)
    case alreadyStopped

    public var message: String {
        switch self {
        case let .cliMissing(path): "host binary missing at \(path)"
        case .noReadyLine: "no ready line (timeout)"
        case .readyLineMalformed: "ready line not parseable"
        case let .socketMismatch(expected, reported): "ready socket path \(reported) ≠ configured \(expected)"
        case let .connectionFailed(reason): "connection failed: \(reason)"
        case let .hostExited(status): "host exited (status \(status))"
        case let .spawnFailed(reason): "spawn failed: \(reason)"
        case .alreadyStopped: "supervisor already stopped"
        }
    }
}

private func describe(_ error: Error) -> String {
    if let supervisorError = error as? SupervisorError { return supervisorError.message }
    if let connectionError = error as? ConnectionError {
        switch connectionError {
        case let .io(reason): return "io: \(reason)"
        case let .transport(fault): return "transport: \(fault)"
        case let .authDenied(message): return "auth denied: \(message)"
        case .disconnected: return "disconnected"
        case .timedOut: return "timed out"
        case .cancelled: return "cancelled"
        case .notConnected: return "not connected"
        case let .refused(failure): return "refused: \(failure.error.message)"
        }
    }
    return String(describing: error)
}

/// One supervised child: pid + ready-pipe read end. Exit polling and
/// termination live on a queue so the actor never blocks.
final class ChildHandle: @unchecked Sendable {
    let pid: pid_t
    let readyReadFD: Int32
    var lifecycleContinuation: AsyncStream<SupervisorEvent>.Continuation?
    /// `OSAllocatedUnfairLock` because `awaitExit` (an async context) reads
    /// this state; `NSLock.lock` is unavailable there.
    private struct GuardState {
        var exited = false
        var lastStatus: Int32 = 0
        var terminating = false
    }
    private let guardState = OSAllocatedUnfairLock(initialState: GuardState())

    private init(pid: pid_t, readyReadFD: Int32) {
        self.pid = pid
        self.readyReadFD = readyReadFD
    }

    enum SpawnFailure: Error { case posix(Int32) }

    static func spawn(executable: String,
                      arguments: [String],
                      workingDirectory: String,
                      environment: [String: String]) throws -> ChildHandle {
        var readyPipe: [Int32] = [0, 0]
        guard pipe(&readyPipe) == 0 else { throw SpawnFailure.posix(errno) }

        var argv = arguments
        argv.insert(executable, at: 0)
        var cArgs: [UnsafeMutablePointer<CChar>?] = argv.map { $0.withCString { strdup($0) } }
        cArgs.append(nil)
        defer { for p in cArgs where p != nil { free(p) } }

        var envVariables: [String] = ["PATH=/usr/bin:/bin:/usr/sbin:/sbin", "HOME=" + (NSHomeDirectory())]
        for (key, value) in environment { envVariables.append("\(key)=\(value)") }
        var cEnv: [UnsafeMutablePointer<CChar>?] = envVariables.map { $0.withCString { strdup($0) } }
        cEnv.append(nil)
        defer { for p in cEnv where p != nil { free(p) } }

        var spawnedPID = pid_t(0)
        // New SDK overlays map the `void *` typedefs to UnsafeMutableRawPointer:
        // the struct value forms no longer default-construct. Init allocates
        // through the inout pointer; destroy frees.
        var fileActions: UnsafeMutableRawPointer? = nil
        guard posix_spawn_file_actions_init(&fileActions) == 0, fileActions != nil else {
            close(readyPipe[1])
            close(readyPipe[0])
            throw SpawnFailure.posix(errno)
        }
        defer { posix_spawn_file_actions_destroy(&fileActions) }
        posix_spawn_file_actions_addclose(&fileActions, readyPipe[0])
        posix_spawn_file_actions_adddup2(&fileActions, readyPipe[1], 3)
        posix_spawn_file_actions_addclose(&fileActions, readyPipe[1])
        posix_spawn_file_actions_addopen(&fileActions, 1, "/dev/null", O_WRONLY, 0)
        // serve's stderr goes to /dev/null by default (P4-01 baseline). When
        // AERORIC_SERVE_STDERR_LOG is set (UI-test diagnostics only), it is
        // appended to that file instead — the 2026-10-01 "supervisor never
        // ready under testmanagerd" investigation had no way to see why the
        // child exited fast because stderr was unwritable-by-design.
        if let stderrLog = ProcessInfo.processInfo.environment["AERORIC_SERVE_STDERR_LOG"],
           !stderrLog.isEmpty {
            posix_spawn_file_actions_addopen(&fileActions, 2, stderrLog,
                                             O_WRONLY | O_CREAT | O_APPEND, 0o600)
        } else {
            posix_spawn_file_actions_addopen(&fileActions, 2, "/dev/null", O_WRONLY, 0)
        }
        posix_spawn_file_actions_addchdir_np(&fileActions, workingDirectory)

        var attributes: UnsafeMutableRawPointer? = nil
        guard posix_spawnattr_init(&attributes) == 0, attributes != nil else {
            posix_spawn_file_actions_destroy(&fileActions)
            close(readyPipe[1])
            close(readyPipe[0])
            throw SpawnFailure.posix(errno)
        }
        defer { posix_spawnattr_destroy(&attributes) }

        let result: Int32 = cArgs.withUnsafeBufferPointer { argvBuffer in
            cEnv.withUnsafeBufferPointer { envBuffer in
                posix_spawn(&spawnedPID, executable, &fileActions, &attributes,
                            argvBuffer.baseAddress, envBuffer.baseAddress)
            }
        }
        close(readyPipe[1])
        guard result == 0 else {
            close(readyPipe[0])
            throw SpawnFailure.posix(result)
        }
        return ChildHandle(pid: spawnedPID, readyReadFD: readyPipe[0])
    }

    /// Polls `waitpid(WNOHANG)` until the child exits (or we're cancelled).
    /// `ECHILD` means the reaper in `terminate` won the race — status is
    /// unknown but the child is gone.
    func awaitExit() async -> Int32? {
        while true {
            var status: Int32 = 0
            let result = waitpid(pid, &status, WNOHANG)
            if result == pid {
                let captured = status
                guardState.withLock {
                    $0.exited = true
                    $0.lastStatus = captured
                }
                return captured
            }
            if result == -1, errno == ECHILD {
                let status = guardState.withLock { state -> Int32 in
                    state.exited = true
                    return state.lastStatus
                }
                return status
            }
            if result == -1, errno == EINTR { continue }
            try? await Task.sleep(for: .milliseconds(200))
            if Task.isCancelled { return nil }
        }
    }

    /// SIGTERM → 2 s grace → SIGKILL → blocking reap (off-actor). Also closes
    /// the ready-pipe read end. Idempotent.
    func terminate() {
        let alreadyTerminating = guardState.withLock { state -> Bool in
            let wasTerminating = state.terminating
            state.terminating = true
            return wasTerminating
        }
        guard !alreadyTerminating else { return }
        kill(pid, SIGTERM)
        lifecycleContinuation?.finish()
        close(readyReadFD)
        let childPID = pid
        DispatchQueue.global(qos: .utility).async {
            let deadline = Date().addingTimeInterval(2)
            while Date() < deadline {
                var status: Int32 = 0
                if waitpid(childPID, &status, WNOHANG) != 0 { return }
                usleep(100_000)
            }
            kill(childPID, SIGKILL)
            var status: Int32 = 0
            while waitpid(childPID, &status, 0) == -1, errno == EINTR {}
        }
    }
}
