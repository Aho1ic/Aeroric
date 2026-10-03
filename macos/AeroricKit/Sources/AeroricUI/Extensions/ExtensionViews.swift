import Foundation
import SwiftUI

/// P8-01：扩展面簿记（六视图的数据源；凭据/env 值以掩码形态进入 store——
/// 宿主 `mcp::mask_env` 的产出原样落地，store 不持明文）。
@MainActor
@Observable
public final class ExtensionStore {
    public struct McpServer: Identifiable, Equatable, Sendable {
        public var name: String
        public var command: String
        public var args: [String]
        /// 掩码后的 env（凭据值＝••••；本类型不保存明文）。
        public var maskedEnv: [String: String]
        public var enabled: Bool

        public var id: String { name }

        public init(name: String, command: String, args: [String], maskedEnv: [String: String], enabled: Bool) {
            self.name = name
            self.command = command
            self.args = args
            self.maskedEnv = maskedEnv
            self.enabled = enabled
        }
    }

    public struct HookEntry: Identifiable, Equatable, Sendable {
        public var script: String
        public var event: String
        public var active: Bool

        public var id: String { "\(script)#\(event)" }

        public init(script: String, event: String, active: Bool) {
            self.script = script
            self.event = event
            self.active = active
        }
    }

    public struct SkillEntry: Identifiable, Equatable, Sendable {
        public var name: String
        public var agent: String
        public var health: String

        public var id: String { "\(agent)#\(name)" }

        public init(name: String, agent: String, health: String) {
            self.name = name
            self.agent = agent
            self.health = health
        }
    }

    public struct PendingInstall: Identifiable, Equatable, Sendable {
        public var name: String
        public var origin: String
        public var revision: String?
        public var writeScope: [String]
        public var permissions: [String]

        public var id: String { "\(origin)#\(name)" }

        public init(name: String, origin: String, revision: String?, writeScope: [String], permissions: [String]) {
            self.name = name
            self.origin = origin
            self.revision = revision
            self.writeScope = writeScope
            self.permissions = permissions
        }
    }

    public private(set) var mcpServers: [McpServer] = []
    public private(set) var hooks: [HookEntry] = []
    public private(set) var skills: [SkillEntry] = []
    /// 待确认安装（来源/revision/写入范围/所需权限——UI 呈现为确认表单）。
    public private(set) var pendingInstall: PendingInstall?
    /// 待生效标记：MCP/hook 配置保存成功但运行中 Agent 需下次启动才生效。
    public private(set) var pendingRestartNotice: Bool = false

    public init() {}

    public func ingestMcp(_ servers: [McpServer]) {
        mcpServers = servers
    }

    public func ingestHooks(_ entries: [HookEntry]) {
        hooks = entries
    }

    public func ingestSkills(_ entries: [SkillEntry]) {
        skills = entries
    }

    public func proposeInstall(_ candidate: PendingInstall) {
        pendingInstall = candidate
    }

    public func clearInstallProposal() {
        pendingInstall = nil
    }

    public func markNeedsRestart() {
        pendingRestartNotice = true
    }

    /// 保存确认后清除「需重启」提示（由调用方在真正生效路径上调用）。
    public func clearRestartNotice() {
        pendingRestartNotice = false
    }

    // MARK: - 纯逻辑（与 core extensions 同语义）

    /// 信任判定（与 core `trust::verdict` 同规则，Swift 侧呈现用）。
    public static func trustVerdict(
        kind: String, revision: String?, writeScope: [String], userConfirmed: Bool
    ) -> String {
        if writeScope.isEmpty
            || writeScope.contains(where: {
                $0.isEmpty || $0.hasPrefix("/")
                    || $0.split(separator: "/").contains("..")
                    || !($0.hasPrefix("skills/") || $0 == "skills")
            })
        {
            return "rejected"
        }
        let builtin = kind == "builtin" || kind == "official-marketplace"
        if builtin && revision != nil { return "trusted" }
        return userConfirmed ? "trusted" : "requiresConfirmation"
    }

