import AeroricClient
import AeroricProtocol
import AppKit
import SwiftUI
import UniformTypeIdentifiers

// Settings building blocks (M3). Only settings that something actually reads
// live here; unwired ones were removed rather than shown as working (D4).

/// Settings window navigation. 1124 (settings redesign, note
/// `2026-11-24-settings-redesign`): grouped like the reference apps' settings
/// — 应用 / Agents / 数据与隐私 / 集成 / 编码, 关于 last — each page with a
/// one-line subtitle.
public enum SettingsPane: String, CaseIterable, Identifiable, Sendable {
    case general, appearance, notifications, shortcuts
    case agents, legacyCleanup
    case imports, storage, memory, diagnostics
    case skills, references, remote, connection
    case git, worktrees
    case about

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .general: "通用"
        case .appearance: "外观"
        case .notifications: "通知"
        case .shortcuts: "键盘快捷键"
        case .agents: "新会话与 Agent"
        case .legacyCleanup: "旧版 Agent 清理"
        case .imports: "导入"
        case .storage: "存储"
        case .memory: "记忆与整理"
        case .diagnostics: "诊断"
        case .skills: "技能与扩展"
        case .references: "参考代码库"
        case .remote: "远程与手机"
        case .connection: "宿主连接"
        case .git: "Git"
        case .worktrees: "工作副本"
        case .about: "关于"
        }
    }

    public var subtitle: String {
        switch self {
        case .general: "启动引导、菜单栏、电源与 Agent 进程"
        case .appearance: "主题、材质、字体与字号"
        case .notifications: "什么时候用系统通知提醒你"
        case .shortcuts: "Aeroric 里能用的键盘快捷键"
        case .agents: "新会话的默认选择、内置 Agent、Agent 配置与求助专家"
        case .legacyCleanup: "清理旧版自定义 Agent 留下的配置"
        case .imports: "把 Claude Code、Codex 等已有的会话导入 Aeroric"
        case .storage: "Aeroric 的数据占了多少磁盘，各放在哪"
        case .memory: "项目记忆、后台整理与按意思找"
        case .diagnostics: "导出脱敏的诊断报告，用来排查问题"
        case .skills: "Agent 能用的技能，以及 MCP 服务器、Hook 与插件"
        case .references: "给 Agent 查阅的外部代码库"
        case .remote: "与手机配对、管理已配对的设备、查看远程访问记录"
        case .connection: "Aeroric 宿主的连接状态与协议版本"
        case .git: "Aeroric 写提交说明时加的要求"
        case .worktrees: "会话的独立副本放在哪、什么时候清理，以及现有的副本"
        case .about: "版本与构建信息"
        }
    }

    public var systemImage: String {
        switch self {
        case .general: "gearshape"
        case .appearance: "paintpalette"
        case .notifications: "bell"
        case .shortcuts: "keyboard"
        case .agents: "cpu"
        case .legacyCleanup: "trash"
        case .imports: "tray.and.arrow.down"
        case .storage: "internaldrive"
        case .memory: "brain.head.profile"
        case .diagnostics: "stethoscope"
        case .skills: "puzzlepiece.extension"
        case .references: "books.vertical"
        case .remote: "iphone.radiowaves.left.and.right"
        case .connection: "bolt.horizontal"
        case .git: "arrow.triangle.branch"
        case .worktrees: "square.on.square"
        case .about: "info.circle"
        }
    }

    public var group: SettingsGroup {
        switch self {
        case .general, .appearance, .notifications, .shortcuts: .app
        case .agents, .legacyCleanup: .agents
        case .imports, .storage, .memory, .diagnostics: .data
        case .skills, .references, .remote, .connection: .integrations
        case .git, .worktrees: .coding
        case .about: .about
        }
    }

    /// Pages that bring their own scrolling (a grouped `Form`).
}

/// The left column's groups, in order; 关于 has no heading.
public enum SettingsGroup: String, CaseIterable, Sendable {
    case app, agents, data, integrations, coding, about

    public var title: String? {
        switch self {
        case .app: "应用"
        case .agents: "Agents"
        case .data: "数据与隐私"
        case .integrations: "集成"
        case .coding: "编码"
        case .about: nil
        }
    }
}

/// 1124: what the settings search looks through — every row's title and
/// explanation, by page. Kept beside the rows it names; a test holds the
/// common words to their rows.
public enum SettingsIndex {
    public struct Entry: Equatable, Sendable, Identifiable {
        public var pane: SettingsPane
        /// The row's `SettingsRow` anchor (its title unless the row says otherwise).
        public var anchor: String
        public var detail: String
        public var id: String { "\(pane.rawValue).\(anchor)" }
    }

