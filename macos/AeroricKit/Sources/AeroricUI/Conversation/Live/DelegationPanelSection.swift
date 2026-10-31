import SwiftUI
import AeroricProtocol

/// B4-2: what the progress page knows about a task's delegations.
public struct DelegationPanelModel: Equatable, Sendable {
    public var delegations: [LiveDelegationCard]
    /// This task is a child of that one (it cannot delegate).
    public var delegateOf: String?
    public var roles: [LiveDelegationRole]
    public var stats: [LiveReputationRow]
    public var audit: [LiveDelegationAudit]

    public init(delegations: [LiveDelegationCard] = [], delegateOf: String? = nil, roles: [LiveDelegationRole] = [],
                stats: [LiveReputationRow] = [], audit: [LiveDelegationAudit] = []) {
        self.delegations = delegations
        self.delegateOf = delegateOf
        self.roles = roles
        self.stats = stats
        self.audit = audit
    }

    /// Track records that match what the form has picked (shown, never used to pick).
    public func reputation(role: String?, agent: String?) -> [LiveReputationRow] {
        stats.filter { row in
            (role == nil || row.role == role) && (agent == nil || row.agent == agent)
        }
        .sorted { $0.total > $1.total }
    }
}

public enum DelegationPanelAction: Equatable, Sendable {
    case start(LiveDelegationContract)
    case open(String)
    case saveRole(LiveDelegationRole)
    case deleteRole(String)
}

/// The new-delegation form's choices; `nil` keeps the role's default.
struct DelegationDraft: Equatable {
    var instruction = ""
    var role: String?
    var agent: LiveDelegateAgent?
    var contextMode: String?
    var workspace: String?
    var acceptance = ""
    var check = ""
    var files = ""
    var minutes = ""
    /// CU F3: the token budget (unlimited when empty).
    var tokens = ""

    var contract: LiveDelegationContract {
        let acceptance = acceptance.trimmingCharacters(in: .whitespaces)
        let check = check.trimmingCharacters(in: .whitespaces)
        return LiveDelegationContract(
            instruction: instruction.trimmingCharacters(in: .whitespacesAndNewlines), role: role, agent: agent,
            contextMode: contextMode, contextRefs: DelegationDraft.split(files), workspace: workspace,
            acceptance: acceptance.isEmpty ? [] : [LiveAcceptanceItem(text: acceptance, check: check.isEmpty ? nil : check)],
            budgetMinutes: Int(minutes.trimmingCharacters(in: .whitespaces)),
            budgetTokens: DelegationDraft.tokens(tokens))
    }

