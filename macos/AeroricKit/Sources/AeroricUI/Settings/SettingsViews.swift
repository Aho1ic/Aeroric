import AeroricClient
import AppKit
import SwiftUI
import UniformTypeIdentifiers

// Settings building blocks (M3). Only settings that something actually reads
// live here; unwired ones were removed rather than shown as working (D4).

/// Settings window navigation.
public enum SettingsPane: String, CaseIterable, Identifiable, Sendable {
    case general, appearance, agents, imports, connection, legacyCleanup, about

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .general: "通用"
        case .appearance: "外观"
        case .agents: "Agents"
        case .imports: "导入"
        case .connection: "连接"
        case .legacyCleanup: "旧版 Agent 清理"
        case .about: "关于"
        }
    }

    public var systemImage: String {
        switch self {
        case .general: "gearshape"
        case .appearance: "paintpalette"
        case .agents: "cpu"
        case .imports: "tray.and.arrow.down"
        case .connection: "bolt.horizontal"
        case .legacyCleanup: "trash"
        case .about: "info.circle"
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

/// Rounded card grouping settings rows, separated by hairlines.
public struct SettingsCard<Content: View>: View {
    let title: String?
    let content: Content

    public init(_ title: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let title {
                Text(title)
                    .font(AeroricType.sectionLabel())
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.leading, AeroricSpace.xs)
            }
            // Each row draws a top hairline; the first one sits on the card
            // edge and is hidden by the clip, so rows read as separated.
            VStack(spacing: 0) { content }
                .padding(.top, -1)
                .background(AeroricColor.layer1)
                .clipShape(AeroricRadius.shape(AeroricRadius.m))
                .overlay { AeroricRadius.shape(AeroricRadius.m).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
        }
    }
}

/// Title and grey explanation on the left, the control on the right.
public struct SettingsRow<Control: View>: View {
    let title: String
    let detail: String?
    let control: Control

    public init(_ title: String, detail: String? = nil, @ViewBuilder control: () -> Control) {
        self.title = title
        self.detail = detail
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
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.m + 2)
        .frame(minHeight: 44)
        .overlay(alignment: .top) { HairlineDivider().padding(.leading, AeroricSpace.l) }
    }
}

// MARK: - Panes

struct GeneralSettingsPane: View {
    let controller: InteractionModeController?
    let fallbackMode: Binding<InteractionMode>?
    let onFallbackSave: ((InteractionMode) -> Void)?
    let fallbackError: String?

    private var mode: InteractionMode { controller?.mode ?? fallbackMode?.wrappedValue ?? .native }
    private var saveError: String? { controller?.lastSaveError ?? fallbackError }

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

    /// N6 D5 (Q6): Claude and Codex always run as native sessions now; the
    /// old "advanced terminal mode" switch is retired to a read-only note.
    private var interaction: some View {
        SettingsCard("交互") {
            SettingsRow("会话呈现",
                        detail: "Claude 与 Codex 始终使用原生会话（对话、审批卡、工具卡），不再提供终端模式开关。远程 SSH 工作区、未迁移的自定义 Agent 与 omp 仍在终端中运行。") {
                Text("原生")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("settings.general.interaction-mode")
            }
        }
    }

    private func save(_ newMode: InteractionMode) {
        if let controller {
            controller.setMode(newMode)
        } else {
            fallbackMode?.wrappedValue = newMode
            onFallbackSave?(newMode)
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
            SettingsCard("字号") {
                sizeRow(.text, title: "正文", detail: "界面与对话文字")
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
        .onChange(of: themeRaw) { _, _ in AppTheme.applyStored() }
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
                    CapsuleSlider(selection: $defaultHarness, groupTitle: "默认 Harness", identifier: "settings.agents.default-harness",
                                  segments: NewSessionLayout.harnessOrder.map { .init(value: $0, title: AgentConfigText.harnessTitle($0)) })
                        .fixedSize()
                }
            }
            SettingsCard {
                SettingsRow("管理 Agent 配置", detail: "第三方 API 连接、号池、模型路由与 Agent 配置，在主窗口侧栏的“Agent 配置”页。") {
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
            if let exporter = state.settingsDiagnosticsExporter {
                DiagnosticsExportCard(export: exporter)
            }
        }
        .accessibilityIdentifier("settings.about")
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

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
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
            if let error = book.lastError ?? book.status?.lastError {
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
                Button("导入") { Task { await book.grant(agent) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .disabled(book.busyAgents.contains(agent) || book.estimates[agent]?.rootExists == false)
                    .accessibilityIdentifier("settings.imports.grant.\(agent)")
            }
        }
    }
}
