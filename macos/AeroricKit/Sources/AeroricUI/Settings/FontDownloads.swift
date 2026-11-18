import CoreText
import CryptoKit
import Foundation
import Observation

/// In-app download of the two preset Chinese fonts (2026-10-29): pinned
/// release, checksum verified, only the weights the conversation uses
/// extracted into the app's own folder, then registered for this process —
/// nothing is installed system-wide and no installer appears. Installed fonts
/// are registered again at every launch (`registerInstalled`).
public struct FontPackage: Sendable, Equatable {
    public let choice: ConversationCJKFont
    public let url: URL
    public let sha256: String
    public let downloadBytes: Int64
    public let installedBytes: Int64
    /// Files taken from the archive.
    public let files: [String]

    public static let all: [FontPackage] = [
        FontPackage(
            choice: .mono,
            url: URL(string: "https://github.com/be5invis/Sarasa-Gothic/releases/download/v1.0.43/SarasaMonoSC-TTF-Unhinted-1.0.43.7z")!,
            sha256: "b2f09e089c0dfb049860635a91766b7decc19b7ef6df8ae9bb11a22451845612",
            downloadBytes: 49_862_338, installedBytes: 41_932_596,
            files: ["SarasaMonoSC-Regular.ttf", "SarasaMonoSC-SemiBold.ttf", "SarasaMonoSC-Bold.ttf"]),
        FontPackage(
            choice: .maple,
            url: URL(string: "https://github.com/subframe7536/maple-font/releases/download/v7.9/MapleMono-CN-unhinted.zip")!,
            sha256: "d41cb72721e99cfe4fbd1a7b0f182a013457de46aa612018f924dd024699d3b9",
            downloadBytes: 140_216_987, installedBytes: 73_958_050,
            files: ["MapleMono-CN-Regular.ttf", "MapleMono-CN-Medium.ttf", "MapleMono-CN-SemiBold.ttf",
                    "MapleMono-CN-Bold.ttf", "LICENSE.txt"]),
    ]

    public static func of(_ choice: ConversationCJKFont) -> FontPackage? { all.first { $0.choice == choice } }

    /// `约 50 MB`.
    public static func megabytes(_ bytes: Int64) -> String { "约 \(Int((Double(bytes) / 1_000_000).rounded())) MB" }

    /// A10 (2026-11-17): what the settings row says about a preset.
    public enum InstallState: Equatable, Sendable {
        /// In use.
        case active(String)
        case notDownloaded
        /// Downloaded but not usable: why, with 重试 and 删除 beside it.
        case notRegistered(String)
    }

    /// A10: a preset's state from the family it resolves to, whether its
    /// files are on disk, and how registering them last went. Files that are
    /// there are never "not downloaded".
    public static func installState(choice: ConversationCJKFont, family: String?, filesPresent: Bool,
                                    failure: String?) -> InstallState {
        if let family { return .active(family) }
        guard filesPresent else { return .notDownloaded }
        if let failure { return .notRegistered("没装上：\(failure)") }
        return .notRegistered("已下载，但系统里找不到「\(choice.candidates.first ?? choice.title)」字体")
    }
}

@MainActor @Observable
public final class FontDownloads {
    public enum Phase: Equatable {
        case idle
        case downloading(Double)
        case installing
        case failed(String)
    }

    public static let shared = FontDownloads()
    public private(set) var phases: [ConversationCJKFont: Phase] = [:]
    /// A10: why a downloaded preset's files did not register, the last time
    /// they were registered (launch, install or 重试); absent when they did.
    public private(set) var failures: [ConversationCJKFont: String] = [:]
    /// Bumped when fonts are registered, so views re-resolve families.
    public private(set) var revision = 0
    private var tasks: [ConversationCJKFont: Task<Void, Never>] = [:]
    /// B2-7: which start owns each choice now; an older one writes nothing.
    private var tokens: [ConversationCJKFont: UUID] = [:]

    public func phase(_ choice: ConversationCJKFont) -> Phase { phases[choice] ?? .idle }