    static func entry(_ pane: SettingsPane, _ anchor: String, _ detail: String = "") -> Entry {
        Entry(pane: pane, anchor: anchor, detail: detail)
    }

    public static let entries: [Entry] = [
        entry(.general, "重新打开引导", "首次启动 检测 Agent 打开项目 导入"),
        entry(.general, "会话呈现", "原生 终端"),
        entry(.general, "运行任务时阻止 Mac 睡眠", "电源 睡眠 唤醒"),
        entry(.general, "空闲后释放 Agent 进程", "进程 空闲 释放"),
        entry(.general, "无人值守的会话被打断或没跑成时自动继续", "重启 续跑 重试 额度"),
        entry(.general, "在菜单栏显示 Aeroric", "菜单栏 状态栏"),
        entry(.general, "导入的会话显示“已导入”", "侧栏"),
        entry(.appearance, "外观", "主题 浅色 深色 跟随系统 暗色"),
        entry(.appearance, "材质", "玻璃 经典 透明"),
        entry(.appearance, "会话中文字体", "字体 更纱 苹方"),
        entry(.appearance, "正文", "字号 界面文字 大小"),
        entry(.appearance, "会话", "字号 消息 大小"),
        entry(.appearance, "代码", "字号 diff 大小"),
        entry(.appearance, "终端", "字号 大小"),
        entry(.notifications, "需要我批准时", "审批 通知 提醒"),
        entry(.notifications, "会话做完或失败时", "完成 失败 通知 提醒"),
        entry(.notifications, "提示音", "声音 通知"),
        entry(.shortcuts, "新建会话", "⌘N 快捷键"),
        entry(.shortcuts, "命令面板", "⌘K 快捷键"),
        entry(.shortcuts, "发送", "⌘↩ 快捷键"),
        entry(.agents, "默认 Harness", "新会话 Codex Claude DSH omp"),
        entry(.agents, "每个项目记住新会话的选择", "配置 模型 思考强度 权限 记住"),
        entry(.agents, "管理 Agent 配置", "第三方 API 连接 号池 路由 升级"),
        entry(.agents, "专家模型", "求助专家 Smart Friend"),
        entry(.legacyCleanup, "旧版 Agent 清理", "自定义 Agent"),
        entry(.imports, "选择要导入的会话", "导入 Claude Code Codex"),
        entry(.storage, "数据目录", "存储 占用 磁盘 空间 大小 访达 工作副本 日志"),
        entry(.memory, "打开记忆页", "记忆 整理 做梦 embedding 按意思找"),
        entry(.diagnostics, "导出诊断报告", "日志 排查 诊断"),
        entry(.skills, "技能目录", "技能 skill"),
        entry(.skills, "MCP 服务器", "mcp"),
        entry(.skills, "Hook", "钩子 hook"),
        entry(.skills, "插件", "plugin"),
        entry(.references, "参考代码库", "索引 参考 文件夹"),
        entry(.remote, "启用远程访问", "手机 配对 二维码 设备 远控 Relay 推送"),
        entry(.remote, "允许手机改文件", "手机 写文件 远程写入 权限"),
        entry(.connection, "连接状态", "宿主 协议版本 环境"),
        entry(.git, "提交说明的附加指引", "提交 commit 生成 提示"),
        entry(.worktrees, "工作副本根目录", "worktree 目录 位置"),
        entry(.worktrees, "没有改动的副本保留多久", "worktree 清理 保留 自动删除 独立副本"),
        entry(.about, "构建", "版本 应用标识"),
    ]

    /// Rows whose page, title or words contain every word of `query`.
    public static func search(_ query: String, in panes: [SettingsPane]) -> [Entry] {
        let words = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !words.isEmpty else { return [] }
        return entries.filter { entry in
            guard panes.contains(entry.pane) else { return false }
            let haystack = [entry.pane.title, entry.pane.subtitle, entry.anchor, entry.detail]
                .joined(separator: " ").lowercased()
            return words.allSatisfy { haystack.contains($0) }
        }
    }
}

/// App-wide light / dark / system choice, applied through `NSApp.appearance`
/// so every window (main, settings, sheets) follows it.
public enum AppTheme: String, CaseIterable, Identifiable, Sendable {
    case system, light, dark

    public static let storageKey = "settings.themeMode"
    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .system: "跟随系统"
        case .light: "浅色"
        case .dark: "深色"
        }
    }

    public var appearance: NSAppearance? {
        switch self {
        case .system: nil
        case .light: NSAppearance(named: .aqua)
        case .dark: NSAppearance(named: .darkAqua)
        }
    }

    public static var stored: AppTheme {
        UserDefaults.standard.string(forKey: storageKey).flatMap(AppTheme.init(rawValue:)) ?? .system
    }

    @MainActor public static func applyStored() {
        NSApp.appearance = stored.appearance
    }
}

