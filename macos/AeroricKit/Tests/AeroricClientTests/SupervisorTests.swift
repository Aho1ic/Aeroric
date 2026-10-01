@testable import AeroricClient
import XCTest
import Darwin

final class SupervisorTests: XCTestCase {
    private var supervisors: [HostSupervisor] = []

    override func tearDown() async throws {
        // Never leak fake-host processes between tests.
        for supervisor in supervisors {
            await supervisor.stop()
        }
        supervisors.removeAll()
    }

    // MARK: - Fake host

    /// Python stand-in for `aeroric-cli serve`: binds the UDS socket first,
    /// writes the ready line to fd 3, then answers JSONL requests. The bind
    /// must precede the ready write — the client connects immediately on ready.
    private func makeFakeHost() throws -> URL {
        let script = """
        #!/usr/bin/env python3
        import sys, os, json, socket
        args = sys.argv[1:]
        data_dir = args[args.index("--data-dir") + 1]
        sock_path = os.path.join(data_dir, "host.sock")
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        if os.path.exists(sock_path):
            os.unlink(sock_path)  # parity with the real host: rebind over a stale socket
        srv.bind(sock_path)
        srv.listen(1)
        with open(os.path.join(data_dir, "host.token"), "w") as token_file:
            token_file.write("test-token")
        ready = {"pid": os.getpid(), "startedAtMs": 1, "epoch": "e1", "revision": "fake",
                 "buildHash": "fake-build", "protocolHash": "fake-proto",
                 "dataDirHash": "fake-data", "socketPath": sock_path}
        os.write(3, json.dumps(ready).encode() + b"\\n")
        conn, _ = srv.accept()
        buf = b""
        while True:
            data = conn.recv(65536)
            if not data:
                break
            buf += data
            while b"\\n" in buf:
                line, buf = buf.split(b"\\n", 1)
                req = json.loads(line)
                if req.get("method") == "events.since":
                    result = {"events": [], "latestSeq": 0, "reset": False}
                else:
                    result = {"clientId": "fake-host"}
                reply = {"v": 1, "id": req["id"], "ok": True, "result": result}
                conn.sendall(json.dumps(reply).encode() + b"\\n")
        """
        let dir = NSTemporaryDirectory() + "sup-" + UUID().uuidString
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let fileURL = URL(fileURLWithPath: dir).appendingPathComponent("fake-host")
        try script.write(to: fileURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fileURL.path)
        return fileURL
    }

    private func makeSupervisor(fakeHost: URL?,
                                dataDir: URL,
                                readyTimeout: Duration = .seconds(5),
                                phaseSink: (@Sendable (SupervisorPhase) -> Void)? = nil) -> HostSupervisor {
        let config = SupervisorConfig(
            dataDir: dataDir,
            socketPath: dataDir.appendingPathComponent("host.sock").path,
            cliExecutable: fakeHost,
            readyTimeout: readyTimeout,
            policy: ReconnectPolicy(baseDelayMs: 50, capDelayMs: 200)
        )
        let supervisor = HostSupervisor(config: config, onChange: { phase in phaseSink?(phase) })
        supervisors.append(supervisor)
        return supervisor
    }

    private func waitForPhase(_ predicate: (SupervisorPhase) -> Bool,
                              on supervisor: HostSupervisor,
                              timeout: TimeInterval = 5) async -> SupervisorPhase {
        let deadline = Date().addingTimeInterval(timeout)
        var current = await supervisor.phase
        while Date() < deadline {
            if predicate(current) { return current }
            try? await Task.sleep(for: .milliseconds(50))
            current = await supervisor.phase
        }
        return current
    }