    /// hook 脚本扩展名守卫（omp loader 静默丢弃非 .js/.ts 的教训）。
    public static func isHookScript(_ name: String) -> Bool {
        let lower = name.lowercased()
        return lower.hasSuffix(".js") || lower.hasSuffix(".ts")
    }
}

// MARK: - 1. SkillHubView

public struct SkillHubView: View {
    @Bindable var store: ExtensionStore
    public var onInstall: ((_ name: String) -> Void)?
    public var onDisable: ((_ entry: ExtensionStore.SkillEntry) -> Void)?

    public init(
        store: ExtensionStore,
        onInstall: ((_ name: String) -> Void)? = nil,
        onDisable: ((_ entry: ExtensionStore.SkillEntry) -> Void)? = nil
    ) {
        self.store = store
        self.onInstall = onInstall
        self.onDisable = onDisable
    }

    public var body: some View {
        List {
            ForEach(Array(store.skills), id: \.id) { skill in
                HStack {
                    VStack(alignment: .leading) {
                        Text(skill.name).font(.callout)
                        Text("\(skill.agent) · \(skill.health)").font(.caption)
                            .foregroundStyle(skill.health == "ok" ? Color.secondary : Color.orange)
                    }
                    Spacer()
                    Button("停用") { onDisable?(skill) }
                        .buttonStyle(.borderless)
                }
            }
        }
        .listStyle(.plain)
        .toolbar {
            Button("安装 Skill…") { onInstall?("") }
                .accessibilityLabel("安装 Skill")
        }
        .accessibilityIdentifier("extensions.skills")
    }
}

// MARK: - 2. SkillDetail

public struct SkillDetail: View {
    public var entry: ExtensionStore.SkillEntry
    public var bodyText: String

    public init(entry: ExtensionStore.SkillEntry, bodyText: String) {
        self.entry = entry
        self.bodyText = bodyText
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Text(entry.name).font(.title3)
                Text("agent: \(entry.agent) · 健康: \(entry.health)")
                    .font(.caption).foregroundStyle(.secondary)
                Text(bodyText).font(.callout).textSelection(.enabled)
            }
            .padding()
        }
        .accessibilityIdentifier("extensions.skillDetail")
    }
}

// MARK: - 3. MCPSettingsView

/// MCP 设置：env 以掩码呈现（宿主 `mask_env`）；保存走「校验→原子替换→失败
/// 回滚」，成功后标记「运行中 Agent 需下次启动生效」。
public struct MCPSettingsView: View {
    @Bindable var store: ExtensionStore
    public var onSave: ((_ servers: [ExtensionStore.McpServer]) -> Void)?

    public init(
        store: ExtensionStore,
        onSave: ((_ servers: [ExtensionStore.McpServer]) -> Void)? = nil
    ) {
        self.store = store
        self.onSave = onSave
    }

    public var body: some View {
        List {
            ForEach(store.mcpServers) { server in
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(server.name).font(.callout)
                        Spacer()
                        Toggle("启用", isOn: Binding(
                            get: { server.enabled },
                            set: { _ in store.markNeedsRestart() }
                        ))
                        .toggleStyle(.switch).fixedSize()
                    }
                    Text("\(server.command) \(server.args.joined(separator: " "))")
                        .font(.caption).foregroundStyle(.secondary)
                    ForEach(server.maskedEnv.keys.sorted(), id: \.self) { key in
                        Text("\(key)=\(server.maskedEnv[key] ?? "")")
                            .font(.caption2).foregroundStyle(.tertiary)
                    }
                }
            }
        }
        .listStyle(.plain)
        .overlay(alignment: .bottom) {
            if store.pendingRestartNotice {
                Label("已保存。运行中的 Agent 将在**下次启动**时生效。", systemImage: "clock.arrow.circlepath")
                    .font(.caption)
                    .padding(8)
                    .background(.orange.opacity(0.15), in: RoundedRectangle(cornerRadius: 8))
                    .padding(.bottom, 8)
            }
        }
        .toolbar {
            Button("保存") { onSave?(store.mcpServers) }
        }
        .accessibilityIdentifier("extensions.mcp")
    }
}

