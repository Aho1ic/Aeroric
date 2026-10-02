import SwiftUI

/// P5-05 Action 1：设置视图集（Action 1 七视图）。每项设置映射到
/// SettingsStore 的 panelRegistry（G11）；分 UI-only（@AppStorage 合法）
/// 与 shared business（SettingsStore 分界——shared 走调用方提交 core 配置，
/// 不用 @AppStorage 另存一份）。
public struct GeneralSettingsView: View {
    /// UI-only 偏好（窗口尺寸等；UserDefaults 合法）。
    @AppStorage("settings.taskDisplayWindow") private var taskDisplayWindow = false
    @AppStorage("settings.attentionBadge") private var attentionBadge = true
    @AppStorage("settings.sftpLocalDefaultPath") private var sftpLocalDefaultPath = ""
    /// shared business 示例（DSH web search）：经调用方提交，本地只镜像。
    public var onSharedSave: ((_ panelID: String, _ value: String) -> Void)?
    @State private var dshWebSearchEnabled = false
    @State private var saveError: String?

    public init(onSharedSave: ((_ panelID: String, _ value: String) -> Void)? = nil) {
        self.onSharedSave = onSharedSave
    }

    public var body: some View {
        Form {
            Toggle("任务完成时弹出显示窗口", isOn: $taskDisplayWindow)
            Toggle("未读提醒角标", isOn: $attentionBadge)
            TextField("SFTP 本地默认路径", text: $sftpLocalDefaultPath)
            Toggle("DSH 联网搜索", isOn: Binding(
                get: { dshWebSearchEnabled },
                set: { newValue in
                    if let error = SettingsStore.validateSave(panelID: "general", value: "") {
                        saveError = error
                        return
                    }
                    dshWebSearchEnabled = newValue
                    onSharedSave?("general", newValue ? "enabled" : "disabled")
                }
            ))
            if let saveError {
                Text(saveError)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.general")
    }
}

/// 外观设置（theme/fonts：UI-only，@AppStorage 合法）。
public struct AppearanceSettingsView: View {
    @AppStorage("settings.themeMode") private var themeMode = "system"
    @AppStorage("settings.terminalFontSize") private var terminalFontSize = 13.0
    @AppStorage("settings.uiFontFamily") private var uiFontFamily = ""
    @AppStorage("settings.monoFontFamily") private var monoFontFamily = ""

    public init() {}

    public var body: some View {
        Form {
            Picker("主题", selection: $themeMode) {
                Text("跟随系统").tag("system")
                Text("浅色").tag("light")
                Text("深色").tag("dark")
            }
            Stepper("终端字号 \(Int(terminalFontSize))", value: $terminalFontSize, in: 8...32)
            TextField("界面字体", text: $uiFontFamily)
            TextField("等宽字体", text: $monoFontFamily)
        }
        .padding(16)
        .accessibilityIdentifier("settings.appearance")
    }
}

/// Agent 设置（all-agent-configs：shared business，经调用方提交 core）。
public struct AgentSettingsView: View {
    public var agents: [SettingsCapabilityAgent]
    public var onSharedSave: ((_ panelID: String, _ value: String) -> Void)?

    public init(
        agents: [SettingsCapabilityAgent] = [],
        onSharedSave: ((_ panelID: String, _ value: String) -> Void)? = nil
    ) {
        self.agents = agents
        self.onSharedSave = onSharedSave
    }

    public var body: some View {
        List {
            ForEach(agents, id: \.id) { agent in
                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.id)
                    Text(agent.family)
                        .font(.caption)
                        .foregroundStyle(Color.dsTextMuted)
                }
                .accessibilityIdentifier("settings.agent.\(agent.id)")
            }
        }
        .accessibilityIdentifier("settings.agents")
    }
}

/// Agent 条目（四族别名＋自定义档案；SettingsCapability.familyOf 判定）。
public struct SettingsCapabilityAgent: Identifiable, Equatable, Sendable {
    public var id: String
    public var family: String
    public var isCustom: Bool

    public init(id: String, family: String, isCustom: Bool = false) {
        self.id = id
        self.family = family
        self.isCustom = isCustom
    }
}

/// 自定义档案编辑器（新增/编辑自定义 agent；family 显式声明可选）。
public struct CustomAgentEditor: View {
    public var onSave: (_ id: String, _ family: String?) -> Void
    @State private var agentID = ""
    @State private var family = ""

