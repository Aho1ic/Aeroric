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
    /// AP3: Optional expected build/protocol hashes for cold-start compatibility check.
    public var expectedBuildHash: String?
    public var expectedProtocolHash: String?
    /// AP3: When true, attaches to an already-online compatible background host before spawning.
    public var allowAttachToExistingHost: Bool

    public init(dataDir: URL,
                socketPath: String,
                cliExecutable: URL? = nil,
                environment: [String: String] = [:],
                readyTimeout: Duration = .seconds(15),
                policy: ReconnectPolicy = ReconnectPolicy(),
                expectedBuildHash: String? = nil,
                expectedProtocolHash: String? = nil,
                allowAttachToExistingHost: Bool = true) {
        self.dataDir = dataDir
        self.socketPath = socketPath
        self.cliExecutable = cliExecutable
        self.environment = environment
        self.readyTimeout = readyTimeout
        self.policy = policy
        self.expectedBuildHash = expectedBuildHash
        self.expectedProtocolHash = expectedProtocolHash
        self.allowAttachToExistingHost = allowAttachToExistingHost
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
/// Lock-protected slot holding the live child handle outside the actor, so a
/// synchronous terminate is available to `applicationWillTerminate` (which
/// runs on the main thread with NO async context; awaiting an actor from a
/// semaphore-blocked main thread deadlocks — measured 2026-10-01: the
/// semaphore waited out and every quit orphaned the serve, which then held
/// the data-dir lock against every later launch).
final class LiveChildSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var child: ChildHandle?

    func store(_ handle: ChildHandle?) {
        lock.lock()
        child = handle
        lock.unlock()
        Self.trace("store pid=\(handle?.pid ?? -1)")
    }

    static func trace(_ line: String) {
        let url = URL(fileURLWithPath: "/tmp/aeroric-slot-trace")
        let stamped = "\(Date().timeIntervalSince1970) \(line)\n"
        if let handle = try? FileHandle(forWritingTo: url) {
            defer { try? handle.close() }
            _ = try? handle.seekToEnd()
            _ = try? handle.write(contentsOf: Data(stamped.utf8))
        } else {
            try? Data(stamped.utf8).write(to: url)
        }
    }

    func load() -> ChildHandle? {
        lock.lock()
        defer { lock.unlock() }
        return child
    }
}