/// A group of settings rows under a grey heading. 1124: flat — rows are
/// split by hairlines, the group has no border or fill (the reference apps'
/// look; a page of bordered cards read as a wall of boxes).
public struct SettingsCard<Content: View>: View {
    let title: String?
    let content: Content

    public init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            if let title {
                Text(title)
                    .font(.system(size: AeroricType.caption, weight: .medium))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            VStack(alignment: .leading, spacing: 0) { content }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The row search landed on, flashed once (1124).
struct SettingsHighlightKey: EnvironmentKey {
    static let defaultValue: String? = nil
}

extension EnvironmentValues {
    var settingsHighlight: String? {
        get { self[SettingsHighlightKey.self] }
        set { self[SettingsHighlightKey.self] = newValue }
    }
}

/// Title and grey explanation on the left, the control on the right.
public struct SettingsRow<Control: View>: View {
    let title: String
    let detail: String?
    let anchor: String
    let control: Control
    @Environment(\.settingsHighlight) private var highlight

    /// `anchor` names the row for search (`SettingsIndex`); the title by default.
    public init(_ title: String, detail: String? = nil, anchor: String? = nil, @ViewBuilder control: () -> Control) {
        self.title = title
        self.detail = detail
        self.anchor = anchor ?? title
        self.control = control()
    }

    public var body: some View {
        HStack(alignment: .center, spacing: AeroricSpace.xl) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(size: AeroricType.body))
                if let detail {
                    Text(detail)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: AeroricSpace.l)
            control
        }
        .padding(.horizontal, AeroricSpace.s)
        .padding(.vertical, AeroricSpace.m + 2)
        .frame(minHeight: 44)
        .background(AeroricRadius.shape(AeroricRadius.s)
            .fill(highlight == anchor ? AeroricColor.accent.opacity(0.14) : .clear))
        .overlay(alignment: .top) { HairlineDivider() }
        .animation(.easeOut(duration: 0.4), value: highlight == anchor)
        .id(SettingsRowAnchor.id(anchor))
    }
}

enum SettingsRowAnchor {
    static func id(_ anchor: String) -> String { "settings.row.\(anchor)" }
}

// MARK: - Panes

struct GeneralSettingsPane: View {
    @AppStorage(OnboardingFlow.completedKey) private var onboardingCompleted = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            interaction
            SettingsCard("引导") {
                SettingsRow("重新打开引导", detail: "回到首次启动的引导页：检测 Agent、打开项目、导入已有会话。") {
                    Button("打开") { onboardingCompleted = false }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .accessibilityIdentifier("settings.general.reopen-onboarding")
                }
            }
        }
    }

    /// ADR-043 (2026-10-27): every Claude Code, Codex and omp session — local
    /// or over SSH — is native; there is no terminal mode to switch to.
    private var interaction: some View {
        SettingsCard("交互") {
            SettingsRow("会话呈现",
                        detail: "Claude Code、Codex 与 omp 在本机和 SSH 远程都使用原生会话（对话、审批卡、工具卡）。只有尚未迁移的自定义包装 Agent 仍在终端中运行。") {
                Text("原生")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("settings.general.interaction-mode")
            }
        }
    }
}

/// 2026-11-06: what the sidebar shows (imported sessions' 已导入 tag, off by default).
struct SidebarSettingsCard: View {
    @AppStorage(SidebarPreferences.showImportedKey) private var showImported = false

    var body: some View {
        SettingsCard("侧栏") {
            SettingsRow("导入的会话显示“已导入”",
                        detail: "从 Claude Code、Codex 等导入的会话在侧栏标出来源；关闭时和普通会话一样显示时间。") {
                Toggle("", isOn: $showImported)
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .accessibilityIdentifier("settings.sidebar.show-imported")
            }
        }
    }
}

/// A16: the menu bar extra (on by default; the App owns the scene, this only
/// flips the shared preference).
struct MenuBarSettingsCard: View {
    @AppStorage("menuBarExtra.shown") private var shown = true

    var body: some View {
        SettingsCard("菜单栏") {
            SettingsRow("在菜单栏显示 Aeroric", detail: "显示运行中与待回应的数量；关掉所有窗口后，宿主仍在后台执行任务。") {
                Toggle("", isOn: $shown)
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .accessibilityIdentifier("settings.menu-bar")
            }
        }
    }
}

/// A14: keep the Mac awake while tasks run (host-held; on by default).
struct KeepAwakeSettingsCard: View {
    let store: KeepAwakeStore

