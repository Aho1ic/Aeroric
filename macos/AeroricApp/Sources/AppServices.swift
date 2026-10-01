import AeroricProtocol
import Foundation
import Observation

/// Bootstrap-scoped app services (P4-01 Action 6): exactly the explicitly
/// injected preview configuration plus derived bootstrap state.
///
/// Deliberately NO `HostSupervisor` / `HostConnection` references — those are
/// built in P4-02, which will inject the single supervisor/client pair here.
/// At this stage the app performs no storage access: the data directory is
/// only ever recorded from the injected environment (`AERORIC_DATA_DIR`),
/// never discovered, never created, never read. Production data lives under
/// the old bundle id `com.aeroric.desktop`; this preview app runs as
/// `com.aeroric.native.dev` and shares no TCC grants with it (ADR-001).
@MainActor
@Observable
final class AppServices {
    /// What the caller injected for this launch. `injectedExplicitly == false`
    /// means a plain launch (Xcode Run, Finder) — still no data access.
    struct PreviewConfiguration: Equatable, Sendable {
        var environmentLabel: String
        var dataDirectoryPath: String?
        var injectedExplicitly: Bool
    }

    private(set) var configuration: PreviewConfiguration
    /// Wire version this build speaks, straight from AeroricKit (v1).
    let protocolVersion: Int

    init(configuration: PreviewConfiguration, protocolVersion: Int) {
        self.configuration = configuration
        self.protocolVersion = protocolVersion
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
}
