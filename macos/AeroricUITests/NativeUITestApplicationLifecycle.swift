import AppKit
import Darwin
import Foundation
#if !NATIVE_UI_LIFECYCLE_FIXTURES
import XCTest
#endif

/// Identity, not a PID, is the unit of custody. These decisions are also
/// compiled as pure fixtures in the dedicated VM; fixtures never query a process.
private struct NativeUITestProcessIdentity: Equatable {
    let pid: Int32
    let uid: UInt32
    let born: UInt64
    let executable: String
}

private struct NativeUITestReservation: Decodable {
    struct Directory: Decodable { let path: String; let dev: UInt64; let ino: UInt64 }
    let version: Int
    let runID: String
    let root: String
    let lease: String
    let ownerReceipt: String
    let directories: [Directory]

    func verify(runID expectedRun: String, root expectedRoot: String, receipts: String, executable: String) throws {
        let appSuffix = "/DerivedData/Build/Products/Debug/AeroricNative.app/Contents/MacOS/AeroricNative"
        guard version == 1, runID == expectedRun, root == expectedRoot, executable.hasSuffix(appSuffix),
              lease == String(executable.dropLast(appSuffix.count)) + "/ui-test-active",
              Set(directories.map(\.path)) == Set([root, root + "/data", root + "/warmup", receipts, lease]),
              directories.count == 5 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        for directory in directories {
            let fd = try verifiedDirectory(directory)
            close(fd)
        }
        guard let parent = directories.first(where: { $0.path == lease }) else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        let fd = try verifiedDirectory(parent)
        defer { close(fd) }
        let owner = openat(fd, "owner.json", O_RDONLY | O_NOFOLLOW)
        guard owner >= 0 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        defer { close(owner) }
        var info = stat()
        guard fstat(owner, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_uid == getuid(), info.st_size == ownerReceipt.utf8.count else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        var bytes = [UInt8](repeating: 0, count: ownerReceipt.utf8.count)
        let count = bytes.withUnsafeMutableBytes { Darwin.read(owner, $0.baseAddress, $0.count) }
        guard count == bytes.count, bytes == Array(ownerReceipt.utf8),
              let data = ownerReceipt.data(using: .utf8),
              let value = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              value["runID"] as? String == runID, value["root"] as? String == root else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
    }

    func verifiedDirectory(_ directory: Directory) throws -> Int32 {
        guard directory.path.hasPrefix("/"),
              URL(fileURLWithPath: directory.path).standardizedFileURL.path == directory.path else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        var fd = open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard fd >= 0 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        for part in directory.path.split(separator: "/") {
            let next = openat(fd, String(part), O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
            close(fd)
            guard next >= 0 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
            fd = next
        }
        var info = stat()
        guard fstat(fd, &info) == 0, UInt64(info.st_dev) == directory.dev, UInt64(info.st_ino) == directory.ino,
              info.st_uid == getuid() else {
            close(fd)
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        return fd
    }
}

private enum NativeUITestLifecycleFailure: Error {
    case unsafeEnvironment, processObservation, existingProcess, identityChanged
    case noOwnedWindow, quitUnavailable, didNotExit
}

private func nativeUITestMayAct(
    expected: NativeUITestProcessIdentity, actual: NativeUITestProcessIdentity?,
    applicationCount: Int, launchDateMatches: Bool
) -> Bool {
    expected.pid > 0 && expected.born > 0 && expected == actual
        && applicationCount == 1 && launchDateMatches
}

/// The real launch/quit implementation and the pure executable below use the
/// same state machine. A safety/observation failure is never retryable.
private final class NativeUITestCustody {
    enum Phase { case idle, launching, owned, quitting, blocked }
    enum Observation { case clear, occupied, failed }
    private(set) var phase = Phase.idle
    private(set) var teardownComplete = false
    func fail() { phase = .blocked }
    func beginLaunch(clear: Bool) throws {
        guard phase == .idle, !teardownComplete, clear else { fail(); throw NativeUITestLifecycleFailure.existingProcess }
        phase = .launching
    }
    func associate(valid: Bool) throws {
        guard phase == .launching, valid else { fail(); throw NativeUITestLifecycleFailure.identityChanged }
        phase = .owned
    }
    func beginQuit(ownerMatches: Bool, hasWindow: Bool) throws {
        guard phase == .owned, ownerMatches, hasWindow else {
            fail()
            throw NativeUITestLifecycleFailure.noOwnedWindow
        }
        phase = .quitting
    }
    func observeExit(_ observation: Observation) throws -> Bool {
        guard phase != .blocked, observation != .failed else {
            fail()
            throw NativeUITestLifecycleFailure.processObservation
        }
        return observation == .clear
    }
    func didExit() throws {
        guard phase == .owned || phase == .quitting else {
            fail()
            throw NativeUITestLifecycleFailure.identityChanged
        }
        phase = .idle
    }
    func finishTeardown() throws {
        guard phase == .idle else { fail(); throw NativeUITestLifecycleFailure.didNotExit }
        teardownComplete = true // Repeated teardown is inert, not another Quit or write.
    }
    func launchFlow(preflight: () throws -> Void, start: () throws -> Void,
                             associate: () throws -> Void) throws {
        do {
            try preflight()
            try beginLaunch(clear: true)
            try start()
            try associate()
            try self.associate(valid: true)
        } catch { fail(); throw error }
    }
    func teardownFlow(hasApplication: Bool, quit: () throws -> Void,
                               verifyClear: () throws -> Void, publish: () throws -> Void) throws {
        do {
            guard phase != .blocked && phase != .launching else {
                throw NativeUITestLifecycleFailure.identityChanged
            }
            if phase == .owned || phase == .quitting {
                guard hasApplication else { throw NativeUITestLifecycleFailure.identityChanged }
                try quit()
                phase = .idle
            }
            try verifyClear()
            if teardownComplete { return }
            try finishTeardown()
            try publish()
        } catch { fail(); throw error }
    }
    func awaitExit(timeout: TimeInterval, now: () -> TimeInterval, pump: () -> Void,
                   isClear: () throws -> Bool) throws {
        do {
            let deadline = now() + timeout
            repeat {
                if try observeExit(isClear() ? .clear : .occupied) { return }
                pump()
            } while now() < deadline
            if try observeExit(isClear() ? .clear : .occupied) { return }
            throw NativeUITestLifecycleFailure.didNotExit
        } catch { fail(); throw error }
    }
}

#if NATIVE_UI_LIFECYCLE_FIXTURES
@main
private enum NativeUITestLifecycleFixtures {
    static func main() throws {
        let own = NativeUITestProcessIdentity(pid: 41, uid: 501, born: 100, executable: "/fixture/App")
        precondition(nativeUITestMayAct(expected: own, actual: own, applicationCount: 1, launchDateMatches: true))
        for replacement in [
            NativeUITestProcessIdentity(pid: 42, uid: 501, born: 100, executable: "/fixture/App"),
            NativeUITestProcessIdentity(pid: 41, uid: 502, born: 100, executable: "/fixture/App"),
            NativeUITestProcessIdentity(pid: 41, uid: 501, born: 101, executable: "/fixture/App"),
            NativeUITestProcessIdentity(pid: 41, uid: 501, born: 100, executable: "/other/App"),
        ] {
            precondition(!nativeUITestMayAct(expected: own, actual: replacement, applicationCount: 1, launchDateMatches: true))
        }
        precondition(!nativeUITestMayAct(expected: own, actual: nil, applicationCount: 1, launchDateMatches: true))
        precondition(!nativeUITestMayAct(expected: own, actual: own, applicationCount: 2, launchDateMatches: true))
        precondition(!nativeUITestMayAct(expected: own, actual: own, applicationCount: 1, launchDateMatches: false))
        var clean = NativeUITestCustody()
        try clean.beginLaunch(clear: true)
        try clean.associate(valid: true)
        try clean.beginQuit(ownerMatches: true, hasWindow: true)
        let occupied = try clean.observeExit(.occupied)
        let exited = try clean.observeExit(.clear)
        precondition(!occupied && exited) // App gone / CLI retained must not count as clear.
        try clean.didExit()
        precondition(clean.phase == .idle) // Repeated teardown has no Quit action.
        try clean.beginLaunch(clear: true) // Internal relaunch requires successful exit.
        try clean.associate(valid: true)
        try clean.didExit() // Already naturally exited, with strict clear observation.
        try clean.finishTeardown()
        try clean.finishTeardown()
        precondition(clean.teardownComplete && clean.phase == .idle)
        for fault in ["baseline", "launch", "identity", "window", "observation", "quit", "assertion"] {
            var state = NativeUITestCustody()
            var launchActions = 0
            var quitActions = 0
            do {
                try state.beginLaunch(clear: fault != "baseline")
                launchActions += 1
                try state.associate(valid: fault != "launch")
                // An ordinary assertion failure still takes normal teardown.
                try state.beginQuit(ownerMatches: fault != "identity", hasWindow: fault != "window")
                quitActions += 1
                if fault == "quit" { state.fail(); throw NativeUITestLifecycleFailure.quitUnavailable }
                _ = try state.observeExit(fault == "observation" ? .failed : .clear)
                try state.didExit()
            } catch { state.fail() }
            if fault == "assertion" {
                precondition(state.phase == .idle && quitActions == 1)
            } else {
                precondition(state.phase == .blocked)
                do { try state.beginLaunch(clear: true); preconditionFailure("unsafe relaunch") } catch {}
                precondition(state.phase == .blocked)
            }
            if fault == "baseline" { precondition(launchActions == 0 && quitActions == 0) }
            if ["launch", "identity", "window"].contains(fault) { precondition(quitActions == 0) }
        }
        var late = NativeUITestCustody()
        late.fail()
        do { _ = try late.observeExit(.clear); preconditionFailure("error washed by late exit") } catch {}
        for fault in ["none", "start-interrupted", "assertion-abort", "CLI-timeout", "observation-error", "no-window"] {
            var state = NativeUITestCustody()
            var app = false
            var quitCalls = 0
            var receipts = 0
            var clock: TimeInterval = 0
            do {
                try state.launchFlow(preflight: {}, start: {
                    if fault == "start-interrupted" { throw NativeUITestLifecycleFailure.identityChanged }
                    app = true
                }, associate: {})
                if fault == "assertion-abort" { throw NativeUITestLifecycleFailure.quitUnavailable }
            } catch {
                if fault != "assertion-abort" { precondition(state.phase == .blocked) }
            }
            for _ in 0..<2 { // Actual teardown orchestration, including duplicate call.
                do {
                    try state.teardownFlow(hasApplication: app, quit: {
                        quitCalls += 1
                        if fault == "no-window" { throw NativeUITestLifecycleFailure.noOwnedWindow }
                        app = false
                        try state.awaitExit(timeout: 3, now: { clock }, pump: { clock += 1 }, isClear: {
                            if fault == "observation-error" { throw NativeUITestLifecycleFailure.processObservation }
                            return fault != "CLI-timeout"
                        })
                    }, verifyClear: {
                    }, publish: { receipts += 1 })
                } catch { precondition(state.phase == .blocked) }
            }
            if ["none", "assertion-abort"].contains(fault) {
                precondition(quitCalls == 1 && receipts == 1 && state.teardownComplete)
            } else {
                precondition(receipts == 0 && state.phase == .blocked)
                if fault == "start-interrupted" { precondition(quitCalls == 0 && !app) }
                if fault == "CLI-timeout" { precondition(clock == 3 && quitCalls == 1) }
            }
        }
        print("NativeUITestApplicationLifecycle: 23 pure identity/real-flow fixtures passed; no process access")
    }
}
#else
/// Ordinary product tests only. Review suites keep their own explicit custody.
/// Never calls XCUIApplication.terminate(), signals a child or deletes a lock.
@MainActor
final class NativeUITestApplicationLifecycle {
    private static let bundleID = "com.aeroric.native.dev"
    private static let nativeNames: Set<String> = ["AeroricNative", "aeroric-cli", "aeroric-host"]
    private var application: XCUIApplication?
    private var identity: NativeUITestProcessIdentity?
    private var started: Date?
    private var observedChildren = Set<Int32>()
    private var custody = NativeUITestCustody()
    private var resourceRoot: String?
    private var directoryIdentities: [String: [UInt64]] = [:]
    private var reservation: NativeUITestReservation?
    private var reservationContext: (String, String, String, String)?

    func launch(_ app: XCUIApplication) throws {
        do { try launchOwned(app) }
        catch { latchFailure(); throw error }
    }

    private func launchOwned(_ app: XCUIApplication) throws {
        guard custody.phase == .idle, application == nil else { throw NativeUITestLifecycleFailure.existingProcess }
        let environment = ProcessInfo.processInfo.environment
        guard environment["AERORIC_ENV"] == "ui-test",
              let root = environment["AERORIC_UI_TEST_RUN_ROOT"],
              let runID = environment["AERORIC_UI_TEST_RUN_ID"], UUID(uuidString: runID) != nil,
              let attemptID = environment["AERORIC_UI_TEST_ATTEMPT_ID"], UUID(uuidString: attemptID) != nil,
              let receiptRoot = environment["AERORIC_UI_TEST_RECEIPT_ROOT"],
              receiptRoot == NSHomeDirectory() + "/Library/Caches/aeroric-ui-lifecycle-" + runID,
              let data = environment["AERORIC_DATA_DIR"], data == root + "/data",
              let executable = environment["AERORIC_UI_TEST_APP_EXECUTABLE"],
              executable.hasSuffix("/Debug/AeroricNative.app/Contents/MacOS/AeroricNative"),
              let account = getpwuid(getuid()),
              let home = account.pointee.pw_dir else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        let caches = String(cString: home) + "/Library/Caches/aeroric-native/"
        guard root == caches + "ui-test-" + runID,
              URL(fileURLWithPath: root).resolvingSymlinksInPath().path == root,
              app.launchEnvironment["AERORIC_ENV"] == "ui-test",
              app.launchEnvironment["AERORIC_DATA_DIR"] == data else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        guard let raw = environment["AERORIC_UI_TEST_RESOURCE_RECEIPT"], raw.utf8.count <= 8192,
              let encoded = raw.data(using: .utf8) else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        let reserved = try JSONDecoder().decode(NativeUITestReservation.self, from: encoded)
        try reserved.verify(runID: runID, root: root, receipts: receiptRoot, executable: executable)
        reservation = reserved
        reservationContext = (runID, root, receiptRoot, executable)
        for directory in [root, data, root + "/warmup", receiptRoot] {
            let actual = try directoryIdentity(directory)
            if let expected = directoryIdentities[directory], actual != expected {
                throw NativeUITestLifecycleFailure.unsafeEnvironment
            }
            directoryIdentities[directory] = actual
        }
        if resourceRoot == nil {
            resourceRoot = receiptRoot
            try writeReceipt(root: receiptRoot, name: "lifecycle-start-\(attemptID).json", value: Data("{\"version\":1}\n".utf8))
        } else if resourceRoot != receiptRoot {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        // launch() can terminate a pre-existing instance. Check BEFORE calling it,
        // including after every method-internal relaunch.
        let began = Date()
        var state = custody
        defer { custody = state }
        try state.launchFlow(preflight: {
            try assertNoNativeProcesses()
            try verifyDirectories() // Includes the original reservation and lease, on every launch.
        }, start: {
            app.launchEnvironment["AERORIC_UI_TEST_RUN_ID"] = runID
            app.launch()
            application = app
        }, associate: {
            let apps = NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
            guard apps.count == 1, let live = apps.first, let date = live.launchDate,
                  date >= began.addingTimeInterval(-1), live.executableURL?.path == executable,
                  let actual = try processIdentity(live.processIdentifier), actual.uid == getuid(),
                  actual.executable == executable else { throw NativeUITestLifecycleFailure.identityChanged }
            identity = actual
            started = date
            try observeChildren(of: actual.pid)
        })
    }

    func quit(_ app: XCUIApplication) throws {
        do { try quitOwned(app) }
        catch { latchFailure(); throw error }
    }

    private func quitOwned(_ app: XCUIApplication) throws {
        guard custody.phase != .blocked else { throw NativeUITestLifecycleFailure.identityChanged }
        try verifyDirectories()
        guard let owned = application else {
            try assertNoNativeProcesses() // Idempotent, but never ignore a leaked child.
            return
        }
        guard owned === app, let expected = identity, let started else {
            throw NativeUITestLifecycleFailure.identityChanged
        }
        if try processIdentity(expected.pid) == nil {
            try assertNoNativeProcesses()
            try custody.didExit()
            clear()
            return
        }
        do {
            try assertOwner(expected, started: started)
            try observeChildren(of: expected.pid)
            try custody.beginQuit(ownerMatches: true, hasWindow: app.windows.count > 0)
            // XCUIApplication.activate() may relaunch a dead App. Activate only
            // this verified NSRunningApplication; this API never creates one.
            guard let live = NSRunningApplication(processIdentifier: expected.pid),
                  live.launchDate == started, live.executableURL?.path == expected.executable,
                  live.activate(options: []) else { throw NativeUITestLifecycleFailure.identityChanged }
            guard try wait(5, {
                NSWorkspace.shared.frontmostApplication?.processIdentifier == expected.pid
                    && app.state == .runningForeground
            }) else { throw NativeUITestLifecycleFailure.identityChanged }
            try assertOwner(expected, started: started)
            let menu = app.menuBars.menuBarItems.element(boundBy: 1)
            guard menu.exists && menu.isEnabled else { throw NativeUITestLifecycleFailure.quitUnavailable }
            menu.click()
            let items = menu.menus.menuItems.matching(NSPredicate(
                format: "label BEGINSWITH 'Quit ' OR title BEGINSWITH 'Quit ' OR label BEGINSWITH '退出' OR title BEGINSWITH '退出'"
            ))
            guard items.count == 1, items.firstMatch.isEnabled else {
                throw NativeUITestLifecycleFailure.quitUnavailable
            }
            try assertOwner(expected, started: started)
            items.firstMatch.click() // The real standard Quit command, not force termination.
            try custody.awaitExit(timeout: 15, now: { Date().timeIntervalSince1970 }, pump: {
                RunLoop.current.run(until: Date().addingTimeInterval(0.05))
            }, isClear: {
                let rows = try self.nativeProcesses()
                return rows.isEmpty && NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty
            })
            // A process reparented before observation is still caught globally.
            try assertNoNativeProcesses()
            try custody.didExit()
            clear()
        } catch {
            throw error // No next launch and no "cleanup" signal after a failed barrier.
        }
    }

    func tearDown() throws {
        guard let root = resourceRoot else { return } // Pure vocabulary test never launches.
        var state = custody
        defer { custody = state }
        do {
            try state.teardownFlow(hasApplication: application != nil, quit: {
                if let app = application { try quit(app) }
            }, verifyClear: {
                try verifyDirectories()
                try assertNoNativeProcesses()
            }, publish: {
                let environment = ProcessInfo.processInfo.environment
                guard let runID = environment["AERORIC_UI_TEST_RUN_ID"],
                      let attemptID = environment["AERORIC_UI_TEST_ATTEMPT_ID"],
                      UUID(uuidString: attemptID) != nil else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
                let value = try JSONSerialization.data(withJSONObject: [
                    "version": 1, "runID": runID, "attemptID": attemptID, "safeToRetry": true,
                ])
                try writeReceipt(root: root, name: "lifecycle-complete-\(attemptID).json", value: value)
            })
        } catch { latchFailure(); throw error }
    }

    private func clear() {
        application = nil
        identity = nil
        started = nil
        observedChildren.removeAll()
    }

    private func assertOwner(_ expected: NativeUITestProcessIdentity, started: Date) throws {
        let apps = NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
        guard nativeUITestMayAct(expected: expected, actual: try processIdentity(expected.pid),
                                 applicationCount: apps.count, launchDateMatches: apps.first?.launchDate == started),
              apps.first?.processIdentifier == expected.pid,
              apps.first?.executableURL?.path == expected.executable else {
            throw NativeUITestLifecycleFailure.identityChanged
        }
    }

    private func processIdentity(_ pid: Int32) throws -> NativeUITestProcessIdentity? {
        var info = proc_bsdinfo()
        let size = MemoryLayout<proc_bsdinfo>.stride
        let count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(size))
        if count == 0 && errno == ESRCH { return nil }
        guard count == size else { throw NativeUITestLifecycleFailure.processObservation }
        var buffer = [CChar](repeating: 0, count: 4096)
        let bytes = buffer.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
        guard bytes > 0 else { throw NativeUITestLifecycleFailure.processObservation }
        return NativeUITestProcessIdentity(pid: pid, uid: info.pbi_uid,
                                          born: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
                                          executable: String(cString: buffer))
    }

    private func processIDs(kind: UInt32, value: UInt32) throws -> [Int32] {
        var buffer = [Int32](repeating: 0, count: 16384)
        let count = buffer.withUnsafeMutableBytes {
            proc_listpids(kind, value, $0.baseAddress, Int32($0.count))
        }
        guard count >= 0, (kind != UInt32(PROC_UID_ONLY) || count > 0),
              count < buffer.count * MemoryLayout<Int32>.stride,
              Int(count) % MemoryLayout<Int32>.stride == 0 else {
            throw NativeUITestLifecycleFailure.processObservation
        }
        return buffer.prefix(Int(count) / MemoryLayout<Int32>.stride).filter { $0 > 0 }
    }

    private func nativeProcesses() throws -> [NativeUITestProcessIdentity] {
        var result: [NativeUITestProcessIdentity] = []
        // Outer unsandboxed runner/probe guards all users. Inside XCTest,
        // enumerate the complete dedicated UID (including reparented CLI),
        // never interpret protected system-process EPERM as an empty row.
        for pid in try processIDs(kind: UInt32(PROC_UID_ONLY), value: getuid()) {
            var name = [CChar](repeating: 0, count: 1024)
            let length = name.withUnsafeMutableBytes { proc_name(pid, $0.baseAddress, UInt32($0.count)) }
            if length <= 0 {
                // Only confirmed ESRCH is absence. Permission/read failures are
                // sticky failures, not an empty native-process observation.
                var info = proc_bsdinfo()
                errno = 0
                let count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(MemoryLayout<proc_bsdinfo>.stride))
                if count == 0 && errno == ESRCH { continue }
                throw NativeUITestLifecycleFailure.processObservation
            }
            guard Self.nativeNames.contains(String(cString: name)) else { continue }
            if let row = try processIdentity(pid) { result.append(row) }
        }
        return result
    }

    private func assertNoNativeProcesses() throws {
        guard NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty,
              try nativeProcesses().isEmpty else { throw NativeUITestLifecycleFailure.existingProcess }
        for pid in observedChildren {
            guard try processIdentity(pid) == nil else { throw NativeUITestLifecycleFailure.didNotExit }
        }
    }

    private func observeChildren(of root: Int32) throws {
        var pending = [root]
        var visited = Set<Int32>()
        while let pid = pending.popLast() {
            guard visited.count < 256 else { throw NativeUITestLifecycleFailure.processObservation }
            if !visited.insert(pid).inserted { continue }
            let children = try processIDs(kind: UInt32(PROC_PPID_ONLY), value: UInt32(pid))
            observedChildren.formUnion(children)
            pending.append(contentsOf: children)
        }
    }

    private func directoryIdentity(_ path: String) throws -> [UInt64] {
        var url = URL(fileURLWithPath: path)
        guard url.standardizedFileURL.path == path else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        var result: [UInt64] = []
        while url.path != "/" {
            var info = stat()
            guard lstat(url.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else {
                throw NativeUITestLifecycleFailure.unsafeEnvironment
            }
            result.append(UInt64(info.st_dev))
            result.append(UInt64(info.st_ino))
            url.deleteLastPathComponent()
        }
        return result
    }

    private func verifyDirectories() throws {
        if let reservation, let context = reservationContext {
            try reservation.verify(runID: context.0, root: context.1, receipts: context.2, executable: context.3)
        }
        for (path, expected) in directoryIdentities {
            guard try directoryIdentity(path) == expected else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        }
    }

    private func latchFailure() {
        custody.fail()
        print("AERORIC_UI_LIFECYCLE_FAILURE_V1") // Runner latches even when the sandbox refuses the receipt.
        guard let root = resourceRoot, (try? verifyDirectories()) != nil else { return }
        try? writeReceipt(root: root, name: "lifecycle-failure.json", value: Data("{\"version\":1,\"failed\":true}\n".utf8))
    }

    private func writeReceipt(root: String, name: String, value: Data) throws {
        guard let reservation, let directory = reservation.directories.first(where: { $0.path == root }) else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        let fd = try reservation.verifiedDirectory(directory)
        defer { close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0,
              let expected = directoryIdentities[root],
              expected.prefix(2).elementsEqual([UInt64(info.st_dev), UInt64(info.st_ino)]) else {
            throw NativeUITestLifecycleFailure.unsafeEnvironment
        }
        let marker = openat(fd, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard marker >= 0 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
        defer { close(marker) }
        guard value.withUnsafeBytes({ Darwin.write(marker, $0.baseAddress, $0.count) }) == value.count,
              fsync(marker) == 0 else { throw NativeUITestLifecycleFailure.unsafeEnvironment }
    }

    private func wait(_ timeout: TimeInterval, _ condition: () throws -> Bool) throws -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if try condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while Date() < deadline
        return try condition()
    }
}
#endif
