import Foundation

/// One row in a composer picker popover: icon + name + grey description,
/// a check on the current item, and a reason instead of a click when the
/// host says the item is unavailable.
public struct PickerOption: Identifiable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var detail: String?
    public var systemImage: String?
    public var selected: Bool
    public var disabledReason: String?
    /// A short note on the same line, right-aligned (2026-10-29: options stay one line).
    public var trailing: String? = nil

    public init(id: String, title: String, detail: String? = nil, systemImage: String? = nil,
                selected: Bool = false, disabledReason: String? = nil) {
        self.id = id
        self.title = title
        self.detail = detail
        self.systemImage = systemImage
        self.selected = selected
        self.disabledReason = disabledReason
    }

    public var isEnabled: Bool { disabledReason == nil }
}

public struct PickerSection: Identifiable, Equatable, Sendable {
    public var id: String
    public var title: String?
    public var options: [PickerOption]
}

/// What a picker shows: sections of options, or a note saying why there is
/// nothing to choose. Never a fabricated default.
public struct PickerMenu: Equatable, Sendable {
    public var sections: [PickerSection]
    public var note: String?

    public var isEmpty: Bool { sections.allSatisfy(\.options.isEmpty) }
}

/// Composer picker options. Every list comes from the host catalogue
/// (`agent.configurations`) or the project list; this type only shapes them.
@MainActor
public enum ComposerPickerOptions {
    public static func selectedConfiguration(_ configurations: [SessionConfiguration],
                                             draft: NewSessionDraft) -> SessionConfiguration? {
        configurations.first {
            $0.id == draft.configurationID && $0.version == draft.configurationVersion
                && $0.agent == draft.task.agent
        }
    }

    public static func workspaces(_ projects: [TaskStore.ProjectEntry], selectedID: String?) -> PickerMenu {
        let options = projects.map {
            PickerOption(id: $0.id, title: $0.name, detail: abbreviatedPath($0.path),
                         systemImage: "folder", selected: $0.id == selectedID)
        }
        return PickerMenu(sections: [PickerSection(id: "projects", title: "项目", options: options)],
                          note: projects.isEmpty ? "先打开项目目录。" : nil)
    }

    /// Configurations grouped by harness, in the harness picker's order.
    /// Blocked configurations stay visible with their first reason.
    /// `remote`: the project is on an SSH host, so the CLI's own login is the
    /// remote machine's and presets reach this Mac's gateway over SSH (G4).
    public static func configurations(_ configurations: [SessionConfiguration],
                                      draft: NewSessionDraft, harness: String? = nil,
                                      remote: Bool = false) -> PickerMenu {
        let selected = selectedConfiguration(configurations, draft: draft)
        if let harness {
            // D: the harness slider already chose the family; split its
            // entries into the CLI's own login and gateway configurations.
            let rows = configurations.filter { $0.harness == harness }
            func option(_ configuration: SessionConfiguration, _ detail: String) -> PickerOption {
                PickerOption(id: "\(configuration.id)@\(configuration.version)", title: configuration.title,
                             detail: detail, systemImage: AgentGlyph.symbol(for: configuration.agent),
                             selected: configuration == selected, disabledReason: configuration.blockedReasons.first)
            }
            let builtIn = rows.filter { !NewSessionLayout.isAgentConfig($0) }
                .map { option($0, remote ? "使用远程机器自己的账号" : "使用本机 CLI 自己的登录") }
            // 2026-10-27: the harness is the slider's; this menu picks the
            // saved connection (and, below, any configuration of your own).
            let gateway = remote ? "经 SSH 转发到本机网关" : "经本机网关"
            let connections = rows.filter(NewSessionLayout.isConnection).map { option($0, gateway) }
            let configs = rows.filter { NewSessionLayout.isAgentConfig($0) && !NewSessionLayout.isConnection($0) }
                .map { option($0, "\(gateway) · 自定义配置") }
            let sections = remote
                ? [
                    PickerSection(id: "connections", title: "经本机网关 · 我的连接", options: connections),
                    PickerSection(id: "configs", title: "经本机网关 · Agent 预设", options: configs),
                    PickerSection(id: "builtin", title: "远程自带登录", options: builtIn),
                ]
                : [
                    PickerSection(id: "builtin", title: "本机登录", options: builtIn),
                    PickerSection(id: "connections", title: "我的连接", options: connections),
                    PickerSection(id: "configs", title: "Agent 预设", options: configs),
                ]
            return PickerMenu(
                sections: sections.filter { !$0.options.isEmpty },
                note: rows.isEmpty ? "\(AgentGlyph.displayName(for: harness)) 还没有可用的登录或连接：安装并登录它，或在侧栏“Agent 配置 → 连接”里新建连接。" : nil
            )
        }
        let known = NativeHarnessPicker.harnesses.map(\.id)
        let harnesses = known + Set(configurations.map(\.harness)).subtracting(known).sorted()
        let sections = harnesses.compactMap { harness -> PickerSection? in
            let rows = configurations.filter { $0.harness == harness }
            guard !rows.isEmpty else { return nil }
            return PickerSection(
                id: harness, title: AgentGlyph.displayName(for: harness),
                options: rows.map {
                    PickerOption(id: "\($0.id)@\($0.version)", title: $0.title,
                                 detail: configurationDetail($0), systemImage: AgentGlyph.symbol(for: $0.agent),
                                 selected: $0 == selected, disabledReason: $0.blockedReasons.first)
                }
            )
        }
        return PickerMenu(sections: sections,
                          note: configurations.isEmpty ? "宿主尚未提供 Agent 配置目录。" : nil)
    }