    public nonisolated static var directory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent(Bundle.main.bundleIdentifier ?? "Aeroric", isDirectory: true)
            .appendingPathComponent("Fonts", isDirectory: true)
    }

    /// Registers every downloaded font for this process; call at launch.
    /// A10: what failed is kept for the settings row.
    public static func registerInstalled() {
        for package in FontPackage.all {
            shared.failures[package.choice] = registerAll(package)
        }
    }

    /// Whether `choice`'s files are on disk.
    public nonisolated static func filesPresent(_ choice: ConversationCJKFont) -> Bool {
        FontPackage.of(choice).map { !installedFonts($0).isEmpty } ?? false
    }

    nonisolated static func installedFonts(_ package: FontPackage, root: URL = directory) -> [URL] {
        let folder = root.appendingPathComponent(package.choice.rawValue, isDirectory: true)
        return package.files.filter { $0.hasSuffix(".ttf") }
            .map { folder.appendingPathComponent($0) }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
    }

    /// Registers `url` for this process; nil when it is in (already
    /// registered counts), else why not (A10).
    nonisolated static func register(_ url: URL) -> String? {
        var error: Unmanaged<CFError>?
        guard !CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) else { return nil }
        let cfError = error?.takeRetainedValue()
        let reason = registrationFailure(code: cfError.map(CFErrorGetCode) ?? 0,
                                         described: cfError.map { CFErrorCopyDescription($0) as String })
        if let reason { NSLog("Aeroric: registering %@ failed: %@", url.lastPathComponent, reason) }
        return reason
    }

    /// A10: every downloaded file of `package` registered; the first failure
    /// as `<file>：<why>`, nil when all are in.
    nonisolated static func registerAll(_ package: FontPackage, root: URL = directory) -> String? {
        var first: String?
        for url in installedFonts(package, root: root) {
            if let reason = register(url), first == nil { first = "\(url.lastPathComponent)：\(reason)" }
        }
        return first
    }

    /// A10: CoreText's reason, in words; nil for "already registered".
    nonisolated static func registrationFailure(code: Int, described: String?) -> String? {
        switch code {
        case CTFontManagerError.alreadyRegistered.rawValue: nil
        case CTFontManagerError.fileNotFound.rawValue: "找不到字体文件"
        case CTFontManagerError.insufficientPermissions.rawValue: "没有权限读取字体文件"
        case CTFontManagerError.unrecognizedFormat.rawValue: "不是能识别的字体文件"
        case CTFontManagerError.invalidFontData.rawValue: "字体文件已损坏"
        case CTFontManagerError.exceededResourceLimit.rawValue: "超出系统可注册的字体数量"
        default: described ?? "原因不明（错误码 \(code)）"
        }
    }

    /// A10: register `choice`'s files again — no download.
    public func retryRegistration(_ choice: ConversationCJKFont) {
        guard let package = FontPackage.of(choice) else { return }
        failures[choice] = Self.registerAll(package)
        revision += 1
    }

    /// A10: unregister and delete `choice`'s files; back to not downloaded.
    public func remove(_ choice: ConversationCJKFont) {
        guard let package = FontPackage.of(choice), tasks[choice] == nil else { return }
        for url in Self.installedFonts(package) {
            CTFontManagerUnregisterFontsForURL(url as CFURL, .process, nil)
        }
        let folder = Self.directory.appendingPathComponent(choice.rawValue, isDirectory: true)
        do {
            try FileManager.default.removeItem(at: folder)
            failures[choice] = nil
        } catch {
            failures[choice] = "删除失败：\(error.localizedDescription)"
        }
        phases[choice] = .idle
        revision += 1
    }

    public func start(_ choice: ConversationCJKFont) {
        guard let package = FontPackage.of(choice), tasks[choice] == nil else { return }
        phases[choice] = .downloading(0)
        let token = UUID()
        tokens[choice] = token
        tasks[choice] = Task { [weak self] in
            // B2-7: a cancelled (or replaced) download writes nothing back.
            let current = { self?.tokens[choice] == token }
            do {
                let archive = try await Self.download(package) { fraction in
                    Task { @MainActor in
                        guard self?.tokens[choice] == token else { return }
                        if case .downloading = self?.phases[choice] { self?.phases[choice] = .downloading(fraction) }
                    }
                }
                // Cancelled while the last bytes came in: not installed.
                guard !Task.isCancelled, current() else {
                    try? FileManager.default.removeItem(at: archive)
                    throw CancellationError()
                }
                self?.phases[choice] = .installing
                let failure = try await Task.detached { try Self.install(package, archive: archive) }.value
                self?.failures[choice] = failure
                self?.phases[choice] = .idle
                self?.revision += 1
            } catch is CancellationError {
                if current() { self?.phases[choice] = .idle }
            } catch {
                if current() { self?.phases[choice] = .failed(Self.describe(error)) }
            }
            if current() {
                self?.tasks[choice] = nil
                self?.tokens[choice] = nil
            }
        }
    }

    /// B2-7: the download stops and the choice is free at once (a new start
    /// is not blocked by the one winding down).
    public func cancel(_ choice: ConversationCJKFont) {
        tasks[choice]?.cancel()
        tasks[choice] = nil
        tokens[choice] = nil
        phases[choice] = .idle
    }

    // MARK: Steps

    private nonisolated static func download(_ package: FontPackage,
                                             progress: @escaping @Sendable (Double) -> Void) async throws -> URL {
        let watcher = DownloadWatcher(expected: package.downloadBytes, progress: progress)
        let session = URLSession(configuration: .ephemeral, delegate: watcher, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                watcher.begin(session.downloadTask(with: package.url), continuation)
            }
        } onCancel: {
            watcher.cancel()
        }
    }

    /// Verify, extract the needed files, move them into place, register.
    /// A10: returns why registering failed (the files stay), nil when in.
    @discardableResult
    nonisolated static func install(_ package: FontPackage, archive: URL, root: URL = directory) throws -> String? {
        defer { try? FileManager.default.removeItem(at: archive) }
        guard try sha256(of: archive) == package.sha256 else { throw FontDownloadError.checksum }
        let scratch = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-font-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }
        // libarchive's tar reads both 7z and zip.
        let tar = Process()
        tar.executableURL = URL(fileURLWithPath: "/usr/bin/tar")
        tar.arguments = ["-xf", archive.path, "-C", scratch.path] + package.files
        try tar.run()
        tar.waitUntilExit()
        guard tar.terminationStatus == 0 else { throw FontDownloadError.extract }
        let folder = root.appendingPathComponent(package.choice.rawValue, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for name in package.files {
            let target = folder.appendingPathComponent(name)
            try? FileManager.default.removeItem(at: target)
            try FileManager.default.moveItem(at: scratch.appendingPathComponent(name), to: target)
        }
        return registerAll(package, root: root)
    }

    nonisolated static func sha256(of file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 4 << 20), !chunk.isEmpty { hasher.update(data: chunk) }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    private nonisolated static func describe(_ error: Error) -> String {
        switch error {
        case FontDownloadError.checksum: "下载的文件校验不通过，请重试"
        case FontDownloadError.extract: "解压失败，请重试"
        case let error as URLError where error.code == .notConnectedToInternet: "没有网络连接"
        default: "下载失败：\(error.localizedDescription)"
        }
    }
}