    var body: some View {
        SettingsCard("电源") {
            SettingsRow("运行任务时阻止 Mac 睡眠", detail: detail) {
                Toggle("", isOn: Binding(
                    get: { store.state?.enabled ?? true },
                    set: { on in Task { await store.setEnabled(on) } }
                ))
                .toggleStyle(.switch)
                .labelsHidden()
                .accessibilityIdentifier("settings.keep-awake")
            }
        }
        .task { await store.refresh() }
    }

    private var detail: String {
        guard let state = store.state else { return "有任务在运行或排队时，阻止 Mac 进入空闲睡眠；合盖睡眠不受影响。" }
        if state.active { return "正在阻止睡眠：\(state.activeTasks) 个任务在运行。" }
        return state.enabled ? "当前没有任务在运行，Mac 可以正常睡眠。" : "已关闭：任务运行时 Mac 也可能进入空闲睡眠。"
    }
}

/// A14b (2026-11-07): release an idle Agent's process after an hour
/// (host-held; on by default). Never while it has background work.
struct IdleReleaseSettingsCard: View {
    let store: IdleReleaseStore
    /// 2026-11-14: 宿主重启后自动继续, beside it.
    var resume: ResumeAfterRestartStore?

    var body: some View {
        SettingsCard("Agent 进程") {
            SettingsRow("空闲 \(hours)后释放 Agent 进程", detail: detail, anchor: "空闲后释放 Agent 进程") {
                Toggle("", isOn: Binding(
                    get: { store.enabled },
                    set: { on in Task { await store.setEnabled(on) } }
                ))
                .toggleStyle(.switch)
                .labelsHidden()
                .accessibilityIdentifier("settings.idle-release")
            }
            if let resume, resume.isAvailable {
                SettingsRow("无人值守的会话被打断或没跑成时自动继续", detail: Self.resumeDetail(resume)) {
                    Toggle("", isOn: Binding(
                        get: { resume.enabled },
                        set: { on in Task { await resume.setEnabled(on) } }
                    ))
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .accessibilityIdentifier("settings.resume-after-restart")
                }
            }
        }
        .task {
            await store.refresh()
            await resume?.refresh()
        }
    }

    static func resumeDetail(_ resume: ResumeAfterRestartStore) -> String {
        if let error = resume.lastError { return "没能保存：\(error)" }
        return resume.enabled
            ? "定时任务开的会话、委派出去的子会话和交办出去的待办：被 Aeroric 重启打断，启动几秒后接着做一次；某一轮因为网络或服务出错没跑成，1、5、15 分钟后各重试一次；额度用完，重置后（不知道重置时间就 30 分钟后）重试一次。你自己开的会话不自动继续。"
            : "已关闭：被重启打断的会话等你点“继续”，没跑成的会话不自动重试。"
    }

    private var hours: String {
        store.idleMinutes % 60 == 0 ? "\(store.idleMinutes / 60) 小时" : "\(store.idleMinutes) 分钟"
    }

    private var detail: String {
        if let error = store.lastError { return "没能保存：\(error)" }
        guard store.enabled else { return "已关闭：从不自动释放，进程一直保留到你关闭会话。" }
        return "没有回合、没有待审批、也没有后台任务的会话，空闲 \(hours)后结束进程；下次发消息自动续接。后台命令或后台子 Agent 还在跑时不释放。关闭后从不自动释放。"
    }
}

