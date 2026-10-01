import AeroricClient
import AeroricProtocol
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
    let workspaceStore = WorkspaceStore()

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
}
