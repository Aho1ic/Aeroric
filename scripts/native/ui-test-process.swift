import AppKit
import Darwin
import Foundation

// Runner-local public-API tool. No arbitrary PID/quit command and no signals.
// --fixtures uses only values, before any filesystem, AppKit or process access.
private struct Identity: Equatable {
    let pid: Int32
    let uid: UInt32
    let born: UInt64
    let executable: String
}
private enum Failure: Error { case unsafe, observation, occupied, launch, identity, quit, pending }
private struct ResourceReceipt: Decodable {
    struct Directory: Decodable { let path: String; let dev: UInt64; let ino: UInt64 }
    let version: Int
    let runID: String
    let root: String
    let lease: String
    let ownerReceipt: String
    let directories: [Directory]
    func verify(app: String, root expectedRoot: String, runID expectedRun: String) throws {
        let suffix = "/DerivedData/Build/Products/Debug/AeroricNative.app"
        let receipts = NSHomeDirectory() + "/Library/Containers/com.aeroric.native.dev.uitests.xctrunner/Data/Library/Caches/aeroric-ui-lifecycle-" + expectedRun
        guard version == 1, root == expectedRoot, runID == expectedRun, app.hasSuffix(suffix),
              lease == String(app.dropLast(suffix.count)) + "/ui-test-active",
              Set(directories.map(\.path)) == Set([root, root + "/data", root + "/warmup", receipts, lease]),
              directories.count == 5 else { throw Failure.unsafe }
        for directory in directories { let fd = try openDirectory(directory); close(fd) }
        guard let directory = directories.first(where: { $0.path == lease }) else { throw Failure.unsafe }
        let fd = try openDirectory(directory)
        defer { close(fd) }
        let owner = openat(fd, "owner.json", O_RDONLY | O_NOFOLLOW)
        guard owner >= 0 else { throw Failure.unsafe }
        defer { close(owner) }
        var info = stat()
        guard fstat(owner, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == getuid(),
              info.st_size == ownerReceipt.utf8.count else { throw Failure.unsafe }
        var bytes = [UInt8](repeating: 0, count: ownerReceipt.utf8.count)
        let size = bytes.withUnsafeMutableBytes { Darwin.read(owner, $0.baseAddress, $0.count) }
        guard size == bytes.count, bytes == Array(ownerReceipt.utf8),
              let data = ownerReceipt.data(using: .utf8),
              let value = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              value["runID"] as? String == runID, value["root"] as? String == root else { throw Failure.unsafe }
    }
    func openDirectory(_ directory: Directory) throws -> Int32 {
        guard directory.path.hasPrefix("/"),
              URL(fileURLWithPath: directory.path).standardizedFileURL.path == directory.path else { throw Failure.unsafe }
        var fd = open("/", O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard fd >= 0 else { throw Failure.unsafe }
        for part in directory.path.split(separator: "/") {
            let next = openat(fd, String(part), O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
            close(fd)
            guard next >= 0 else { throw Failure.unsafe }
            fd = next
        }
        var info = stat()
        guard fstat(fd, &info) == 0, UInt64(info.st_dev) == directory.dev, UInt64(info.st_ino) == directory.ino,
              info.st_uid == getuid() else { close(fd); throw Failure.unsafe }
        return fd
    }
}
private struct WarmupCustody {
    enum Phase { case initial, launching, associated, quitting, complete, blocked }
    enum Observation { case clear, occupied, failed }
    private(set) var phase = Phase.initial
    mutating func fail() { phase = .blocked }
    mutating func begin(clear: Bool) throws {
        guard phase == .initial, clear else { fail(); throw Failure.occupied }
        phase = .launching
    }
    mutating func associate(valid: Bool) throws {
        guard phase == .launching, valid else { fail(); throw Failure.launch }
        phase = .associated
    }
    mutating func requestQuit(valid: Bool) throws {
        guard phase == .associated, valid else { fail(); throw Failure.identity }
        phase = .quitting
    }
    mutating func observeExit(_ observation: Observation) throws -> Bool {
        guard phase == .quitting, observation != .failed else { fail(); throw Failure.observation }
        if observation == .clear { phase = .complete; return true }
        return false
    }
}
private final class LaunchReceipt: @unchecked Sendable {
    private let lock = NSLock()
    private var value: (NSRunningApplication?, Bool)?
    func publish(_ application: NSRunningApplication?, failed: Bool) {
        lock.lock()
        defer { lock.unlock() }
        value = (application, failed)
    }
    func read() -> (NSRunningApplication?, Bool)? {
        lock.lock()
        defer { lock.unlock() }
        return value
    }
}
private let names: Set<String> = [
    "AeroricNative", "aeroric-cli", "aeroric-host", "xcodebuild", "xctest", "AeroricNativeUITests-Runner",
]

private func mayQuit(expected: Identity, actual: Identity?, causal: Bool,
                     root: String, environment: [String: String], runID: String) -> Bool {
    causal && expected.pid > 0 && expected.born > 0 && expected == actual
        && environment["AERORIC_ENV"] == "ui-test"
        && environment["AERORIC_DATA_DIR"] == root + "/warmup"
        && environment["AERORIC_UI_TEST_RUN_ID"] == runID
}

private func fixtures() throws {
    let own = Identity(pid: 9, uid: 501, born: 12, executable: "/fixture/App")
    let root = "/fixture/root"
    let env = ["AERORIC_ENV": "ui-test", "AERORIC_DATA_DIR": root + "/warmup", "AERORIC_UI_TEST_RUN_ID": "fixture"]
    precondition(mayQuit(expected: own, actual: own, causal: true, root: root, environment: env, runID: "fixture"))
    for replacement in [
        Identity(pid: 10, uid: 501, born: 12, executable: "/fixture/App"),
        Identity(pid: 9, uid: 502, born: 12, executable: "/fixture/App"),
        Identity(pid: 9, uid: 501, born: 13, executable: "/fixture/App"),
        Identity(pid: 9, uid: 501, born: 12, executable: "/wrong/App"),
    ] {
        precondition(!mayQuit(expected: own, actual: replacement, causal: true, root: root, environment: env, runID: "fixture"))
    }
    precondition(!mayQuit(expected: own, actual: nil, causal: true, root: root, environment: env, runID: "fixture"))
    precondition(!mayQuit(expected: own, actual: own, causal: false, root: root, environment: env, runID: "fixture"))
    precondition(!mayQuit(expected: own, actual: own, causal: true, root: "/wrong", environment: env, runID: "fixture"))
    precondition(!mayQuit(expected: own, actual: own, causal: true, root: root, environment: [:], runID: "fixture"))
    precondition(!mayQuit(expected: own, actual: own, causal: true, root: root, environment: env, runID: "wrong"))
    for fault in ["none", "baseline", "callback", "identity", "quit", "child", "observation"] {
        var state = WarmupCustody()
        var launches = 0
        var quits = 0
        do {
            try state.begin(clear: fault != "baseline")
            launches += 1
            try state.associate(valid: fault != "callback")
            try state.requestQuit(valid: fault != "identity")
            quits += 1
            if fault == "quit" { throw Failure.quit }
            if fault == "child" {
                let exited = try state.observeExit(.occupied)
                precondition(!exited && state.phase == .quitting)
                throw Failure.pending
            }
            _ = try state.observeExit(fault == "observation" ? .failed : .clear)
        } catch { state.fail() }
        precondition(state.phase == (fault == "none" ? .complete : .blocked))
        if fault == "baseline" { precondition(launches == 0 && quits == 0) }
        if ["callback", "identity"].contains(fault) { precondition(quits == 0) }
        if fault != "none" {
            do { try state.begin(clear: true); preconditionFailure("unsafe restart") } catch {}
        }
    }
    print("ui-test-process: 17 pure identity/normal-Quit state-flow fixtures passed; no process access")
}

private func identity(_ pid: Int32) throws -> Identity? {
    var info = proc_bsdinfo()
    let size = MemoryLayout<proc_bsdinfo>.stride
    let count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(size))
    if count == 0 && errno == ESRCH { return nil }
    guard count == size else { throw Failure.observation }
    var buffer = [CChar](repeating: 0, count: 4096)
    guard buffer.withUnsafeMutableBytes({ proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }) > 0 else {
        throw Failure.observation
    }
    return Identity(pid: pid, uid: info.pbi_uid,
                    born: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
                    executable: String(cString: buffer))
}

private func selectedProcesses() throws -> [Identity] {
    // proc_name is intentionally denied for many unrelated protected system
    // processes. Use the public all-user ps identity table to select names,
    // then require full kernel identity for every selected native process.
    // No argv/environment is requested and unrelated rows are never emitted.
    let process = Process()
    let pipe = Pipe()
    process.executableURL = URL(fileURLWithPath: "/bin/ps")
    process.arguments = ["-ww", "-axo", "pid=,comm="]
    process.standardOutput = pipe
    process.standardError = FileHandle.nullDevice
    try process.run()
    let data = pipe.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    guard process.terminationStatus == 0, let text = String(data: data, encoding: .utf8),
          !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw Failure.observation }
    var result: [Identity] = []
    var seen = Set<Int32>()
    for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
        let fields = line.split(maxSplits: 1, whereSeparator: { $0 == " " || $0 == "\t" })
        guard fields.count == 2, let pid = Int32(fields[0]), pid > 0, seen.insert(pid).inserted else {
            throw Failure.observation
        }
        let path = String(fields[1]).trimmingCharacters(in: .whitespaces)
        guard names.contains(URL(fileURLWithPath: path).lastPathComponent) else { continue }
        if let row = try identity(pid) {
            guard row.executable == path else { throw Failure.observation }
            result.append(row)
        }
    }
    return result
}