struct AppearanceSettingsPane: View {
    @AppStorage(AppTheme.storageKey) private var themeRaw = AppTheme.system.rawValue
    @AppStorage("settings.surfacePreference") private var surfaceRaw = SurfacePreference.glass.rawValue
    @AppStorage(LegacySurfacePreview.storageKey) private var legacySurfacePreview = false
    private let scale = AeroricTypeScale.shared

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            SettingsCard("主题") {
                SettingsRow("外观", detail: "浅色、深色，或跟随系统设置。") {
                    CapsuleSlider(selection: $themeRaw, groupTitle: "外观", identifier: "settings.appearance.theme",
                                  segments: AppTheme.allCases.map { .init(value: $0.rawValue, title: $0.title) })
                    .fixedSize()
                }
                SettingsRow("材质", detail: "玻璃：侧栏、工具栏和浮层使用系统玻璃效果。经典：全部不透明。系统“降低透明度”开启时总是不透明。") {
                    CapsuleSlider(selection: $surfaceRaw, groupTitle: "材质", identifier: "settings.appearance.surface",
                                  segments: [
                                      .init(value: SurfacePreference.glass.rawValue, title: "玻璃"),
                                      .init(value: SurfacePreference.classic.rawValue, title: "经典"),
                                  ])
                    .fixedSize()
                }
            }
            SettingsCard("字体") {
                ConversationFontSettingsRow()
            }
            SettingsCard("字号") {
                sizeRow(.text, title: "正文", detail: "界面文字")
                sizeRow(.conversation, title: "会话", detail: "会话里的消息正文与代码块")
                sizeRow(.code, title: "代码", detail: "代码、diff 与工具调用详情")
                sizeRow(.terminal, title: "终端", detail: "内嵌终端")
            }
            if LegacySurfacePreview.isAvailable {
                SettingsCard("调试") {
                    SettingsRow("预览旧系统外观", detail: "在 macOS 26 上强制使用 macOS 14–25 的材质回退，仅调试构建可见。") {
                        Toggle("预览旧系统外观", isOn: $legacySurfacePreview)
                            .toggleStyle(.switch)
                            .labelsHidden()
                            .accessibilityIdentifier("settings.debug.legacy-surface")
                    }
                }
            }
        }
        // After the pill's slide: switching the appearance redraws every
        // window, which would cut the animation short.
        .onChange(of: themeRaw) { _, _ in
            Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(320))
                AppTheme.applyStored()
            }
        }
    }

    private func sizeRow(_ kind: AeroricTypeScale.Kind, title: String, detail: String) -> some View {
        SettingsRow(title, detail: "\(detail) · 当前 \(Int(kind.size(scale.step(for: kind)))) pt") {
            CapsuleSlider(selection: Binding(get: { scale.step(for: kind) }, set: { scale.set($0, for: kind) }),
                          groupTitle: title, identifier: "settings.appearance.size.\(kind.rawValue)",
                          segments: AeroricTypeScale.Step.allCases.map { .init(value: $0, title: $0.title) })
            .fixedSize()
        }
    }
}

struct AgentsSettingsPane: View {
    var openAgentConfig: (@MainActor () -> Void)?
    /// 2026-11-13 Smart Friend.
    var expert: ExpertStore?
    @AppStorage(NewSessionLayout.defaultHarnessKey) private var defaultHarness = "codex"

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            SettingsCard("内置 Agent") {
                ForEach(NativeHarnessPicker.harnesses) { harness in
                    SettingsRow(harness.title, detail: "协议族：\(harness.id)") {
                        AgentGlyph(agent: harness.id, size: 18)
                    }
                }
            }
            SettingsCard("新会话") {
                SettingsRow("默认 Harness", detail: "新建会话时右上角滑块默认选中的一项；它不可用时选第一个可用的。") {
                    // A harness slider: icons, only the selected one named (as on the new-session page).
                    CapsuleSlider(selection: $defaultHarness, groupTitle: "默认 Harness", mode: .iconsWithSelectedTitle,
                                  identifier: "settings.agents.default-harness",
                                  segments: NewSessionLayout.harnessOrder.map { id in
                                      .init(value: id, title: AgentConfigText.harnessTitle(id),
                                            asset: NativeHarnessPicker.harnesses.first { $0.id == id }?.asset)
                                  })
                        .fixedSize()
                }
                SettingsRow("每个项目记住新会话的选择",
                            detail: "在一个项目里选过的 Agent 配置、模型、思考强度和权限，下次新建会话、重启后都还在；切换 Harness 时换到同一个连接在那个 Harness 下的配置。") {
                    Text("自动")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            if let expert, expert.isAvailable {
                ExpertSettingsCard(store: expert)
            }
            SettingsCard {
                SettingsRow("管理 Agent 配置", detail: "第三方 API 连接、号池、模型路由、Agent 预设与 Agent 升级，在主窗口侧栏的“Agent 配置”页。") {
                    if let openAgentConfig {
                        Button("打开 Agent 配置") { openAgentConfig() }
                            .accessibilityIdentifier("settings.agents.open-config")
                    } else {
                        ComingSoonBadge()
                    }
                }
            }
        }
        .accessibilityIdentifier("settings.agents")
    }
}

/// 2026-11-13 Smart Friend: pick a configuration, then a model; the model
/// chip sets the expert at once.
struct ExpertSettingsCard: View {
    let store: ExpertStore
    @State private var picked: String?

    private var shown: LiveExpertChoice? {
        let id = picked ?? store.expert?.configurationId
        return store.choices.first { $0.configurationId == id } ?? store.choices.first
    }

