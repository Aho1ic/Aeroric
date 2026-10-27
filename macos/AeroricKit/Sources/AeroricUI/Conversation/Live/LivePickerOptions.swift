import Foundation

/// Facts the session page shows but the host's `session.state` does not carry:
/// from the task record, the project and the Agent's configurations.
public struct LiveSessionFacts: Equatable, Sendable {
    public var agent: String?
    public var configurationTitle: String?
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

    public init(agent: String?, configurationTitle: String? = nil, workingDirectory: String? = nil,
                branch: String? = nil, startedAt: Date? = nil, additions: UInt64? = nil, deletions: UInt64? = nil,
                models: [String] = [], efforts: [String] = [],
                downgradeBadge: String? = nil, downgradeHelp: String? = nil, initialPrompt: String? = nil) {
        self.agent = agent
        self.configurationTitle = configurationTitle
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