enum FontDownloadError: Error { case checksum, extract }

/// Progress and completion for one download task.
///
/// B2-7: `onCancel` runs on another thread, possibly before the task exists;
/// the lock keeps the task, the continuation and the cancel flag together.
final class DownloadWatcher: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    let expected: Int64
    let progress: @Sendable (Double) -> Void
    private let lock = NSLock()
    private var continuation: CheckedContinuation<URL, Error>?
    private var task: URLSessionDownloadTask?
    private var cancelled = false
    private var lastReported = -1

    /// Starts `task`, unless the download was cancelled first.
    func begin(_ task: URLSessionDownloadTask, _ continuation: CheckedContinuation<URL, Error>) {
        lock.lock()
        if cancelled {
            lock.unlock()
            continuation.resume(throwing: CancellationError())
            return
        }
        self.task = task
        self.continuation = continuation
        lock.unlock()
        task.resume()
    }

    func cancel() {
        lock.lock()
        cancelled = true
        let task = task
        lock.unlock()
        task?.cancel()
    }

    /// The continuation, once: whoever finishes first resumes it.
    private func take() -> CheckedContinuation<URL, Error>? {
        lock.lock()
        defer { lock.unlock() }
        let taken = continuation
        continuation = nil
        return taken
    }

    init(expected: Int64, progress: @escaping @Sendable (Double) -> Void) {
        self.expected = expected
        self.progress = progress
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64,
                    totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        let total = totalBytesExpectedToWrite > 0 ? totalBytesExpectedToWrite : expected
        let percent = Int(Double(totalBytesWritten) / Double(max(total, 1)) * 100)
        guard percent != lastReported else { return }
        lastReported = percent
        progress(min(1, Double(percent) / 100))
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        // The file at `location` is removed when this returns: keep it.
        let kept = FileManager.default.temporaryDirectory.appendingPathComponent("aeroric-font-\(UUID().uuidString)")
        do {
            if let http = downloadTask.response as? HTTPURLResponse, !(200 ..< 300).contains(http.statusCode) {
                throw URLError(.badServerResponse)
            }
            try FileManager.default.moveItem(at: location, to: kept)
            take()?.resume(returning: kept)
        } catch {
            take()?.resume(throwing: error)
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let error, let continuation = take() else { return }
        let cancelled = (error as? URLError)?.code == .cancelled
        continuation.resume(throwing: cancelled ? CancellationError() : error)
    }
}