    var body: some View {
        SettingsCard("求助专家") {
            SettingsRow("专家模型", detail: ExpertText.detail) {
                HStack(spacing: AeroricSpace.s) {
                    Text(ExpertText.current(store.expert))
                        .foregroundStyle(store.expert == nil ? AeroricColor.textTertiary : AeroricColor.text)
                        .accessibilityIdentifier("settings.expert.current")
                    if store.expert != nil {
                        Button("不用专家") { Task { await store.clear() } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .accessibilityIdentifier("settings.expert.clear")
                    }
                }
            }
            if store.choices.isEmpty {
                Text("没有能用作专家的配置：需要一个能启动的 Claude Code 或 Codex 配置")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            } else {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    FlowLayout(spacing: AeroricSpace.xs) {
                        ForEach(store.choices) { choice in
                            Button(choice.title) { picked = choice.configurationId }
                                .buttonStyle(AeroricChipStyle(selected: choice.configurationId == shown?.configurationId))
                                .accessibilityIdentifier("settings.expert.config.\(choice.configurationId)")
                        }
                    }
                    if let shown {
                        FlowLayout(spacing: AeroricSpace.xs) {
                            ForEach(ExpertText.models(shown), id: \.self) { model in
                                Button(model ?? "默认模型") {
                                    Task { await store.set(shown.configurationId, model: model) }
                                }
                                .buttonStyle(AeroricChipStyle(selected: ExpertText.isCurrent(store.expert, shown, model: model)))
                                .accessibilityIdentifier("settings.expert.model.\(model ?? "default")")
                            }
                        }
                    }
                }
                .padding(.vertical, AeroricSpace.xs)
            }
            if let error = store.lastError {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
            }
        }
        .accessibilityIdentifier("settings.expert")
    }
}

struct ConnectionSettingsPane<State: SettingsStateProviding>: View {
    let state: State

    var body: some View {
        SettingsCard("宿主") {
            SettingsRow("连接状态") {
                Text(state.connectionPhaseText ?? "未知")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("settings.connection.phase")
            }
            SettingsRow("协议版本") {
                Text(state.protocolVersionText)
                    .font(.system(size: AeroricType.secondary, design: .monospaced))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("settings.protocol")
            }
            SettingsRow("环境") {
                Text(state.environmentLabel)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("settings.environment")
            }
        }
    }
}

struct AboutSettingsPane<State: SettingsStateProviding>: View {
    let state: State

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            HStack(spacing: AeroricSpace.l) {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 56, height: 56)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Aeroric").font(.system(size: AeroricType.title, weight: .semibold))
                    Text("多 Agent 协作工作台").foregroundStyle(AeroricColor.textSecondary)
                }
            }
            SettingsCard {
                SettingsRow("构建") {
                    Text(state.buildIdentifier)
                        .font(.system(size: AeroricType.secondary, design: .monospaced))
                        .textSelection(.enabled)
                        .accessibilityIdentifier("settings.build")
                }
                SettingsRow("应用标识") {
                    Text(state.applicationTag)
                        .font(.system(size: AeroricType.secondary, design: .monospaced))
                        .accessibilityIdentifier("settings.application")
                }
            }
        }
        .accessibilityIdentifier("settings.about")
    }
}

/// 1124: which system notifications Aeroric posts (read by the app's
/// notifier; approval on and finished turns off by default).
public enum NotificationPreferences {
    public static let approvalKey = "notifications.approval"
    public static let finishedKey = "notifications.finished"
    public static let soundKey = "notifications.sound"

    public static var approval: Bool { UserDefaults.standard.object(forKey: approvalKey) as? Bool ?? true }
    public static var finished: Bool { UserDefaults.standard.object(forKey: finishedKey) as? Bool ?? false }
    public static var sound: Bool { UserDefaults.standard.object(forKey: soundKey) as? Bool ?? true }
}

struct NotificationSettingsPane: View {
    @AppStorage(NotificationPreferences.approvalKey) private var approval = true
    @AppStorage(NotificationPreferences.finishedKey) private var finished = false
    @AppStorage(NotificationPreferences.soundKey) private var sound = true

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            SettingsCard("系统通知") {
                SettingsRow("需要我批准时", detail: "Aeroric 不在前台时，会话等你批准命令或改文件，就发一条通知；点它回到那张审批卡。") {
                    switchControl($approval, "settings.notifications.approval")
                }
                SettingsRow("会话做完或失败时", detail: "Aeroric 不在前台时，一个会话从运行中变成已完成或失败，就发一条通知。定时任务和委派开的会话也算。") {
                    switchControl($finished, "settings.notifications.finished")
                }
                SettingsRow("提示音", detail: "通知带系统默认提示音。") {
                    switchControl($sound, "settings.notifications.sound")
                }
            }
            Text("系统设置 › 通知 里关掉 Aeroric 时，这里打开也收不到。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
    }

    private func switchControl(_ value: Binding<Bool>, _ identifier: String) -> some View {
        Toggle("", isOn: value).toggleStyle(.switch).labelsHidden().accessibilityIdentifier(identifier)
    }
}