private func snapshot() throws -> [String: Any] {
    let apps = NSRunningApplication.runningApplications(withBundleIdentifier: "com.aeroric.native.dev")
    let rows = try selectedProcesses()
    return [
        "schemaVersion": 1, "errors": [], "observedAt": Date().timeIntervalSince1970,
        "applications": apps.map { ["pid": $0.processIdentifier, "executable": $0.executableURL?.path ?? "",
                                   "launchDate": $0.launchDate?.timeIntervalSince1970 ?? 0] as [String: Any] },
        "processes": rows.map { ["pid": $0.pid, "uid": $0.uid, "born": $0.born, "executable": $0.executable] as [String: Any] },
    ]
}

private func noNativeProcesses() throws {
    guard NSRunningApplication.runningApplications(withBundleIdentifier: "com.aeroric.native.dev").isEmpty,
          try selectedProcesses().isEmpty else { throw Failure.occupied }
}

private func environment(_ own: Identity) throws -> [String: String] {
    var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, own.pid]
    var buffer = [UInt8](repeating: 0, count: 1024 * 1024)
    var size = buffer.count
    defer { _ = buffer.withUnsafeMutableBytes { $0.initializeMemory(as: UInt8.self, repeating: 0) } }
    guard buffer.withUnsafeMutableBytes({ sysctl(&mib, 3, $0.baseAddress, &size, nil, 0) }) == 0,
          size >= 4, size <= buffer.count else { throw Failure.observation }
    let argc = Int(UInt32(buffer[0]) | UInt32(buffer[1]) << 8 | UInt32(buffer[2]) << 16 | UInt32(buffer[3]) << 24)
    guard argc > 0 && argc < size else { throw Failure.observation }
    var offset = 4
    func string() throws -> ArraySlice<UInt8> {
        guard offset < size, let end = buffer[offset..<size].firstIndex(of: 0) else { throw Failure.observation }
        defer { offset = end + 1 }
        return buffer[offset..<end]
    }
    guard Array(try string()) == Array(own.executable.utf8) else { throw Failure.identity }
    while offset < size && buffer[offset] == 0 { offset += 1 }
    for _ in 0..<argc { _ = try string() } // Never decode, retain or print argv.
    let allowed: Set<String> = ["AERORIC_ENV", "AERORIC_DATA_DIR", "AERORIC_UI_TEST_RUN_ID"]
    var values: [String: String] = [:]
    while offset < size && buffer[offset] != 0 {
        let entry = try string()
        guard let equals = entry.firstIndex(of: 61),
              let key = String(bytes: entry[..<equals], encoding: .utf8), allowed.contains(key) else { continue }
        guard values[key] == nil, let value = String(bytes: entry[(equals + 1)...], encoding: .utf8) else { throw Failure.observation }
        values[key] = value
    }
    guard try identity(own.pid) == own else { throw Failure.identity }
    return values
}

