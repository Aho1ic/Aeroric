import AeroricClient
import SwiftUI

/// Settings sections (P4-04 Action 3). Mirrors the React
/// `AppSettingsDialog` section set (`SECTION_ORDER`:
/// application / agents / about) so the native settings window starts from
/// the same information architecture.
public enum SettingsSection: String, CaseIterable, Identifiable, Hashable, Sendable {
    case application
    case agents
    case about

    public var id: String { rawValue }

    var titleKey: LocalizedStringKey {
        switch self {
        case .application: "通用"
        case .agents: "Agents"
        case .about: "关于"
        }
    }

    var systemImage: String {
        switch self {
        case .application: "gearshape"
        case .agents: "cpu"
        case .about: "info.circle"
        }
    }
}

/// Read model for the settings window. The app object conforms; the
/// settings surface never reaches into client types.
@MainActor
public protocol SettingsStateProviding: AnyObject {
    var environmentLabel: String { get }
    var protocolVersionText: String { get }
    var buildIdentifier: String { get }
    var applicationTag: String { get }
    /// Legacy custom Agent cleanup; nil hides the entry (previews, tests).
    var legacyCleanup: LegacyCleanupStore? { get }
    /// The app's interaction-mode owner; nil in previews and tests.
    /// Host connection phase for the 连接 pane; nil shows "未知".
    var connectionPhaseText: String? { get }
    /// Imported sessions (M4); nil or unavailable hides the 导入 pane.
    var settingsImportBook: ImportBook? { get }
    /// Keep-awake switch (A14); nil or unavailable hides it.
    var settingsKeepAwake: KeepAwakeStore? { get }
    /// A13: fetch the host's redacted diagnostics report; nil hides the entry.
    var settingsDiagnosticsExporter: (@MainActor () async throws -> DiagnosticsClient.Report)? { get }
    /// G: bring the main window to the Agent configuration page; nil hides the button.
    var settingsOpenAgentConfig: (@MainActor () -> Void)? { get }
}

public extension SettingsStateProviding {
    var legacyCleanup: LegacyCleanupStore? { nil }
    var connectionPhaseText: String? { nil }
    var settingsImportBook: ImportBook? { nil }
    var settingsKeepAwake: KeepAwakeStore? { nil }
    var settingsDiagnosticsExporter: (@MainActor () async throws -> DiagnosticsClient.Report)? { nil }
    var settingsOpenAgentConfig: (@MainActor () -> Void)? { nil }
}