    private func makeDataDir() -> URL {
        let url = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("sup-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    // MARK: - Tests

    func testSpawnsFakeHostAndReachesReady() async throws {
        let dataDir = makeDataDir()
        let supervisor = makeSupervisor(fakeHost: try makeFakeHost(), dataDir: dataDir)
        await supervisor.start()
        let phase = await waitForPhase({ $0 == .ready }, on: supervisor)
        XCTAssertEqual(phase, .ready)
        let ready = await supervisor.currentReadyInfo
        XCTAssertEqual(ready?.epoch, "e1")
        XCTAssertEqual(ready?.socketPath, dataDir.appendingPathComponent("host.sock").path)
    }

    func testChildKillLeadsToReconnecting() async throws {
        let dataDir = makeDataDir()
        let recorded = Locked<[SupervisorPhase]>([])
        let supervisor = makeSupervisor(
            fakeHost: try makeFakeHost(),
            dataDir: dataDir,
            phaseSink: { phase in recorded.mutate { $0.append(phase) } }
        )
        await supervisor.start()
        let readyPhase = await waitForPhase({ $0 == .ready }, on: supervisor)
        XCTAssertEqual(readyPhase, .ready)
        let ready = await supervisor.currentReadyInfo
        let pid = Int32(ready?.pid ?? 0)
        XCTAssertGreaterThan(pid, 0)
        kill(pid, SIGKILL)
        // The child is gone; the supervisor must degrade, reconnect and spawn a
        // NEW process. The strongest observable proof is a ready phase whose
        // pid differs — attempt-count phases are sub-100ms windows.
        let deadline = Date().addingTimeInterval(8)
        var respawned = false
        while Date() < deadline {
            let info = await supervisor.currentReadyInfo
            if let info, info.pid != pid, await supervisor.phase == .ready {
                respawned = true
                break
            }
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTAssertTrue(respawned, "expected a respawned host with a new pid after SIGKILL")
        let degradedSeen = recorded.value.contains { phase in
            if case .degraded = phase { return true }
            return false
        }
        XCTAssertTrue(degradedSeen, "the kill must surface a degraded phase before the respawn")
    }

    func testStopKillsChildAndClearsReady() async throws {
        let dataDir = makeDataDir()
        let supervisor = makeSupervisor(fakeHost: try makeFakeHost(), dataDir: dataDir)
        await supervisor.start()
        let readyPhase = await waitForPhase({ $0 == .ready }, on: supervisor)
        XCTAssertEqual(readyPhase, .ready)
        let ready = await supervisor.currentReadyInfo
        let pid = Int32(ready?.pid ?? 0)
        XCTAssertGreaterThan(pid, 0)
        await supervisor.stop()
        let phase = await supervisor.phase
        XCTAssertEqual(phase, .stopped)
        let cleared = await supervisor.currentReadyInfo
        XCTAssertNil(cleared)
        // The reaper (SIGTERM → 2s → SIGKILL → blocking reap) must finish:
        // the pid stops existing entirely.
        let deadline = Date().addingTimeInterval(4)
        var gone = false
        while Date() < deadline {
            if kill(pid, 0) == -1 && errno == ESRCH { gone = true; break }
            try await Task.sleep(for: .milliseconds(100))
        }
        XCTAssertTrue(gone, "fake host pid \(pid) was still alive after stop")
    }

    func testNoReadyLineDegradesThenReconnects() async throws {
        let dataDir = makeDataDir()
        // /bin/sleep never writes a ready line; the pipe closes when it exits.
        let recorded = Locked<[SupervisorPhase]>([])
        let supervisor = makeSupervisor(
            fakeHost: URL(fileURLWithPath: "/bin/sleep"),
            dataDir: dataDir,
            readyTimeout: .milliseconds(500),
            phaseSink: { phase in
                recorded.mutate { $0.append(phase) }
            }
        )
        await supervisor.start()
        let reconnecting = await waitForPhase(
            { if case .reconnecting = $0 { return true }; return false },
            on: supervisor
        )
        guard case .reconnecting = reconnecting else {
            XCTFail("expected reconnecting, got \(reconnecting)")
            return
        }
        let degradedSeen = recorded.value.contains { phase in
            if case .degraded = phase { return true }
            return false
        }
        XCTAssertTrue(degradedSeen, "a failed attempt must pass through degraded before retrying")
    }
}