public actor HostSupervisor {
    public private(set) var phase: SupervisorPhase = .idle
    /// Ready line of the most recent successful spawn attempt (cleared on stop).
    public private(set) var currentReadyInfo: ReadyInfo?
    /// Mirror of the actor's `child`, for the synchronous process-exit path.
    private let liveChild = LiveChildSlot()
    private let config: SupervisorConfig
    private let onChange: @Sendable (SupervisorPhase) -> Void
    private let configureConnection: @Sendable (HostConnection) async -> Void
    private var runTask: Task<Void, Never>?
    private var child: ChildHandle?
    private var connection: HostConnection?
    private var attachedFaultContinuation: AsyncStream<SupervisorEvent>.Continuation?
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

    /// Synchronous SIGTERM for `applicationWillTerminate`: no async context
    /// exists there, and blocking the main thread on an actor call deadlocks
    /// (the orphaned-serve root cause, 2026-10-01). Reads the live child
    /// through the lock-protected slot and kills it; the actor's own
    /// bookkeeping is irrelevant at process exit.
    public nonisolated func terminateChildForProcessExit(keepAliveInBackground: Bool = false) {
        let child = liveChild.load()
        if keepAliveInBackground {
            LiveChildSlot.trace("processExit keepAliveInBackground=true pid=\(child?.pid ?? -1)")
            return
        }
        LiveChildSlot.trace("processExit terminate pid=\(child?.pid ?? -1)")
        child?.terminate()
    }

    /// Detaches the UI connection while leaving the background host process running.
    public func detachForBackgroundKeepAlive() async {
        stopped = true
        runTask?.cancel()
        runTask = nil
        attachedFaultContinuation?.finish()
        attachedFaultContinuation = nil
        await connection?.close()
        connection = nil
        setPhase(.stopped)
    }

    /// Terminates the child (SIGTERM, then SIGKILL after a grace period) and
    /// closes the connection. The run loop observes `stopped` and exits.
    public func stop() async {
        stopped = true
        runTask?.cancel()
        runTask = nil
        attachedFaultContinuation?.finish()
        attachedFaultContinuation = nil
        liveChild.store(nil)
        await connection?.close()
        connection = nil
        if let child {
            child.terminate()
        } else if let attachedPID = currentReadyInfo?.pid, attachedPID > 0 {
            kill(pid_t(attachedPID), SIGTERM)
        }
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
                if let incompatibility = checkRuntimeCompatibility() {
                    setPhase(.incompatible(incompatibility))
                    return
                }
                try await attemptOnce()
                consecutiveFailures = 0
            } catch let SupervisorError.incompatibleVersion(reason) {
                setPhase(.incompatible(reason))
                return
            } catch {
                consecutiveFailures += 1
                setPhase(.degraded(describe(error)))
            }
        }
    }

    private func checkRuntimeCompatibility() -> String? {
        let runtimeURL = config.dataDir.appendingPathComponent("runtime.json")
        guard let data = try? Data(contentsOf: runtimeURL),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        if let schemaVersion = obj["schemaVersion"] as? Int, schemaVersion > 1 {
            return "runtime schemaVersion \(schemaVersion) requires a newer client (supported: 1)"
        }
        if let expectedProto = config.expectedProtocolHash,
           let actualProto = obj["protocolHash"] as? String,
           actualProto != expectedProto {
            return "host protocolHash \(actualProto) != expected \(expectedProto); coordinated restart required"
        }
        if let expectedBuild = config.expectedBuildHash,
           let actualBuild = obj["buildHash"] as? String,
           actualBuild != expectedBuild {
            return "host buildHash \(actualBuild) != expected \(expectedBuild); coordinated restart required"
        }
        return nil
    }

    private func tryAttachExistingHost() async -> Bool {
        guard config.allowAttachToExistingHost,
              FileManager.default.fileExists(atPath: config.socketPath),
              FileManager.default.fileExists(atPath: config.dataDir.appendingPathComponent("host.token").path) else {
            return false
        }
        let connection = HostConnection(
            socketPath: config.socketPath,
            tokenProvider: { [config] in
                let tokenURL = config.dataDir.appendingPathComponent("host.token")
                let token = try String(contentsOf: tokenURL, encoding: .utf8)
                return token.trimmingCharacters(in: .whitespacesAndNewlines)
            }
        )
        let (lifecycle, lifecycleContinuation) = AsyncStream<SupervisorEvent>.makeStream()
        self.attachedFaultContinuation = lifecycleContinuation
        defer { self.attachedFaultContinuation = nil }
        await connection.setFaultSink { [weak self] fault in
            guard let self else { return }
            Task { await self.lifecycleNote(fault) }
        }
        await connection.setEventSink { _ in }
        await configureConnection(connection)
        do {
            try await connection.connect()
        } catch {
            await connection.close()
            return false
        }
        self.connection = connection
        defer { if self.connection === connection { self.connection = nil } }
        if let runtimeData = try? Data(contentsOf: config.dataDir.appendingPathComponent("runtime.json")),
           let obj = try? JSONSerialization.jsonObject(with: runtimeData) as? [String: Any] {
            let pid = (obj["pid"] as? Int) ?? 0
            let epoch = (obj["epoch"] as? String) ?? "attached"
            let rev = (obj["revision"] as? String) ?? "1"
            let buildHash = (obj["buildHash"] as? String) ?? "attached"
            let protoHash = (obj["protocolHash"] as? String) ?? "protocol-v1"
            currentReadyInfo = ReadyInfo(
                pid: pid,
                startedAtMs: 0,
                epoch: epoch,
                revision: rev,
                buildHash: buildHash,
                protocolHash: protoHash,
                dataDirHash: "",
                socketPath: config.socketPath
            )
        }
        setPhase(.ready)
        for await event in lifecycle {
            switch event {
            case .connectionFault, .childExited:
                await connection.close()
                return true
            }
        }
        return true
    }

    /// One spawn→ready→connect→watch cycle. Throws when the attempt failed.
    private func attemptOnce() async throws {
        if await tryAttachExistingHost() {
            return
        }
        let child = try spawnChild()
        self.child = child
        liveChild.store(child)
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
        if let expectedProto = config.expectedProtocolHash, ready.protocolHash != expectedProto {
            child.terminate()
            throw SupervisorError.incompatibleVersion("ready protocolHash \(ready.protocolHash) != expected \(expectedProto)")
        }
        if let expectedBuild = config.expectedBuildHash, ready.buildHash != expectedBuild {
            child.terminate()
            throw SupervisorError.incompatibleVersion("ready buildHash \(ready.buildHash) != expected \(expectedBuild)")
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
        child?.lifecycleContinuation?.yield(.connectionFault(fault))
        attachedFaultContinuation?.yield(.connectionFault(fault))
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
    case incompatibleVersion(String)

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
        case let .incompatibleVersion(reason): "incompatible host version: \(reason)"
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
        // The child inherits the SPAWNING THREAD's signal mask. Swift
        // Concurrency / AppKit worker threads block delivery signals, and a
        // serve spawned from one of those threads inherits a mask with
        // SIGTERM blocked — the signal then sits pending forever and the
        // child cannot be terminated (measured 2026-10-01: SIGTERM was a
        // no-op on such a serve, both from the supervisor and manually;
        // SIGKILL was the only thing that worked, and the hung serve held
        // the data-dir lock against every later spawn). Reset the mask to
        // empty and the terminate signals to their defaults so the child is
        // always killable, no matter which thread ran this spawn.
        var inheritMask = sigset_t()
        sigemptyset(&inheritMask)
        posix_spawnattr_setsigmask(&attributes, &inheritMask)
        var defaultSignals = sigset_t()
        sigemptyset(&defaultSignals)
        for signalNumber in [SIGTERM, SIGINT, SIGHUP, SIGQUIT] {
            sigaddset(&defaultSignals, signalNumber)
        }
        posix_spawnattr_setsigdefault(&attributes, &defaultSignals)
        posix_spawnattr_setflags(&attributes,
                                 Int16(POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK))

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
