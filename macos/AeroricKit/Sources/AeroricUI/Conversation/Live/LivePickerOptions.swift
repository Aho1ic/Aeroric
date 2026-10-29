import Foundation

/// Facts the session page shows but the host's `session.state` does not carry:
/// from the task record, the project and the Agent's configurations.
public struct LiveSessionFacts: Equatable, Sendable {
    public var agent: String?
    public var configurationTitle: String?
    /// The Agent preset the task is pinned to (nil: the CLI's own login).
    public var presetTitle: String?
    public var workingDirectory: String?
    public var branch: String?
    public var startedAt: Date?
    public var additions: UInt64?
    public var deletions: UInt64?
    /// Models and efforts the Agent's configurations declare (picker options).
    public var models: [String]
    public var efforts: [String]
    /// R8: set while the gateway answers this task with a fallback model.
    public var downgradeBadge: String?
    public var downgradeHelp: String?
    /// The prompt the task started with (shown until history has it).
    public var initialPrompt: String?
    /// The project's linked folders (2026-10-29); the info card lists them.
    public var linkedFolders: [String] = []
    /// Other model sources of this harness the session can move to
    /// (2026-10-29): 「全部第三方模型」 and, for a third-party session, the login.
    public var modelSources: [LiveModelSource] = []
    /// Whether the session runs on the harness's own login.
    public var onLogin = true

    public init(agent: String?, configurationTitle: String? = nil, presetTitle: String? = nil,
                workingDirectory: String? = nil,
                branch: String? = nil, startedAt: Date? = nil, additions: UInt64? = nil, deletions: UInt64? = nil,
                models: [String] = [], efforts: [String] = [],
                downgradeBadge: String? = nil, downgradeHelp: String? = nil, initialPrompt: String? = nil) {
        self.agent = agent
        self.configurationTitle = configurationTitle
        self.presetTitle = presetTitle
        self.workingDirectory = workingDirectory
        self.branch = branch
        self.startedAt = startedAt
        self.additions = additions
        self.deletions = deletions
        self.models = models
        self.efforts = efforts
        self.downgradeBadge = downgradeBadge
        self.downgradeHelp = downgradeHelp
        self.initialPrompt = initialPrompt
    }

    /// G4 (2026-10-28): a remote session pinned to an Agent preset reaches it
    /// through this Mac's gateway; nil otherwise.
    public var remoteGatewayBadge: String? {
        guard workingDirectory?.hasPrefix("ssh://") == true, let title = presetTitle,
              !title.isEmpty else { return nil }
        return "经本机网关 · \(title)"
    }

    /// N4 D9: lines, not files — the record only knows lines.
    public var changesText: String {
        guard additions != nil || deletions != nil else { return "—" }
        return "+\(additions ?? 0) −\(deletions ?? 0)"
    }
}

/// Composer capsules on the session page: next-turn model / permission / effort.
@MainActor
public enum LivePickerOptions {
    public static let permissionModes = ["ask", "auto_edit", "full_access"]

    /// The current source's models, then every other source's (one line each,
    /// grouped by connection). Ids of another source carry `<source>␟<model>`.
    public static func allModels(_ facts: LiveSessionFacts, current: String?) -> PickerMenu {
        var menu = models(facts.models, current: current)
        for source in facts.modelSources {
            let own = Set(facts.models)
            var configuration = source.configuration
            configuration.options.models = configuration.options.models.filter { !own.contains($0) }
            guard !configuration.options.models.isEmpty else { continue }
            var sections = ComposerPickerOptions.groupedModels(configuration, idPrefix: source.key + moveSeparator) { _ in false }
            if source.isLogin, !sections.isEmpty {
                sections[0].title = "本机登录 · 换过去时原样搬会话"
            } else if sections.count == 1, sections[0].title == nil {
                sections[0].title = source.title
            }
            menu.sections += sections
        }
        return menu
    }

    public static let moveSeparator = "\u{1F}"

    /// `(source key, model)` for an option that belongs to another source.
    public static func moveTarget(_ id: String) -> (source: String, model: String)? {
        let parts = id.components(separatedBy: moveSeparator)
        guard parts.count == 2 else { return nil }
        return (parts[0], parts[1])
    }

    public static func models(_ declared: [String], current: String?) -> PickerMenu {
        var names = declared
        if let current, !current.isEmpty, !names.contains(current) { names.insert(current, at: 0) }
        let options = names.map { PickerOption(id: $0, title: $0, systemImage: "cpu", selected: $0 == current) }
        return PickerMenu(sections: [PickerSection(id: "models", title: nil, options: options)],
                          note: options.isEmpty ? "此 Agent 没有已存模型目录；可用 /model 名称 直接指定。" : "下一轮生效")
    }

    public static func permissions(current: String?) -> PickerMenu {
        let options = permissionModes.map {
            PickerOption(id: $0, title: ComposerPickerOptions.permissionTitle($0),
                         detail: ComposerPickerOptions.permissionDetail($0),
                         systemImage: ComposerPickerOptions.permissionSymbol($0), selected: $0 == current)
        }
        return PickerMenu(sections: [PickerSection(id: "permissions", title: nil, options: options)], note: "下一轮生效")
    }

    /// nil when there is nothing to choose; `editable` false shows why (Claude: Q12).
    public static func efforts(_ declared: [String], current: String?, editable: Bool) -> PickerMenu? {
        guard !declared.isEmpty else { return nil }
        let reason = editable ? nil : "Claude 的推理强度在启动时确定，会话中不可更改"
        let options = declared.map {
            PickerOption(id: $0, title: ComposerPickerOptions.effortTitle($0), systemImage: "brain",
                         selected: $0 == current, disabledReason: $0 == current ? nil : reason)
        }
        return PickerMenu(sections: [PickerSection(id: "efforts", title: nil, options: options)],
                          note: editable ? "下一轮生效" : reason)
    }
}

/// A model source a session can move to (2026-10-29).
public struct LiveModelSource: Equatable, Sendable {
    /// The bare Agent configuration id; nil for the harness's own login.
    public var configID: String?
    public var title: String
    public var configuration: SessionConfiguration
    public var isLogin: Bool { configID == nil }
    public var key: String { configID ?? "builtin" }

    public init(configID: String?, title: String, configuration: SessionConfiguration) {
        self.configID = configID
        self.title = title
        self.configuration = configuration
    }
}
