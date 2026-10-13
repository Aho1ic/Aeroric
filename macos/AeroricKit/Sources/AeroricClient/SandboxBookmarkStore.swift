import Foundation

/// AP3: Sandbox / App Group data root resolver with UDS `sun_path` (104-byte) budget enforcement.
public enum SandboxDataRoot {
    public static let sunPathLimit = 104

    /// Resolves the managed data directory without touching real user homes when an
    /// explicit path is injected, and avoids double-nesting inside App Sandbox containers
    /// (`~/Library/Containers/<bundle-id>/Data/Library/Application Support/...` exceeds 104B).
    public static func resolve(
        injectedPath: String? = nil,
        appGroupIdentifier: String? = nil,
        homeDirectory: String = NSHomeDirectory(),
        bundleIdentifier: String = Bundle.main.bundleIdentifier ?? "com.aeroric.native.dev"
    ) -> URL {
        if let injectedPath, !injectedPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return URL(fileURLWithPath: injectedPath, isDirectory: true)
        }
        if let groupID = appGroupIdentifier,
           let groupURL = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: groupID) {
            let candidate = groupURL.appendingPathComponent(".aeroric", isDirectory: true)
            if candidate.appendingPathComponent("host.sock").path.utf8.count < sunPathLimit {
                return candidate
            }
        }
        if homeDirectory.contains("/Library/Containers/") {
            return URL(fileURLWithPath: homeDirectory, isDirectory: true)
                .appendingPathComponent(".aeroric", isDirectory: true)
        }
        let appSupport = URL.applicationSupportDirectory
            .appendingPathComponent(bundleIdentifier, isDirectory: true)
        if appSupport.appendingPathComponent("host.sock").path.utf8.count < sunPathLimit {
            return appSupport
        }
        return URL(fileURLWithPath: homeDirectory, isDirectory: true)
            .appendingPathComponent(".aeroric", isDirectory: true)
    }

    /// Returns `<dataDir>/host.sock` when within the kernel `sun_path` limit (104B).
    public static func socketPath(for dataDir: URL) -> String {
        let direct = dataDir.appendingPathComponent("host.sock").path
        if direct.utf8.count < sunPathLimit {
            return direct
        }
        // Deterministic short fallback under /tmp when an unusually deep path is used.
        var hash: UInt64 = 14695981039346656037
        for byte in dataDir.path.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1099511628211
        }
        return String(format: "/tmp/aeroric-%016llx.sock", hash)
    }
}

/// AP3: Explicit opt-in background host policy.
/// Strictly separates `keepAliveOnUIQuit` from `remoteEnabled`, `launchAtLogin`, and `preventSleep`.
public struct BackgroundHostPolicy: Codable, Equatable, Sendable {
    public var keepAliveOnUIQuit: Bool
    public var remoteEnabled: Bool
    public var launchAtLogin: Bool
    public var preventSleep: Bool

    public init(
        keepAliveOnUIQuit: Bool = false,
        remoteEnabled: Bool = false,
        launchAtLogin: Bool = false,
        preventSleep: Bool = false
    ) {
        self.keepAliveOnUIQuit = keepAliveOnUIQuit
        self.remoteEnabled = remoteEnabled
        self.launchAtLogin = launchAtLogin
        self.preventSleep = preventSleep
    }

    public static func load(from dataDir: URL) -> BackgroundHostPolicy {
        let url = dataDir.appendingPathComponent("background-policy.json")
        guard let data = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode(BackgroundHostPolicy.self, from: data) else {
            return BackgroundHostPolicy()
        }
        return decoded
    }

    public func save(to dataDir: URL) throws {
        try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        let url = dataDir.appendingPathComponent("background-policy.json")
        let data = try JSONEncoder().encode(self)
        try data.write(to: url, options: .atomic)
    }
}

/// Impact report returned when stopping the background host service explicitly.
public struct BackgroundStopImpact: Equatable, Sendable {
    public var wasRunning: Bool
    public var backgroundKeepAliveWasEnabled: Bool
    public var terminatedPID: Int32?

    public init(wasRunning: Bool, backgroundKeepAliveWasEnabled: Bool, terminatedPID: Int32?) {
        self.wasRunning = wasRunning
        self.backgroundKeepAliveWasEnabled = backgroundKeepAliveWasEnabled
        self.terminatedPID = terminatedPID
    }
}

/// AP3: Security-scoped bookmark driver seam for real sandbox and deterministic tests.
public protocol BookmarkDriver: Sendable {
    func createBookmarkData(for url: URL) throws -> Data
    func resolveBookmarkData(_ data: Data) throws -> (url: URL, isStale: Bool)
    func startAccessing(_ url: URL) -> Bool
    func stopAccessing(_ url: URL)
}

public struct SystemBookmarkDriver: BookmarkDriver {
    public init() {}

    public func createBookmarkData(for url: URL) throws -> Data {
        if let scoped = try? url.bookmarkData(
            options: [.withSecurityScope],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        ) {
            return scoped
        }
        return try url.bookmarkData(
            options: [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
    }

    public func resolveBookmarkData(_ data: Data) throws -> (url: URL, isStale: Bool) {
        var stale = false
        if let scoped = try? URL(
            resolvingBookmarkData: data,
            options: [.withSecurityScope, .withoutUI],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        ) {
            return (scoped, stale)
        }
        let plain = try URL(
            resolvingBookmarkData: data,
            options: [.withoutUI],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        )
        return (plain, stale)
    }

    public func startAccessing(_ url: URL) -> Bool {
        _ = url.startAccessingSecurityScopedResource()
        var isDir: ObjCBool = false
        return FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) && isDir.boolValue
    }

    public func stopAccessing(_ url: URL) {
        url.stopAccessingSecurityScopedResource()
    }
}

/// AP3: Persistent security-scoped bookmark manager for user-authorized project directories.
public final class SandboxBookmarkStore: @unchecked Sendable {
    public enum BookmarkState: Equatable, Sendable {
        case active(path: String, refreshedStale: Bool)
        case revoked(path: String, reason: String)
    }