    var ready: Bool { !instruction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    /// "200000", "200k", "1.5m" → tokens; nil when empty or unreadable.
    static func tokens(_ text: String) -> UInt64? {
        let t = text.trimmingCharacters(in: .whitespaces).lowercased().replacingOccurrences(of: ",", with: "")
        guard !t.isEmpty else { return nil }
        let (number, scale): (Substring, Double) = t.hasSuffix("k") ? (t.dropLast(), 1_000)
            : t.hasSuffix("m") ? (t.dropLast(), 1_000_000) : (Substring(t), 1)
        guard let value = Double(number), value > 0 else { return nil }
        return UInt64(value * scale)
    }

    /// "a.rs, b.rs  c.rs" → ["a.rs", "b.rs", "c.rs"].
    static func split(_ text: String) -> [String] {
        text.split(whereSeparator: { $0 == "," || $0 == "，" || $0 == " " || $0.isNewline })
            .map(String.init)
    }
}

struct DelegationPanelSection: View {
    let model: DelegationPanelModel
    let onAction: (DelegationPanelAction) -> Void
    @State private var composing = false
    @State private var draft = DelegationDraft()
    @State private var showAudit = false
    @State private var showRoles = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            CheckpointSectionLabel(title: "委派")
            if let parent = model.delegateOf {
                Button { onAction(.open(parent)) } label: {
                    CheckpointRow(symbol: "person.2", text: "这是一个委派出来的子会话，不能再委派") {
                        Text("打开主会话")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.accent)
                            .fixedSize()
                    }
                }
                .buttonStyle(.plain)
            } else {
                ForEach(model.delegations.reversed(), id: \.delegationId) { card in
                    Button {
                        if let child = card.childTaskId { onAction(.open(child)) }
                    } label: {
                        CheckpointRow(symbol: "person.2", text: "\(DelegationText.summary(card)) · \(card.contract.instruction)",
                                      highlight: card.needsUser ? AeroricColor.warning : nil) {
                            if card.childTaskId != nil {
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
                if composing {
                    form
                } else {
                    HStack(spacing: AeroricSpace.m) {
                        Button("新委派") { composing = true }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .frame(maxWidth: .infinity)
                            .help("把一块能独立完成、能验收的工作交给一个子会话；也可以在输入框里用 #角色 开头")
                            .accessibilityIdentifier("progress.delegation.new")
                        Button("角色库") { showRoles = true }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .frame(maxWidth: .infinity)
                            .accessibilityIdentifier("progress.delegation.roles")
                    }
                }
                if !model.audit.isEmpty {
                    Button { showAudit.toggle() } label: {
                        CheckpointRow(symbol: "list.bullet.rectangle", text: "审计 · \(model.audit.count) 条") {
                            Image(systemName: showAudit ? "chevron.up" : "chevron.down")
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("progress.delegation.audit")
                    if showAudit {
                        ForEach(Array(model.audit.suffix(20).reversed().enumerated()), id: \.offset) { _, entry in
                            Text(DelegationText.audit(entry))
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textSecondary)
                                .lineLimit(1)
                                .truncationMode(.tail)
                                .help(DelegationText.audit(entry))
                                .padding(.horizontal, AeroricSpace.l)
                        }
                    }
                }
            }
        }
        .sheet(isPresented: $showRoles) {
            RoleLibrarySheet(roles: model.roles, onSave: { onAction(.saveRole($0)) },
                             onDelete: { onAction(.deleteRole($0)) }, onClose: { showRoles = false })
        }
    }

    private var roleTitle: String {
        guard let id = draft.role else { return "不用角色" }
        return model.roles.first { $0.id == id }?.name ?? id
    }

    private var agentTitle: String {
        switch draft.agent {
        case nil: return draft.role == nil ? "同族" : "按角色"
        case .same?: return "同族"
        case .other?: return "另一族"
        case let .named(agent, _)?: return DelegationText.agentLabel(agent)
        }
    }

    private var pickedAgent: String? {
        if case let .named(agent, _)? = draft.agent { return agent }
        return nil
    }

    @ViewBuilder private var form: some View {
        CapsuleField(placeholder: "要子会话做什么", text: $draft.instruction)
            .accessibilityIdentifier("progress.delegation.instruction")
        HStack(spacing: 0) {
            CapsulePicker(title: roleTitle, systemImage: "person.crop.square") {
                Button("不用角色") { draft.role = nil }
                Divider()
                ForEach(model.roles) { role in
                    Button(role.name) { draft.role = role.id }
                }
            }
            CapsulePicker(title: agentTitle, systemImage: "cpu") {
                Button("按角色默认") { draft.agent = nil }
                Button("与本会话同族") { draft.agent = .same }
                Button("另一族") { draft.agent = .other }
                Divider()
                Button("Claude Code") { draft.agent = .named(agent: "claude") }
                Button("Codex") { draft.agent = .named(agent: "codex") }
            }
            Spacer(minLength: 0)
        }
        HStack(spacing: 0) {
            CapsulePicker(title: draft.contextMode.map(DelegationText.mode) ?? "上下文按角色",
                          systemImage: "doc.on.doc") {
                Button("按角色默认") { draft.contextMode = nil }
                Button("交接清单") { draft.contextMode = "brief" }
                Button("隔离（只给指令和文件）") { draft.contextMode = "isolated" }
                Button("续做（带上本会话的对话）") { draft.contextMode = "fork" }
            }
            CapsulePicker(title: draft.workspace.map { $0 == "shared" ? "共享可写" : "只读" } ?? "工作区按角色",
                          systemImage: "folder") {
                Button("按角色默认") { draft.workspace = nil }
                Button("只读") { draft.workspace = "readOnly" }
                Button("共享可写（同一时刻只能一个）") { draft.workspace = "shared" }
            }
            Spacer(minLength: 0)
        }
        CapsuleField(placeholder: "验收条款（可选）", text: $draft.acceptance, height: 26)
        CapsuleField(placeholder: "核对命令（可选，如 cargo test）", text: $draft.check, height: 26)
        CapsuleField(placeholder: "先读的文件（可选，用逗号分隔）", text: $draft.files, height: 26)
        HStack(spacing: AeroricSpace.m) {
            CapsuleField(placeholder: "时长上限（分钟，默认 30）", text: $draft.minutes, height: 26)
            CapsuleField(placeholder: "token 上限（如 200k，默认不限）", text: $draft.tokens, height: 26)
                .accessibilityIdentifier("progress.delegation.tokens")
        }
        ForEach(Array(model.reputation(role: draft.role, agent: pickedAgent).prefix(2).enumerated()), id: \.offset) { _, row in
            Text(DelegationText.reputation(row, roles: model.roles))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
        if draft.acceptance.trimmingCharacters(in: .whitespaces).isEmpty
            && (model.roles.first { $0.id == draft.role }?.acceptance.isEmpty ?? true) {
            Text("没有验收条款：结果无法核对，建议继续拆分")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.warning)
                .lineLimit(1)
        }
        HStack(spacing: AeroricSpace.m) {
            Button("取消") {
                composing = false
                draft = DelegationDraft()
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .frame(maxWidth: .infinity)
            Button("委派") {
                onAction(.start(draft.contract))
                composing = false
                draft = DelegationDraft()
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
            .frame(maxWidth: .infinity)
            .disabled(!draft.ready)
            .help("子会话会用你的账号调用模型；权限不会高于本会话")
            .accessibilityIdentifier("progress.delegation.start")
        }
    }
}

/// B4-2: the role library — built-in roles (changeable, restorable) and the
/// user's own.
struct RoleLibrarySheet: View {
    let roles: [LiveDelegationRole]
    let onSave: (LiveDelegationRole) -> Void
    let onDelete: (String) -> Void
    let onClose: () -> Void
    @State private var editing: LiveDelegationRole?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text("角色库").font(.system(size: AeroricType.title, weight: .semibold))
                Spacer()
                Button("完成", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .keyboardShortcut(.cancelAction)
            }
            if let role = editing {
                RoleEditor(role: role, onSave: {
                    onSave($0)
                    editing = nil
                }, onCancel: { editing = nil })
            } else {
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: AeroricSpace.s) {
                        ForEach(roles) { role in
                            HStack(spacing: AeroricSpace.m) {
                                Image(systemName: role.builtin ? "person.crop.square" : "person.crop.square.badge.plus")
                                    .foregroundStyle(AeroricColor.textSecondary)
                                    .frame(width: 16)
                                Text(role.name)
                                    .font(.system(size: AeroricType.secondary, weight: .medium))
                                    .lineLimit(1)
                                    .fixedSize()
                                Text(RoleText.line(role))
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textSecondary)
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .help(role.description)
                                Button("改") { editing = role }
                                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                                    .fixedSize()
                                if !role.builtin || role.customized {
                                    Button(role.builtin ? "恢复默认" : "删除") { onDelete(role.id) }
                                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                                        .fixedSize()
                                }
                            }
                            .padding(.horizontal, AeroricSpace.l)
                            .frame(height: 32)
                            .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
                        }
                    }
                }
                Button("新增角色") { editing = LiveDelegationRole(name: "") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier("roles.add")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520, height: 460)
    }
}

private struct RoleEditor: View {
    @State var role: LiveDelegationRole
    let onSave: (LiveDelegationRole) -> Void
    let onCancel: () -> Void
    @State private var acceptance = ""
    @State private var minutes = ""

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            CapsuleField(placeholder: "角色名", text: $role.name)
                .disabled(role.builtin)
            CapsuleField(placeholder: "一句话说明", text: $role.description)
            CapsuleField(placeholder: "给子会话的固定要求", text: $role.instruction)
            HStack(spacing: 0) {
                CapsulePicker(title: agentTitle, systemImage: "cpu") {
                    Button("与主会话同族") { role.agent = .same }
                    Button("另一族") { role.agent = .other }
                    Button("Claude Code") { role.agent = .named(agent: "claude") }
                    Button("Codex") { role.agent = .named(agent: "codex") }
                }
                CapsulePicker(title: DelegationText.mode(role.contextMode), systemImage: "doc.on.doc") {
                    Button("交接清单") { role.contextMode = "brief" }
                    Button("隔离") { role.contextMode = "isolated" }
                    Button("续做") { role.contextMode = "fork" }
                }
                CapsulePicker(title: DelegationText.workspace(role.workspace, permission: role.permission),
                              systemImage: "folder") {
                    Button("只读") {
                        role.workspace = "readOnly"
                        role.permission = "plan"
                    }
                    ForEach(["ask", "auto_edit", "full_access"], id: \.self) { mode in
                        Button("可写 · \(DelegationText.permission(mode))") {
                            role.workspace = "shared"
                            role.permission = mode
                        }
                    }
                }
                Spacer(minLength: 0)
            }
            CapsuleField(placeholder: "默认验收条款（可选）", text: $acceptance)
            CapsuleField(placeholder: "默认时长上限（分钟，可选）", text: $minutes)
            Text("子会话的权限永远不会高于发起委派的会话")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
            HStack(spacing: AeroricSpace.m) {
                Button("取消", action: onCancel)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .frame(maxWidth: .infinity)
                Button("保存") {
                    var saved = role
                    let text = acceptance.trimmingCharacters(in: .whitespaces)
                    saved.acceptance = text.isEmpty ? [] : [LiveAcceptanceItem(text: text)]
                    saved.budgetMinutes = Int(minutes.trimmingCharacters(in: .whitespaces))
                    onSave(saved)
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .frame(maxWidth: .infinity)
                .disabled(role.name.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("roles.save")
            }
        }
        .onAppear {
            acceptance = role.acceptance.first?.text ?? ""
            minutes = role.budgetMinutes.map(String.init) ?? ""
        }
    }

    private var agentTitle: String {
        switch role.agent {
        case .same: return "同族"
        case .other: return "另一族"
        case let .named(agent, _): return DelegationText.agentLabel(agent)
        }
    }
}

public enum RoleText {
    /// "另一族 · 隔离 · 只读 · 只审查，不要修改…".
    public static func line(_ role: LiveDelegationRole) -> String {
        let agent: String
        switch role.agent {
        case .same: agent = "同族"
        case .other: agent = "另一族"
        case let .named(name, _): agent = DelegationText.agentLabel(name)
        }
        var parts = [agent, DelegationText.mode(role.contextMode),
                     DelegationText.workspace(role.workspace, permission: role.permission)]
        if role.customized { parts.append("已改") }
        if !role.description.isEmpty { parts.append(role.description) }
        return parts.joined(separator: " · ")
    }
}