/// 1124: the shortcuts Aeroric has, read-only.
struct ShortcutsSettingsPane: View {
    struct Shortcut: Identifiable {
        let title: String
        let keys: String
        var note: String? = nil
        var id: String { title }
    }

    struct Group: Identifiable {
        let title: String
        let items: [Shortcut]
        var id: String { title }
    }

    static let groups: [Group] = [
        Group(title: "窗口与导航", items: [
            Shortcut(title: "新建会话", keys: "⌘N"),
            Shortcut(title: "新建窗口", keys: "⇧⌘N"),
            Shortcut(title: "打开项目…", keys: "⌘O"),
            Shortcut(title: "命令面板", keys: "⌘K"),
            Shortcut(title: "设置…", keys: "⌘,"),
            Shortcut(title: "关闭标签或窗口", keys: "⌘W"),
            Shortcut(title: "折叠或展开侧栏", keys: "⌃⌘S"),
            Shortcut(title: "显示或隐藏辅助面板", keys: "⌥⌘I"),
        ]),
        Group(title: "会话输入框", items: [
            Shortcut(title: "发送", keys: "⌘↩"),
            Shortcut(title: "这轮结束后再发", keys: "⌥↩", note: "一轮正在跑时"),
            Shortcut(title: "问答卡选第 n 项", keys: "1 – 9", note: "Agent 提问时"),
        ]),
        Group(title: "编辑", items: [
            Shortcut(title: "保存", keys: "⌘S", note: "文件、Agent 配置和笔记的编辑器里"),
        ]),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxl) {
            ForEach(Self.groups) { group in
                SettingsCard(group.title) {
                    ForEach(group.items) { item in
                        SettingsRow(item.title, detail: item.note) {
                            Text(item.keys)
                                .font(.system(size: AeroricType.secondary, design: .rounded).weight(.medium))
                                .foregroundStyle(AeroricColor.textSecondary)
                                .padding(.horizontal, AeroricSpace.s)
                                .frame(minHeight: 22)
                                .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.xs))
                        }
                    }
                }
            }
            Text("快捷键现在不能改。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
    }
}

/// A row that sends the person to a page of the main window.
struct SettingsOpenRow: View {
    let title: String
    let detail: String
    let button: String
    let open: (@MainActor () -> Void)?
    var identifier: String

    var body: some View {
        SettingsRow(title, detail: detail) {
            if let open {
                Button(button) { open() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .accessibilityIdentifier(identifier)
            } else {
                ComingSoonBadge()
            }
        }
    }
}

struct MemorySettingsPane: View {
    var open: (@MainActor () -> Void)?

    var body: some View {
        SettingsCard {
            SettingsOpenRow(title: "打开记忆页",
                            detail: "每个项目的记忆、待确认的候选、后台整理（做梦）、整理记录和“按意思找”的开关都在主窗口的“记忆”页。",
                            button: "打开记忆", open: open, identifier: "settings.memory.open")
        }
    }
}

struct SkillsSettingsPane: View {
    var openSkills: (@MainActor () -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            SettingsCard("技能") {
                SettingsOpenRow(title: "技能目录",
                                detail: "内置、记忆沉淀、你自己的目录和 Claude Code 插件里的技能，在主窗口的“扩展”页按来源列出。",
                                button: "打开技能目录", open: openSkills, identifier: "settings.skills.open")
            }
            SettingsCard("还没有提供的") {
                SettingsRow("MCP 服务器", detail: "管理 Agent 能连的 MCP 服务器。") { ComingSoonBadge() }
                SettingsRow("Hook", detail: "按用户配置、插件和项目列出钩子，标出有问题和待审核的。") { ComingSoonBadge() }
                SettingsRow("插件", detail: "启用、停用和升级插件。") { ComingSoonBadge() }
            }
        }
    }
}

/// A13: save the host's redacted diagnostics report where the user chooses.
struct DiagnosticsExportCard: View {
    let export: @MainActor () async throws -> DiagnosticsClient.Report
    @State private var busy = false
    @State private var message: String?