    private struct PersistedCatalog: Codable {
        var bookmarksBase64: [String: String] = [:]
    }

    private let lock = NSLock()
    private let dataDir: URL
    private let driver: BookmarkDriver
    private var activeURLs: [String: URL] = [:]
    private var states: [String: BookmarkState] = [:]

    public init(dataDir: URL, driver: BookmarkDriver = SystemBookmarkDriver()) {
        self.dataDir = dataDir
        self.driver = driver
    }

    public var storageURL: URL {
        dataDir.appendingPathComponent("bookmarks.json")
    }

    public func currentStates() -> [String: BookmarkState] {
        lock.lock()
        defer { lock.unlock() }
        return states
    }

    /// Registers a directory chosen by the user via `NSOpenPanel`, creates & persists
    /// its bookmark, and begins security-scoped access.
    @discardableResult
    public func registerAuthorizedDirectory(_ url: URL) throws -> BookmarkState {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir), isDir.boolValue else {
            throw NSError(
                domain: "AeroricSandboxBookmark",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Authorized path does not exist or is not a directory: \(url.path)"]
            )
        }
        let canonicalPath = url.standardizedFileURL.path
        let bookmarkData = try driver.createBookmarkData(for: url)
        guard driver.startAccessing(url) else {
            let state = BookmarkState.revoked(path: canonicalPath, reason: "security-scoped access denied")
            lock.lock()
            states[canonicalPath] = state
            lock.unlock()
            return state
        }

        lock.lock()
        defer { lock.unlock() }
        var catalog = loadCatalogLocked()
        catalog.bookmarksBase64[canonicalPath] = bookmarkData.base64EncodedString()
        try saveCatalogLocked(catalog)
        activeURLs[canonicalPath] = url
        let state = BookmarkState.active(path: canonicalPath, refreshedStale: false)
        states[canonicalPath] = state
        return state
    }

    /// Restores all persisted bookmarks across app restarts, refreshing stale bookmarks
    /// and marking missing/revoked directories as `.revoked` without silent fallback.
    @discardableResult
    public func restoreAllBookmarks() -> [String: BookmarkState] {
        lock.lock()
        defer { lock.unlock() }
        var catalog = loadCatalogLocked()
        var catalogDirty = false
        var restored: [String: BookmarkState] = [:]

        for (recordedPath, b64) in catalog.bookmarksBase64 {
            guard let raw = Data(base64Encoded: b64) else {
                let revoked = BookmarkState.revoked(path: recordedPath, reason: "corrupted bookmark data")
                restored[recordedPath] = revoked
                continue
            }
            do {
                let (resolvedURL, isStale) = try driver.resolveBookmarkData(raw)
                let canonicalPath = resolvedURL.standardizedFileURL.path
                var isDir: ObjCBool = false
                guard FileManager.default.fileExists(atPath: canonicalPath, isDirectory: &isDir),
                      isDir.boolValue,
                      driver.startAccessing(resolvedURL) else {
                    let revoked = BookmarkState.revoked(path: recordedPath, reason: "target directory missing or access revoked")
                    restored[recordedPath] = revoked
                    continue
                }
                var refreshed = false
                if isStale, let freshData = try? driver.createBookmarkData(for: resolvedURL) {
                    catalog.bookmarksBase64.removeValue(forKey: recordedPath)
                    catalog.bookmarksBase64[canonicalPath] = freshData.base64EncodedString()
                    catalogDirty = true
                    refreshed = true
                }
                activeURLs[canonicalPath] = resolvedURL
                let state = BookmarkState.active(path: canonicalPath, refreshedStale: refreshed)
                restored[canonicalPath] = state
            } catch {
                let revoked = BookmarkState.revoked(path: recordedPath, reason: error.localizedDescription)
                restored[recordedPath] = revoked
            }
        }
        if catalogDirty {
            try? saveCatalogLocked(catalog)
        }
        states = restored
        return restored
    }

    /// Explicitly revokes a bookmark and stops security-scoped resource access.
    public func revokeBookmark(forPath path: String) {
        let canonical = URL(fileURLWithPath: path).standardizedFileURL.path
        lock.lock()
        defer { lock.unlock() }
        if let active = activeURLs.removeValue(forKey: canonical) {
            driver.stopAccessing(active)
        }
        var catalog = loadCatalogLocked()
        catalog.bookmarksBase64.removeValue(forKey: canonical)
        catalog.bookmarksBase64.removeValue(forKey: path)
        try? saveCatalogLocked(catalog)
        states[canonical] = .revoked(path: canonical, reason: "explicitly revoked")
    }

    /// Releases all active security-scoped resources on shutdown.
    public func stopAllAccess() {
        lock.lock()
        let urls = Array(activeURLs.values)
        activeURLs.removeAll()
        lock.unlock()
        for url in urls {
            driver.stopAccessing(url)
        }
    }

    private func loadCatalogLocked() -> PersistedCatalog {
        guard let data = try? Data(contentsOf: storageURL),
              let decoded = try? JSONDecoder().decode(PersistedCatalog.self, from: data) else {
            return PersistedCatalog()
        }
        return decoded
    }

    private func saveCatalogLocked(_ catalog: PersistedCatalog) throws {
        try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        let data = try JSONEncoder().encode(catalog)
        try data.write(to: storageURL, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: storageURL.path)
    }
}