    public static func permissions(_ configuration: SessionConfiguration?, draft: NewSessionDraft) -> PickerMenu {
        guard let configuration else { return PickerMenu(sections: [], note: "先选择 Agent 配置。") }
        let options = configuration.options.permissionModes.map {
            PickerOption(id: $0, title: permissionTitle($0), detail: permissionDetail($0),
                         systemImage: permissionSymbol($0),
                         selected: $0 == draft.task.permissionMode && draft.permissionAgent == draft.task.agent)
        }
        return PickerMenu(sections: [PickerSection(id: "permissions", title: nil, options: options)],
                          note: options.isEmpty ? "此配置没有声明权限档位。" : nil)
    }

    public static func models(_ configuration: SessionConfiguration?, draft: NewSessionDraft) -> PickerMenu {
        guard let configuration else { return PickerMenu(sections: [], note: "先选择 Agent 配置。") }
        let sections = groupedModels(configuration) { $0 == draft.task.model }
        let empty = sections.allSatisfy { $0.options.isEmpty }
        return PickerMenu(sections: sections,
                          note: empty ? "此配置没有已存模型目录；不会查询 API 或假设默认值。" : nil)
    }

    /// One section per serving connection when the configuration says (「全部
    /// 第三方模型」), else one plain section; notes ride on the same line.
    public static func groupedModels(_ configuration: SessionConfiguration, idPrefix: String = "",
                                     selected: @escaping (String) -> Bool) -> [PickerSection] {
        let option = { (model: String) in
            var row = PickerOption(id: idPrefix + model, title: model, systemImage: "cpu", selected: selected(model))
            row.trailing = configuration.modelNotes[model]
            return row
        }
        let connections = configuration.modelConnections
        let names = configuration.options.models
        guard Set(names.compactMap { connections[$0] }).count > 1 else {
            return [PickerSection(id: "models.\(configuration.id)", title: nil, options: names.map(option))]
        }
        var order: [String] = []
        for name in names {
            if let connection = connections[name], !order.contains(connection) { order.append(connection) }
        }
        return order.map { connection in
            PickerSection(id: "models.\(configuration.id).\(connection)", title: connection,
                          options: names.filter { connections[$0] == connection }.map(option))
        }
    }

    /// nil when the configuration offers no effort levels: the capsule is hidden.
    public static func efforts(_ configuration: SessionConfiguration?, draft: NewSessionDraft) -> PickerMenu? {
        guard let efforts = configuration?.options.efforts, !efforts.isEmpty else { return nil }
        let options = efforts.map {
            PickerOption(id: $0, title: effortTitle($0), detail: $0 == effortTitle($0) ? nil : $0,
                         systemImage: "brain", selected: $0 == draft.task.effort)
        }
        return PickerMenu(sections: [PickerSection(id: "efforts", title: nil, options: options)], note: nil)
    }

    // MARK: - Labels

    public static func permissionTitle(_ mode: String) -> String {
        switch mode {
        case "ask": "每次询问"
        case "auto_edit": "自动编辑"
        case "full_access": "完全访问"
        default: mode
        }
    }

    static func permissionDetail(_ mode: String) -> String? {
        switch mode {
        case "ask": "执行命令和改文件前都先问你"
        case "auto_edit": "改文件不问，执行命令前问"
        case "full_access": "不再询问；受保护规则与停止仍有效"
        default: nil
        }
    }

    static func permissionSymbol(_ mode: String) -> String {
        switch mode {
        case "ask": "hand.raised"
        case "auto_edit": "pencil.and.outline"
        case "full_access": "bolt.shield"
        default: "lock.shield"
        }
    }

    public static func effortTitle(_ effort: String) -> String {
        switch effort.lowercased() {
        case "minimal": "极简"
        case "low": "低"
        case "medium": "中"
        case "high": "高"
        case "xhigh": "超高"
        case "max": "最高"
        default: effort
        }
    }

    static func configurationDetail(_ configuration: SessionConfiguration) -> String {
        if let reason = configuration.blockedReasons.first { return reason }
        let models = configuration.options.models
        switch models.count {
        case 0: return AgentGlyph.displayName(for: configuration.agent)
        case 1: return models[0]
        default: return "\(models[0]) 等 \(models.count) 个模型"
        }
    }

    static func abbreviatedPath(_ path: String) -> String {
        let home = NSHomeDirectory()
        return path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }
}
