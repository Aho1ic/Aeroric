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
    /// Bumped when fonts are registered, so views re-resolve families.
    public private(set) var revision = 0
    private var tasks: [ConversationCJKFont: Task<Void, Never>] = [:]

    public func phase(_ choice: ConversationCJKFont) -> Phase { phases[choice] ?? .idle }

    public nonisolated static var directory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent(Bundle.main.bundleIdentifier ?? "Aeroric", isDirectory: true)
            .appendingPathComponent("Fonts", isDirectory: true)
    }

    /// Registers every downloaded font for this process; call at launch.
    public nonisolated static func registerInstalled() {
        for package in FontPackage.all {
            for url in installedFonts(package) { register(url) }
        }
    }

    nonisolated static func installedFonts(_ package: FontPackage, root: URL = directory) -> [URL] {
        let folder = root.appendingPathComponent(package.choice.rawValue, isDirectory: true)
        return package.files.filter { $0.hasSuffix(".ttf") }
            .map { folder.appendingPathComponent($0) }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
    }

    nonisolated static func register(_ url: URL) {
        var error: Unmanaged<CFError>?
        if !CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) {
            // Already registered is fine; anything else falls back to PingFang.
            NSLog("Aeroric: registering %@ failed", url.lastPathComponent)
        }
    }

    public func start(_ choice: ConversationCJKFont) {
        guard let package = FontPackage.of(choice), tasks[choice] == nil else { return }
        phases[choice] = .downloading(0)
        tasks[choice] = Task { [weak self] in
            do {
                let archive = try await Self.download(package) { fraction in
                    Task { @MainActor in
                        if case .downloading = self?.phases[choice] { self?.phases[choice] = .downloading(fraction) }
                    }
                }
                self?.phases[choice] = .installing
                try await Task.detached { try Self.install(package, archive: archive) }.value
                self?.phases[choice] = .idle
                self?.revision += 1
            } catch is CancellationError {
                self?.phases[choice] = .idle
            } catch {
                self?.phases[choice] = .failed(Self.describe(error))
            }
            self?.tasks[choice] = nil
        }
    }

    public func cancel(_ choice: ConversationCJKFont) {
        tasks[choice]?.cancel()
    }

    // MARK: Steps

    private nonisolated static func download(_ package: FontPackage,
                                             progress: @escaping @Sendable (Double) -> Void) async throws -> URL {
        let watcher = DownloadWatcher(expected: package.downloadBytes, progress: progress)
        let session = URLSession(configuration: .ephemeral, delegate: watcher, delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                watcher.continuation = continuation
                let task = session.downloadTask(with: package.url)
                watcher.task = task
                task.resume()
            }
        } onCancel: {
            watcher.task?.cancel()
        }
    }

    /// Verify, extract the needed files, move them into place, register.
    nonisolated static func install(_ package: FontPackage, archive: URL, root: URL = directory) throws {
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
        for url in installedFonts(package, root: root) { register(url) }
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
private final class DownloadWatcher: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    let expected: Int64
    let progress: @Sendable (Double) -> Void
    var continuation: CheckedContinuation<URL, Error>?
    var task: URLSessionDownloadTask?
    private var lastReported = -1

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
            continuation?.resume(returning: kept)
        } catch {
            continuation?.resume(throwing: error)
        }
        continuation = nil
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let error, let continuation else { return }
        self.continuation = nil
        let cancelled = (error as? URLError)?.code == .cancelled
        continuation.resume(throwing: cancelled ? CancellationError() : error)
    }
}
