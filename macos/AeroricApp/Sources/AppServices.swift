import AeroricClient
import AeroricProtocol
import AeroricUI
import Foundation
import Observation

/// App-scoped services. P4-02 adds the single supervisor/client pair:
/// `startSupervisor()` spawns the headless host (`aeroric-cli serve`) from
/// the bundle and wires `ConnectionStore` as the single consumer of both
/// phases and events.
///
/// Data directory: the explicitly injected `AERORIC_DATA_DIR` wins; a plain
/// launch derives the default under `Application Support` (relative to the
/// standard location, never an absolute path baked into source). The host is
/// the only tenant of that directory — the app itself never reads it; it
/// only passes it through.
@MainActor
@Observable
final class AppServices {
    /// What the caller injected for this launch. `injectedExplicitly == false`
    /// means a plain launch (Xcode Run, Finder).
    struct PreviewConfiguration: Equatable, Sendable {
        var environmentLabel: String
        var dataDirectoryPath: String?
        var injectedExplicitly: Bool
    }

    private(set) var configuration: PreviewConfiguration
    /// Wire version this build speaks, straight from AeroricKit (v1).
    let protocolVersion: Int

    let connectionStore = ConnectionStore()
    let applicationStore: ApplicationStore
    let projectStore = ProjectStore()

    private(set) var supervisor: HostSupervisor?
    /// The live channel; retained so the sink stays wired between attempts.
    private var attachedConnection: HostConnection?

    init(configuration: PreviewConfiguration, protocolVersion: Int) {
        self.configuration = configuration
        self.protocolVersion = protocolVersion
        self.applicationStore = ApplicationStore(
            environmentTag: configuration.environmentLabel,
            dataDirPath: configuration.dataDirectoryPath,
            injectedExplicitly: configuration.injectedExplicitly
        )
    }

    /// Reads only the explicitly injected environment keys. No defaults that
    /// touch user data; nothing is created or opened here.
    static func resolvingInjectedPreviewConfiguration(
        environment: ProcessInfo = .processInfo
    ) -> AppServices {
        let dataDirectory = environment.environment["AERORIC_DATA_DIR"]
        let injectedEnvironment = environment.environment["AERORIC_ENV"]
        let configuration = PreviewConfiguration(
            environmentLabel: injectedEnvironment ?? "default",
            dataDirectoryPath: dataDirectory,
            injectedExplicitly: dataDirectory != nil || injectedEnvironment != nil
        )
        return AppServices(configuration: configuration, protocolVersion: ProtocolMarker.version)
    }