    var body: some View {
        SettingsCard("诊断") {
            SettingsRow("导出诊断报告",
                        detail: message ?? "版本、宿主能力、启动降级、任务状态计数、Agent 检测与宿主日志尾部；已脱敏，不含提示词、对话与凭据。") {
                Button(busy ? "正在生成…" : "导出…") { Task { await run() } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(busy)
                    .accessibilityIdentifier("settings.diagnostics.export")
            }
        }
    }

    private func run() async {
        busy = true
        defer { busy = false }
        do {
            let report = try await export()
            let panel = NSSavePanel()
            panel.nameFieldStringValue = report.fileName
            panel.allowedContentTypes = [.plainText]
            panel.canCreateDirectories = true
            guard panel.runModal() == .OK, let url = panel.url else { return }
            try report.text.write(to: url, atomically: true, encoding: .utf8)
            message = "已保存到 \(url.lastPathComponent)"
        } catch {
            message = "导出失败：\(error.localizedDescription)"
        }
    }
}

struct ImportsSettingsPane: View {
    let book: ImportBook
    @State private var revoking: String?
    @State private var picking = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            SettingsCard {
                SettingsRow("选择要导入的会话", detail: pickDetail) {
                    Button("选择会话…") { openPicker() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .disabled(book.busyAgents.contains("apply"))
                        .accessibilityIdentifier("settings.imports.pick")
                }
                if excludedCount > 0 {
                    SettingsRow("已排除 \(excludedCount) 个会话",
                                detail: "之前导入时没有勾选的会话不会显示在侧栏；在“选择会话…”里重新勾选即可找回。") {
                        Button("重新选择") { openPicker() }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .accessibilityIdentifier("settings.imports.reselect")
                    }
                }
            }
            SettingsCard("已有会话") {
                ForEach(ImportBook.importableAgents, id: \.self) { agent in
                    row(agent)
                }
            }
            SettingsCard {
                SettingsRow("立即重新扫描", detail: scanDetail) {
                    Button("扫描") { Task { await book.rescan() } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .disabled(book.busyAgents.contains("rescan"))
                        .accessibilityIdentifier("settings.imports.rescan")
                }
                SettingsRow("DSH / oh-my-pi", detail: "这两种 Agent 的会话导入将在后续版本支持。") { ComingSoonBadge() }
            }
            if let error = book.actionError {
                CapsuleErrorLine(error, identifier: "settings.imports.error") { book.dismissError() }
            } else if let error = book.readError ?? book.status?.lastError {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
            }
            Text("导入只读取会话文件，不复制、不修改；撤销会删除 Aeroric 里的索引，原文件不受影响。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .task {
            await book.refresh()
            for agent in ImportBook.importableAgents { await book.estimate(agent) }
        }
        .sheet(isPresented: $picking, onDismiss: { book.pick = nil }) { ImportPickerSheet(book: book) }
        .confirmationDialog("撤销导入授权？", isPresented: Binding(
            get: { revoking != nil }, set: { if !$0 { revoking = nil } }
        ), presenting: revoking) { agent in
            Button("撤销", role: .destructive) { Task { await book.revoke(agent) } }
            Button("取消", role: .cancel) {}
        } message: { agent in
            Text("\(ImportPresentation.agentTitle(agent)) 的已导入会话会从 Aeroric 中移除，原文件不受影响。")
        }
        .accessibilityIdentifier("settings.imports")
    }

    private var pickDetail: String {
        if let last = book.lastApply {
            let projects = last.createdProjects.isEmpty ? "" : "，新建 \(last.createdProjects.count) 个项目"
            return "上次导入了 \(last.imported) 个会话\(projects)。"
        }
        return "按项目列出 Claude Code 与 Codex 的全部会话（含 Codex 已归档的），勾选后导入；新目录会自动添加为项目。"
    }

    /// S8: how many sessions the user left out, so they can be picked again.
    private var excludedCount: Int {
        book.status?.agents.reduce(0) { $0 + $1.excluded } ?? 0
    }

    private func openPicker() {
        book.pick = nil
        picking = true
    }

    private var scanDetail: String {
        guard let ms = book.status?.lastScanAtMs else { return "尚未扫描" }
        let ago = RelativeTime.short(Date(timeIntervalSince1970: Double(ms) / 1000))
        return "上次扫描：" + (ago == "刚刚" ? ago : ago + "前")
    }

    private func row(_ agent: String) -> some View {
        let status = book.agentStatus(agent)
        let granted = status?.granted == true
        let detail = granted
            ? "\(ImportPresentation.abbreviated(status?.root ?? "")) · 已导入 \(status?.sessions ?? 0) 个会话"
            : "\(ImportPresentation.abbreviated(book.estimates[agent]?.root ?? status?.root ?? "")) · "
                + ImportPresentation.estimateText(book.estimates[agent])
        return SettingsRow(ImportPresentation.agentTitle(agent), detail: detail) {
            if granted {
                Button("撤销…") { revoking = agent }
                    .buttonStyle(AeroricCapsuleButtonStyle(.destructive, compact: true))
                    .accessibilityIdentifier("settings.imports.revoke.\(agent)")
            } else {
                Button("导入…") { openPicker() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .disabled(book.busyAgents.contains(agent) || book.estimates[agent]?.rootExists == false)
                    .accessibilityIdentifier("settings.imports.grant.\(agent)")
            }
        }
    }
}
