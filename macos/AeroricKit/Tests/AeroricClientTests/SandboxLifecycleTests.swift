@testable import AeroricClient
import XCTest
import Darwin

final class SandboxLifecycleTests: XCTestCase {
    private var supervisors: [HostSupervisor] = []

    override func tearDown() async throws {
        for supervisor in supervisors {
            await supervisor.stop()
        }
        supervisors.removeAll()
    }

    private func makeScratchDir() -> URL {
        let url = URL(fileURLWithPath: "/tmp")
            .appendingPathComponent("ap3-\(UUID().uuidString.prefix(8))", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    // MARK: - 1. Sandbox Data Root & sun_path budget

    func testSandboxDataRootEnforcesSunPathLimitAndAvoidsDoubleNesting() {
        let containerHome = "/Users/macbook/Library/Containers/com.aeroric.native.dev/Data"
        let resolved = SandboxDataRoot.resolve(
            injectedPath: nil,
            appGroupIdentifier: nil,
            homeDirectory: containerHome,
            bundleIdentifier: "com.aeroric.native.dev"
        )
        XCTAssertEqual(resolved.path, "\(containerHome)/.aeroric")
        let sock = SandboxDataRoot.socketPath(for: resolved)
        XCTAssertLessThan(sock.utf8.count, SandboxDataRoot.sunPathLimit)

        // Ultra-long path fallback still respects the 104B kernel sun_path limit.
        let deepPath = String(repeating: "/very-long-segment", count: 8)
        let deepSock = SandboxDataRoot.socketPath(for: URL(fileURLWithPath: deepPath, isDirectory: true))
        XCTAssertLessThan(deepSock.utf8.count, SandboxDataRoot.sunPathLimit)
        XCTAssertTrue(deepSock.hasPrefix("/tmp/aeroric-"))
    }

    // MARK: - 2. Security-Scoped Bookmarks (Persist, Restart, Stale Refresh, Revoke)

    private final class MockBookmarkDriver: BookmarkDriver, @unchecked Sendable {
        var simulateStale = false
        var allowAccess = true
        var stoppedPaths: [String] = []

        func createBookmarkData(for url: URL) throws -> Data {
            Data(url.standardizedFileURL.path.utf8)
        }

        func resolveBookmarkData(_ data: Data) throws -> (url: URL, isStale: Bool) {
            guard let path = String(data: data, encoding: .utf8), !path.isEmpty else {
                throw NSError(domain: "MockBookmark", code: 2)
            }
            return (URL(fileURLWithPath: path, isDirectory: true), simulateStale)
        }

        func startAccessing(_ url: URL) -> Bool {
            allowAccess
        }

        func stopAccessing(_ url: URL) {
            stoppedPaths.append(url.standardizedFileURL.path)
        }
    }

    func testSecurityScopedBookmarkPersistenceRestartRefreshAndRevocation() throws {
        let dataDir = makeScratchDir()
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let projectDir = dataDir.appendingPathComponent("authorized-repo", isDirectory: true)
        try FileManager.default.createDirectory(at: projectDir, withIntermediateDirectories: true)

        let driver = MockBookmarkDriver()
        let store1 = SandboxBookmarkStore(dataDir: dataDir, driver: driver)
        let state1 = try store1.registerAuthorizedDirectory(projectDir)
        XCTAssertEqual(state1, .active(path: projectDir.standardizedFileURL.path, refreshedStale: false))
        XCTAssertTrue(FileManager.default.fileExists(atPath: store1.storageURL.path))

        // Simulate app restart with stale bookmark refresh.
        driver.simulateStale = true
        let store2 = SandboxBookmarkStore(dataDir: dataDir, driver: driver)
        let restored = store2.restoreAllBookmarks()
        XCTAssertEqual(
            restored[projectDir.standardizedFileURL.path],
            .active(path: projectDir.standardizedFileURL.path, refreshedStale: true)
        )

        // Simulate directory removal / access revocation on next restart.
        try FileManager.default.removeItem(at: projectDir)
        let store3 = SandboxBookmarkStore(dataDir: dataDir, driver: driver)
        let revokedMap = store3.restoreAllBookmarks()
        guard case .revoked = revokedMap[projectDir.standardizedFileURL.path] else {
            XCTFail("Expected .revoked when target directory no longer exists, got \(String(describing: revokedMap))")
            return
        }
    }

    // MARK: - 3. Version Mismatch & Missing Helper

    func testSupervisorIncompatibleRuntimeRefusesWithoutKillingOrRespawning() async throws {
        let dataDir = makeScratchDir()
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let runtimeURL = dataDir.appendingPathComponent("runtime.json")
        let futureRuntime = """
        {"schemaVersion":99,"epoch":"future-epoch","revision":"1","buildHash":"future","protocolHash":"protocol-v99"}
        """
        try futureRuntime.write(to: runtimeURL, atomically: true, encoding: .utf8)

        let config = SupervisorConfig(
            dataDir: dataDir,
            socketPath: SandboxDataRoot.socketPath(for: dataDir),
            cliExecutable: URL(fileURLWithPath: "/usr/bin/true")
        )
        let supervisor = HostSupervisor(config: config, onChange: { _ in })
        supervisors.append(supervisor)
        await supervisor.start()

        let deadline = Date().addingTimeInterval(3)
        var finalPhase = await supervisor.phase
        while Date() < deadline {
            if case .incompatible = finalPhase { break }
            try? await Task.sleep(for: .milliseconds(40))
            finalPhase = await supervisor.phase
        }
        guard case let .incompatible(reason) = finalPhase else {
            XCTFail("Expected .incompatible phase, got \(finalPhase)")
            return
        }
        XCTAssertTrue(reason.contains("schemaVersion 99"))
        let onDiskAfter = try String(contentsOf: runtimeURL, encoding: .utf8)
        XCTAssertEqual(onDiskAfter, futureRuntime, "Incompatible runtime.json must not be overwritten")
    }

    func testMissingHelperReportsCliMissingDegradation() async throws {
        let dataDir = makeScratchDir()
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let missingCLI = dataDir.appendingPathComponent("missing-aeroric-cli")
        let config = SupervisorConfig(
            dataDir: dataDir,
            socketPath: SandboxDataRoot.socketPath(for: dataDir),
            cliExecutable: missingCLI,
            policy: ReconnectPolicy(baseDelayMs: 100, capDelayMs: 200, maxAttempts: 1)
        )
        let recorded = Locked<[SupervisorPhase]>([])
        let supervisor = HostSupervisor(
            config: config,
            onChange: { phase in recorded.mutate { $0.append(phase) } }
        )
        supervisors.append(supervisor)
        await supervisor.start()

        let deadline = Date().addingTimeInterval(3)
        while Date() < deadline {
            let sawMissing = recorded.value.contains { phase in
                if case let .degraded(msg) = phase, msg.contains("host binary missing") {
                    return true
                }
                return false
            }
            if sawMissing { break }
            try? await Task.sleep(for: .milliseconds(40))
        }
        let sawMissing = recorded.value.contains { phase in
            if case let .degraded(msg) = phase, msg.contains("host binary missing") {
                return true
            }
            return false
        }
        XCTAssertTrue(sawMissing, "Missing helper binary must report degraded cliMissing")
    }

    // MARK: - 4. Opt-in Background Host Keep-Alive, Reattach & Explicit Stop

    private func makeMultiAcceptFakeHost() throws -> URL {
        let script = """
        #!/usr/bin/env python3
        import sys, os, json, socket, signal
        args = sys.argv[1:]
        data_dir = args[args.index("--data-dir") + 1]
        sock_path = os.path.join(data_dir, "host.sock")
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        if os.path.exists(sock_path):
            os.unlink(sock_path)
        srv.bind(sock_path)
        srv.listen(4)
        with open(os.path.join(data_dir, "host.token"), "w") as tf:
            tf.write("bg-token\\n")
        runtime = {"schemaVersion": 1, "pid": os.getpid(), "epoch": "bg-epoch-1", "revision": "1",
                   "buildHash": "b1", "protocolHash": "protocol-v1"}
        with open(os.path.join(data_dir, "runtime.json"), "w") as rf:
            json.dump(runtime, rf)
        ready = {"pid": os.getpid(), "startedAtMs": 1, "epoch": "bg-epoch-1", "revision": "1",
                 "buildHash": "b1", "protocolHash": "protocol-v1",
                 "dataDirHash": "d1", "socketPath": sock_path}
        os.write(3, json.dumps(ready).encode() + b"\\n")
        signal.signal(signal.SIGTERM, lambda s, f: sys.exit(0))
        while True:
            conn, _ = srv.accept()
            buf = b""
            try:
                while True:
                    data = conn.recv(65536)
                    if not data:
                        break
                    buf += data
                    while b"\\n" in buf:
                        line, buf = buf.split(b"\\n", 1)
                        req = json.loads(line)
                        if req.get("method") == "events.since":
                            res = {"events": [], "latestSeq": 0, "reset": False}
                        else:
                            res = {"clientId": "bg-host"}
                        reply = {"v": 1, "id": req["id"], "ok": True, "result": res}
                        conn.sendall(json.dumps(reply).encode() + b"\\n")
            except Exception:
                pass
            finally:
                conn.close()
        """
        let dir = makeScratchDir()
        let fileURL = dir.appendingPathComponent("fake-bg-host")
        try script.write(to: fileURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fileURL.path)
        return fileURL
    }

    func testBackgroundKeepAliveOptInPreservesHostOnQuitAndReattachesThenExplicitStopTerminates() async throws {
        let dataDir = makeScratchDir()
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let fakeHost = try makeMultiAcceptFakeHost()
        defer { try? FileManager.default.removeItem(at: fakeHost.deletingLastPathComponent()) }

        // Policy defaults to false, and separates keepAliveOnUIQuit from remoteEnabled.
        var policy = BackgroundHostPolicy.load(from: dataDir)
        XCTAssertFalse(policy.keepAliveOnUIQuit)
        XCTAssertFalse(policy.remoteEnabled)
        policy.keepAliveOnUIQuit = true
        try policy.save(to: dataDir)
        XCTAssertTrue(BackgroundHostPolicy.load(from: dataDir).keepAliveOnUIQuit)
        XCTAssertFalse(BackgroundHostPolicy.load(from: dataDir).remoteEnabled)

        let sockPath = SandboxDataRoot.socketPath(for: dataDir)
        let sup1 = HostSupervisor(
            config: SupervisorConfig(dataDir: dataDir, socketPath: sockPath, cliExecutable: fakeHost),
            onChange: { _ in }
        )
        supervisors.append(sup1)
        await sup1.start()

        let deadline1 = Date().addingTimeInterval(5)
        while Date() < deadline1, await sup1.phase != .ready {
            try? await Task.sleep(for: .milliseconds(40))
        }
        let phase1 = await sup1.phase
        XCTAssertEqual(phase1, .ready)
        let hostPID = Int32(await sup1.currentReadyInfo?.pid ?? 0)
        XCTAssertGreaterThan(hostPID, 0)

        // Simulate UI quit WITH keepAliveInBackground = true: host process must survive!
        sup1.terminateChildForProcessExit(keepAliveInBackground: true)
        await sup1.detachForBackgroundKeepAlive()
        XCTAssertEqual(kill(hostPID, 0), 0, "Background host must stay alive when keepAliveInBackground is true")

        // Second UI launch reattaches to the surviving host WITHOUT spawning a new child!
        let sup2 = HostSupervisor(
            config: SupervisorConfig(
                dataDir: dataDir,
                socketPath: sockPath,
                cliExecutable: URL(fileURLWithPath: "/nonexistent/should-not-spawn")
            ),
            onChange: { _ in }
        )
        supervisors.append(sup2)
        await sup2.start()

        let deadline2 = Date().addingTimeInterval(5)
        while Date() < deadline2, await sup2.phase != .ready {
            try? await Task.sleep(for: .milliseconds(40))
        }
        let phase2 = await sup2.phase
        XCTAssertEqual(phase2, .ready, "Second supervisor must attach to surviving background host")
        let attachedPID = Int32(await sup2.currentReadyInfo?.pid ?? 0)
        XCTAssertEqual(attachedPID, hostPID)

        // Explicit stop terminates the background host!
        await sup2.stop()
        let deadline3 = Date().addingTimeInterval(3)
        var gone = false
        while Date() < deadline3 {
            if kill(hostPID, 0) == -1 && errno == ESRCH { gone = true; break }
            try? await Task.sleep(for: .milliseconds(50))
        }
        XCTAssertTrue(gone, "Explicit stop must terminate the background host")
    }
}