    public init(onSave: @escaping (_ id: String, _ family: String?) -> Void) {
        self.onSave = onSave
    }

    public var body: some View {
        Form {
            TextField("Agent ID", text: $agentID)
                .accessibilityIdentifier("custom-agent.id")
            Picker("Family（可选）", selection: $family) {
                Text("自动").tag("")
                ForEach(["claude", "codex", "dsh", "omp"], id: \.self) { f in
                    Text(f).tag(f)
                }
            }
            Button("保存") {
                let resolved = SettingsCapability.familyOf(
                    agentID: agentID, declared: family.isEmpty ? nil : family
                )
                onSave(agentID, resolved)
            }
            .disabled(agentID.isEmpty)
            .accessibilityIdentifier("custom-agent.save")
        }
        .padding(16)
        .accessibilityIdentifier("settings.custom-agent")
    }
}

/// 模型设置（usage/models：capability 门控——effort 白名单外不发）。
public struct ModelSettingsView: View {
    public var family: String
    public var onEffortChange: ((_ effort: String) -> Void)?
    @State private var selectedEffort = ""

    public init(family: String, onEffortChange: ((_ effort: String) -> Void)? = nil) {
        self.family = family
        self.onEffortChange = onEffortChange
    }

    private var allowedEfforts: [String] {
        // capability 门控（SettingsCapability；OMP 白名单与 DSH 分家）。
        switch family {
        case "omp": SettingsCapability.ompThinkingLevels
        case "dsh", "claude", "codex": SettingsCapability.dshEffortLevels
        default: []
        }
    }

    public var body: some View {
        Form {
            if allowedEfforts.isEmpty {
                Text("该 Agent 家不支持 effort 设置")
                    .foregroundStyle(Color.dsTextMuted)
            } else {
                Picker("Effort", selection: $selectedEffort) {
                    ForEach(allowedEfforts, id: \.self) { effort in
                        Text(effort).tag(effort)
                    }
                }
                Button("应用") { onEffortChange?(selectedEffort) }
                    .disabled(selectedEffort.isEmpty)
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.models")
    }
}

/// 快捷键设置（shortcuts：UI-only）。
public struct ShortcutSettingsView: View {
    @AppStorage("settings.paletteShortcut") private var paletteShortcut = "⌘K"

    public init() {}

    public var body: some View {
        Form {
            TextField("命令面板快捷键", text: $paletteShortcut)
        }
        .padding(16)
        .accessibilityIdentifier("settings.shortcuts")
    }
}

/// Agent 升级视图（Action 4：显示安装渠道/影响任务/进度；用户确认后执行，
/// 不因打开设置自动 upgrade）。
public struct AgentUpdateView: View {
    public var onUpdate: ((_ channel: String) -> Void)?
    @State private var channel = "npm"
    @State private var confirmed = false
    @State private var inFlight = false

    public init(onUpdate: ((_ channel: String) -> Void)? = nil) {
        self.onUpdate = onUpdate
    }

    public var body: some View {
        Form {
            Picker("安装渠道", selection: $channel) {
                ForEach(SettingsCapability.updateChannels, id: \.self) { channel in
                    Text(channel).tag(channel)
                }
            }
            .accessibilityIdentifier("agent-update.channel")
            Text("升级会影响正在运行的任务；确认后执行。")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
            Toggle("我已确认", isOn: $confirmed)
                .accessibilityIdentifier("agent-update.confirm")
            Button("升级") {
                guard SettingsCapability.canStartUpdate(channel: channel, confirmedByUser: confirmed) else {
                    return
                }
                inFlight = true
                onUpdate?(channel)
            }
            .disabled(!confirmed || inFlight)
            .accessibilityIdentifier("agent-update.start")
            if inFlight {
                ProgressView()
                    .accessibilityIdentifier("agent-update.progress")
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.agent-updates")
    }
}

/// 设置搜索索引（panelRegistry 全映射的检索；G11 全 19 panel 可搜）。
public struct SettingsSearchIndex {
    public let entries: [SettingsStore.Panel]

    @MainActor
    public init(store: SettingsStore) {
        self.entries = store.availablePanels()
    }

    /// 按查询过滤面板（title/id 双匹配；空白＝全部）。
    public func search(_ query: String) -> [SettingsStore.Panel] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return entries }
        return entries.filter {
            $0.id.localizedCaseInsensitiveContains(trimmed)
                || $0.section.rawValue.localizedCaseInsensitiveContains(trimmed)
        }
    }
}
