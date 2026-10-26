import Foundation

/// Pure decisions behind the new-session page (2026-10-26 D), kept apart so
/// they can be tested without a view.
public enum NewSessionLayout {
    /// The harness slider, left to right.
    public static let harnessOrder = ["codex", "claude", "dsh", "omp"]
    /// `@AppStorage` key for Settings → Agents → 新会话默认 Harness.
    public static let defaultHarnessKey = "newSession.defaultHarness"

    /// Catalogue ids of agent configurations from the Agent 配置 page; every
    /// other entry is a built-in CLI on its own (official) login.
    public static let agentConfigPrefix = "agent-config:"

    public static func isAgentConfig(_ configuration: SessionConfiguration) -> Bool {
        configuration.id.hasPrefix(agentConfigPrefix)
    }

    /// Whether each harness has at least one usable entry in the catalogue.
    public static func availability(_ configurations: [SessionConfiguration]) -> [String: Bool] {
        Dictionary(uniqueKeysWithValues: harnessOrder.map { harness in
            (harness, configurations.contains { $0.harness == harness && $0.blockedReasons.isEmpty })
        })
    }

    /// The preferred harness when usable, else the first usable one in slider
    /// order, else the preferred one anyway (nothing is usable yet).
    public static func defaultHarness(preferred: String, availability: [String: Bool]) -> String {
        if availability[preferred] == true { return preferred }
        return harnessOrder.first { availability[$0] == true } ?? preferred
    }

    /// What switching to `harness` selects: its first usable entry, the
    /// built-in login before agent configurations.
    public static func firstConfiguration(for harness: String,
                                          in configurations: [SessionConfiguration]) -> SessionConfiguration? {
        let usable = configurations.filter { $0.harness == harness && $0.blockedReasons.isEmpty }
        return usable.first { !isAgentConfig($0) } ?? usable.first
    }
}

/// Whether the project's working tree allows switching branches (Q10-a):
/// never over uncommitted changes.
public enum BranchSwitch: Equatable, Sendable {
    case allowed
    case blocked(String)

    public static func decision(changedFiles: Int) -> BranchSwitch {
        changedFiles == 0 ? .allowed : .blocked("工作区有 \(changedFiles) 个未提交的改动，不能切换分支。先提交或贮藏后再切换。")
    }
}