    /// The host's tenant directory: injected path or the app-support default.
    /// Deriving the default only at supervisor start keeps plain launches
    /// (no supervisor yet) free of any filesystem touch.
    var dataDirectory: URL {
        if let path = configuration.dataDirectoryPath {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        return URL.applicationSupportDirectory
            .appendingPathComponent("com.aeroric.native.dev", isDirectory: true)
    }

    /// Build identifier for the bootstrap surface: version-config+build.
    var buildIdentifier: String {
        #if DEBUG
        let configurationName = "debug"
        #else
        let configurationName = "release-local"
        #endif
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown"
        return "\(version)-\(configurationName)+\(build)"
    }

    /// Idempotent: repeated `.task`/`.onChange(.active)` calls are no-ops once
    /// a supervisor exists (its own run loop lives until `stopSupervisor`).
    func startSupervisor() async {
        guard supervisor == nil else { return }
        let dataDir = dataDirectory
        do {
            try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            // The supervisor degrades on its own if the directory is unusable;
            // surfacing a second failure mode here adds nothing.
        }
        let store = connectionStore
        let supervisor = HostSupervisor(
            config: SupervisorConfig(
                dataDir: dataDir,
                socketPath: dataDir.appendingPathComponent("host.sock").path,
                environment: Self.childEnvironment()
            ),
            onChange: { [weak self] phase in
                Task { @MainActor in
                    self?.connectionStore.applyPhase(phase)
                }
            },
            // Strong capture: this closure lives inside the supervisor we
            // hold; the app services object is process-lifetime anyway.
            configureConnection: { connection in
                await connection.setEventSink { frame in
                    Task { @MainActor in
                        store.ingest(frame)
                    }
                }
                // Runs after start(), so `self.supervisor` is already assigned.
                await MainActor.run {
                    self.attachedConnection = connection
                    self.connectionStore.attach(connection: connection, supervisor: self.supervisor)
                }
            }
        )
        self.supervisor = supervisor
        await supervisor.start()
    }

    func stopSupervisor() async {
        guard let supervisor else { return }
        self.supervisor = nil
        await supervisor.stop()
    }

    /// Environment for the child: whitelist, never a blind passthrough. The
    /// host derives everything from `--data-dir`; only test runners need to
    /// see the runner's own markers.
    static func childEnvironment() -> [String: String] {
        var forwarded: [String: String] = [:]
        for (key, value) in ProcessInfo.processInfo.environment where key.hasPrefix("TEST_RUNNER_") {
            forwarded[key] = value
        }
        return forwarded
    }

    enum ProjectError: Error {
        case notConnected
        case malformedReply
    }

    /// UI-test hook (P4-04 acceptance: per-window selection needs real
    /// sidebar rows, and `NSOpenPanel` cannot be driven from XCUITest).
    /// `AERORIC_TEST_OPEN_PROJECT="path1:path2"` opens the listed directories
    /// through the real `project.open` RPC once the host is ready. Unset in
    /// every plain launch — production behaviour is untouched, and the paths
    /// only ever point at the run's scratch data dir.
    func openInjectedTestProjectsIfRequested() async {
        guard let raw = ProcessInfo.processInfo.environment["AERORIC_TEST_OPEN_PROJECT"],
              !raw.isEmpty else { return }
        for _ in 0..<200 {
            if attachedConnection != nil, case .ready = connectionStore.phase { break }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        guard attachedConnection != nil else { return }
        for path in raw.split(separator: ":", omittingEmptySubsequences: true) {
            _ = try? await openProject(at: String(path))
        }
    }

    /// Opens a project through the host (`project.open`). ADR-013: the
    /// caller mints the request id, so the app-side `newId` travels in
    /// params and the host echoes the confirmed entry back.
    @discardableResult
    func openProject(at path: String) async throws -> ProjectStore.ProjectEntry {
        guard let connection = attachedConnection else {
            throw ProjectError.notConnected
        }
        let newId = UUID().uuidString
        let frame = try await connection.request(
            "project.open",
            params: .object(["path": .string(path), "newId": .string(newId)])
        )
        guard
            let id = frame.result["project"]?["id"], case let .string(projectId) = id,
            let rawPath = frame.result["project"]?["path"], case let .string(projectPath) = rawPath
        else {
            throw ProjectError.malformedReply
        }
        return projectStore.upsertProject(id: projectId, path: projectPath)
    }
}

// MARK: - Shell state injection (P4-04)

/// Maps the client's `SupervisorPhase` onto the shell's snapshot enum at the
/// injection boundary (AeroricUI has no client dependency, INV-02).
extension AppServices: WorkspaceStateProviding {
    var phaseSnapshot: ConnectionPhaseSnapshot {
        switch connectionStore.phase {
        case .idle: .idle
        case .connecting: .connecting
        case .ready: .ready
        case let .degraded(reason): .degraded(reason: reason)
        // Attempt/delay stay in `phaseText`; the snapshot is deliberately
        // detail-free so the shell never re-formats supervisor internals.
        case let .reconnecting(attempt, nextDelayMs): .reconnecting
        case let .incompatible(reason): .incompatible(reason: reason)
        case .stopped: .stopped
        }
    }

    var phaseText: String { connectionStore.phaseText }
    var environmentLabel: String { configuration.environmentLabel }
    var protocolVersionText: String { "v\(protocolVersion)" }
    var applicationTag: String { applicationStore.environmentTag }

    var sidebarItems: [SidebarItem] {
        projectStore.projects.map {
            SidebarItem(id: $0.id, title: URL(fileURLWithPath: $0.path).lastPathComponent)
        }
    }

    var paletteCommands: [PaletteCommand] {
        [
            PaletteCommand(id: "task.new", title: "新建任务"),
            PaletteCommand(id: "project.open", title: "打开项目…"),
            PaletteCommand(id: "settings.show", title: "设置…"),
        ]
    }
}

extension AppServices: SettingsStateProviding {}