private func safeDirectory(_ path: String, required: Bool = false) throws -> [UInt64] {
    guard path.hasPrefix("/"), URL(fileURLWithPath: path).standardizedFileURL.path == path else { throw Failure.unsafe }
    var url = URL(fileURLWithPath: path)
    var identity: [UInt64] = []
    while url.path != "/" {
        var info = stat()
        if lstat(url.path, &info) == 0 {
            guard info.st_mode & S_IFMT == S_IFDIR else { throw Failure.unsafe }
            identity += [UInt64(info.st_dev), UInt64(info.st_ino)]
        } else if required || errno != ENOENT { throw Failure.unsafe }
        url.deleteLastPathComponent()
    }
    return identity
}

private func reservation(app: String, root: String, runID: String) throws -> ResourceReceipt {
    guard let raw = ProcessInfo.processInfo.environment["AERORIC_UI_TEST_RESOURCE_RECEIPT"],
          raw.utf8.count <= 8192, let encoded = raw.data(using: .utf8) else { throw Failure.unsafe }
    let reserved = try JSONDecoder().decode(ResourceReceipt.self, from: encoded)
    try reserved.verify(app: app, root: root, runID: runID)
    return reserved
}

@MainActor
private func warmup(app: String, root: String, runID: String) throws -> [String: Any] {
    var custody = WarmupCustody()
    let reserved = try reservation(app: app, root: root, runID: runID)
    try noNativeProcesses()
    try custody.begin(clear: true)
    let directories = try [root, root + "/warmup", root + "/data"].map {
        ($0, try safeDirectory($0, required: true))
    }
    let executable = app + "/Contents/MacOS/AeroricNative"
    guard FileManager.default.fileExists(atPath: executable) else { throw Failure.unsafe }
    let config = NSWorkspace.OpenConfiguration()
    config.createsNewApplicationInstance = true
    config.activates = false
    config.environment = [
        "AERORIC_ENV": "ui-test", "AERORIC_DATA_DIR": root + "/warmup",
        "AERORIC_UI_TEST_RUN_ID": runID, "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    ]
    // The returned instance is the causal launch receipt. Merely seeing a
    // new PID or a successful `open` subprocess is never sufficient.
    let receipt = LaunchReceipt()
    try reserved.verify(app: app, root: root, runID: runID) // Same reservation, immediately before the actual launch.
    NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: app), configuration: config) { result, error in
        receipt.publish(result, failed: error != nil)
    }
    let deadline = Date().addingTimeInterval(30)
    while receipt.read() == nil && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
    guard let (launched, failed) = receipt.read(), !failed, let instance = launched, let date = instance.launchDate,
          instance.executableURL?.path == executable,
          let own = try identity(instance.processIdentifier), own.uid == getuid(),
          own.executable == executable else { throw Failure.launch }
    try custody.associate(valid: true)
    RunLoop.current.run(until: Date().addingTimeInterval(8))
    let currentApps = NSRunningApplication.runningApplications(withBundleIdentifier: "com.aeroric.native.dev")
    guard currentApps.count == 1, currentApps.first?.processIdentifier == own.pid,
          currentApps.first?.launchDate == date,
          mayQuit(expected: own, actual: try identity(own.pid), causal: true,
                  root: root, environment: try environment(own), runID: runID) else { throw Failure.identity }
    for (path, expected) in directories {
        guard try safeDirectory(path, required: true) == expected else { throw Failure.unsafe }
    }
    try reserved.verify(app: app, root: root, runID: runID)
    try custody.requestQuit(valid: true)
    // Public, normal Quit request. Never forceTerminate and never signal a PID.
    guard instance.terminate() else { throw Failure.quit }
    let exitDeadline = Date().addingTimeInterval(20)
    repeat {
        let clear: Bool
        do { try noNativeProcesses(); clear = true }
        catch Failure.occupied { clear = false } // Only occupancy may be observed again.
        // Observation failure propagates immediately; no later empty sample can wash it.
        if try custody.observeExit(clear ? .clear : .occupied) {
            return ["completed": true, "causalLaunchReceipt": true, "exitClean": true,
                    "pid": own.pid, "born": own.born, "launchDate": date.timeIntervalSince1970]
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    } while Date() < exitDeadline
    throw Failure.pending // Leave the App/child and owner intact; no next launch.
}