// MARK: - 4. HookStatusView

public struct HookStatusView: View {
    @Bindable var store: ExtensionStore
    public var onToggle: ((_ entry: ExtensionStore.HookEntry, _ enabled: Bool) -> Void)?

    public init(
        store: ExtensionStore,
        onToggle: ((_ entry: ExtensionStore.HookEntry, _ enabled: Bool) -> Void)? = nil
    ) {
        self.store = store
        self.onToggle = onToggle
    }

    public var body: some View {
        List(store.hooks) { hook in
            HStack {
                VStack(alignment: .leading) {
                    Text(hook.script).font(.callout)
                    Text(hook.event).font(.caption).foregroundStyle(.secondary)
                    if !ExtensionStore.isHookScript(hook.script) {
                        Text("⚠️ 非 .js/.ts——loader 会静默丢弃").font(.caption2).foregroundStyle(.red)
                    }
                }
                Spacer()
                Toggle("", isOn: Binding(
                    get: { hook.active },
                    set: { onToggle?(hook, $0) }
                ))
                .toggleStyle(.switch).labelsHidden()
            }
        }
        .listStyle(.plain)
        .accessibilityIdentifier("extensions.hooks")
    }
}

// MARK: - 5. PluginManagerView

public struct PluginManagerView: View {
    public var plugins: [String]
    public var onEnable: ((_ name: String) -> Void)?
    public var onRollback: ((_ name: String) -> Void)?
    @State private var selected: String?

    public init(
        plugins: [String],
        onEnable: ((_ name: String) -> Void)? = nil,
        onRollback: ((_ name: String) -> Void)? = nil
    ) {
        self.plugins = plugins
        self.onEnable = onEnable
        self.onRollback = onRollback
    }

    public var body: some View {
        List(plugins, id: \.self) { name in
            HStack {
                Text(name).font(.callout)
                Spacer()
                Button("启用") { onEnable?(name) }
                    .buttonStyle(.borderless)
                Button("回滚") { onRollback?(name) }
                    .buttonStyle(.borderless)
            }
            .contentShape(Rectangle())
            .onTapGesture { selected = name }
        }
        .listStyle(.plain)
        .accessibilityIdentifier("extensions.plugins")
    }
}

// MARK: - 6. ExtensionTrustSheet

/// 安装确认表单：来源/revision/写入范围/所需权限全列——确认是显式动作，
/// 写入范围越界的提议根本到不了这个表单（core trust 判定拒绝）。
public struct ExtensionTrustSheet: View {
    public var candidate: ExtensionStore.PendingInstall
    public var onConfirm: () -> Void
    public var onCancel: () -> Void

    public init(
        candidate: ExtensionStore.PendingInstall,
        onConfirm: @escaping () -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.candidate = candidate
        self.onConfirm = onConfirm
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("安装「\(candidate.name)」", systemImage: "shippingbox")
                .font(.headline)
            LabeledContent("来源", value: candidate.origin)
            if let revision = candidate.revision {
                LabeledContent("Revision", value: revision)
            } else {
                LabeledContent("Revision", value: "未知")
            }
            LabeledContent("写入范围", value: candidate.writeScope.joined(separator: ", "))
            LabeledContent("所需权限", value: candidate.permissions.isEmpty ? "无" : candidate.permissions.joined(separator: ", "))
            Text("扫描文件不等于执行；第三方脚本在运行前还会再请求确认。")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("取消", action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("确认安装", action: onConfirm)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 420)
    }
}