if CommandLine.arguments == [CommandLine.arguments[0], "--fixtures"] {
    try fixtures()
} else {
    do {
        let args = Array(CommandLine.arguments.dropFirst())
        guard args.count == 4, ["snapshot", "prewarm"].contains(args[0]),
              UUID(uuidString: args[3]) != nil,
              args[2] == NSHomeDirectory() + "/Library/Caches/aeroric-native/ui-test-" + args[3],
              args[1].hasSuffix("/Debug/AeroricNative.app") else { throw Failure.unsafe }
        for directory in [args[2], args[2] + "/data", args[2] + "/warmup"] {
            _ = try safeDirectory(directory, required: true)
        }
        _ = try safeDirectory(args[1])
        _ = try reservation(app: args[1], root: args[2], runID: args[3])
        var data: Data
        if args[0] == "snapshot" {
            data = try JSONSerialization.data(withJSONObject: snapshot(), options: [.sortedKeys])
        } else {
            data = try MainActor.assumeIsolated {
                try JSONSerialization.data(withJSONObject: warmup(app: args[1], root: args[2], runID: args[3]), options: [.sortedKeys])
            }
        }
        data.append(10)
        try FileHandle.standardOutput.write(contentsOf: data)
    } catch {
        // No raw argv/env/paths or unrestricted diagnostic exception values.
        let data = Data("{\"error\":\"native UI process custody failed; retain resources\"}\n".utf8)
        try? FileHandle.standardError.write(contentsOf: data)
        exit(78)
    }
}
