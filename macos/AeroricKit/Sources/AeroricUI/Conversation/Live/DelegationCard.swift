import SwiftUI
import AeroricProtocol

/// B4-2: one delegation in the timeline — who it went to under which
/// contract, its state, the result once it is back, each criterion's check,
/// and what the user can do next. Rows are capsules; nothing wraps.
struct DelegationCard: View {
    let card: LiveDelegationCard
    let onOpen: (String) -> Void
    let onSend: () -> Void
    let onStop: () -> Void
    let onRedo: (String) -> Void
    var onAck: () -> Void = {}
    @State private var expanded = false
    @State private var redoing = false
    @State private var note = ""

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "person.2").foregroundStyle(AeroricColor.textSecondary)
                Text(DelegationText.title(card))
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(1)
                    .fixedSize()
                Text(DelegationText.contract(card))
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: .infinity, alignment: .leading)
                let (label, tint) = DelegationText.state(card)
                CheckpointBadge(text: label, tint: tint)
            }
            Text(card.contract.instruction)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.tail)
                .help(card.contract.instruction)
            if let result = card.result {
                Button { expanded.toggle() } label: {
                    HStack(spacing: AeroricSpace.m) {
                        Text("结论")
                            .font(.system(size: AeroricType.caption, weight: .medium))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .fixedSize()
                        Text(DelegationText.firstLine(result))
                            .font(.system(size: AeroricType.secondary))
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Image(systemName: expanded ? "chevron.up" : "chevron.down")
                            .font(.system(size: 10))
                            .foregroundStyle(AeroricColor.textSecondary)
                    }
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(height: 30)
                    .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
                    .contentShape(Capsule(style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("delegation.result")
                if expanded {
                    Text(result)
                        .font(.system(size: AeroricType.caption))
                        .textSelection(.enabled)
                        .padding(.horizontal, AeroricSpace.l)
                }
            }
            ForEach(card.acceptance, id: \.id) { item in
                HStack(spacing: AeroricSpace.m) {
                    let (word, tint) = DelegationText.acceptance(item.state)
                    CheckpointBadge(text: word, tint: tint)
                    Text(item.text)
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
            }
            if let probes = BoardText.probes(card.probes) {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(AeroricColor.warning)
                    Text(probes)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .help(probes)
                    Button("知道了", action: onAck)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("delegation.probes.ack")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.12)))
                .accessibilityIdentifier("delegation.probes")
            }
            if let note = DelegationText.footnote(card) {
                Text(note)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .help(note)
            }
            if redoing {
                HStack(spacing: AeroricSpace.m) {
                    CapsuleField(placeholder: "要它补什么（可留空）", text: $note)
                        .accessibilityIdentifier("delegation.redo.note")
                    Button("退回") {
                        onRedo(note)
                        note = ""
                        redoing = false
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .accessibilityIdentifier("delegation.redo.confirm")
                    Button("取消") { redoing = false }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
            }
            HStack(spacing: AeroricSpace.m) {
                if card.canSend && card.origin == "user" {
                    Button("发给主会话", action: onSend)
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("delegation.send")
                }
                if !card.isOpen && card.childTaskId != nil && !redoing {
                    Button("退回重做") { redoing = true }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("delegation.redo")
                }
                if card.isOpen {
                    Button("停止", action: onStop)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("delegation.stop")
                }
                Spacer(minLength: 0)
                if let child = card.childTaskId, card.state != "starting" {
                    Button("打开子会话") { onOpen(child) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("delegation.open")
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(AeroricColor.separator2, lineWidth: 0.5))
        .accessibilityIdentifier("delegation.card")
    }
}

public enum DelegationText {
    /// "委派 · 调研员" or "委派".
    public static func title(_ card: LiveDelegationCard) -> String {
        card.roleName.map { "委派 · \($0)" } ?? "委派"
    }

    /// "Codex · 隔离 · 只读".
    public static func contract(_ card: LiveDelegationCard) -> String {
        var parts: [String] = []
        parts.append(card.harnessLabel.isEmpty ? "正在启动子会话" : card.harnessLabel)
        parts.append(mode(card.contract.contextMode ?? "brief"))
        parts.append(workspace(card.contract.workspace ?? "readOnly", permission: card.contract.permission ?? "plan"))
        if card.origin == "session" { parts.append("会话发起") }
        return parts.joined(separator: " · ")
    }

    public static func mode(_ mode: String) -> String {
        switch mode {
        case "fork": return "续做"
        case "isolated": return "隔离"
        default: return "交接清单"
        }
    }

    public static func workspace(_ workspace: String, permission: String) -> String {
        if workspace == "readOnly" || permission == "plan" { return "只读" }
        return "可写 · \(DelegationText.permission(permission))"
    }

    public static func state(_ card: LiveDelegationCard) -> (String, Color) {
        switch card.state {
        case "starting": return ("启动中", AeroricColor.accent)
        case "running": return ("进行中", AeroricColor.accent)
        case "checking": return ("核对验收中", AeroricColor.accent)
        case "reported":
            if card.sent { return ("已回报 · 已发给主会话", AeroricColor.success) }
            return ("已回报", AeroricColor.success)
        case "failed": return ("失败", AeroricColor.danger)
        case "stopped": return ("已停止", AeroricColor.textSecondary)
        case "overBudget": return ("超出预算", AeroricColor.danger)
        case "interrupted": return ("已中断", AeroricColor.warning)
        default: return (card.state, AeroricColor.textSecondary)
        }
    }

    public static func acceptance(_ state: String) -> (String, Color) {
        switch state {
        case "passed": return ("通过", AeroricColor.success)
        case "failed": return ("未通过", AeroricColor.danger)
        default: return ("未核对", AeroricColor.textSecondary)
        }
    }

    /// One line under the card: changes, evidence, rework, why it stopped.
    public static func footnote(_ card: LiveDelegationCard) -> String? {
        var parts: [String] = []
        if let error = card.error { parts.append(error) }
        if card.files > 0 { parts.append("改动 \(card.files) 个文件 +\(card.additions) −\(card.deletions)") }
        if card.state == "reported" && !card.evidence { parts.append("没有可核对的证据") }
        if card.contract.acceptance.isEmpty && card.state == "reported" {
            parts.append("没有验收条款，建议继续拆分")
        }
        if card.reworks > 0 { parts.append("退回 \(card.reworks) 次") }
        if let tokens = card.tokens { parts.append("\(tokens) token") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    static func firstLine(_ text: String) -> String {
        text.split(whereSeparator: \.isNewline).first.map(String.init) ?? text
    }

    /// The progress page's one-line summary of a delegation.
    public static func summary(_ card: LiveDelegationCard) -> String {
        var parts = [title(card), state(card).0]
        if !card.acceptance.isEmpty { parts.append("验收 \(card.passedCount)/\(card.acceptance.count)") }
        return parts.joined(separator: " · ")
    }

    /// "#调研员 查一下 X" → ("调研员", "查一下 X"); nil for anything else.
    public static func hashCommand(_ text: String) -> (target: String, instruction: String)? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("#"), trimmed.count > 1, !trimmed.hasPrefix("##") else { return nil }
        let body = trimmed.dropFirst()
        guard let space = body.firstIndex(where: { $0 == " " || $0.isNewline }) else { return nil }
        let target = String(body[..<space])
        let instruction = body[space...].trimmingCharacters(in: .whitespacesAndNewlines)
        guard !target.isEmpty, !instruction.isEmpty else { return nil }
        return (target, instruction)
    }

    /// The harness a `#` target names, if it names one.
    public static func agent(for target: String) -> LiveDelegateAgent? {
        switch target.lowercased() {
        case "claude", "claude-code", "claudecode": return .named(agent: "claude")
        case "codex": return .named(agent: "codex")
        default: return nil
        }
    }

    /// The `#` suggestions while the composer holds "#…" with no space yet.
    public static func suggestions(_ text: String, roles: [LiveDelegationRole]) -> [String] {
        guard text.hasPrefix("#"), !text.contains(" "), !text.contains("\n") else { return [] }
        let typed = text.dropFirst().lowercased()
        let names = roles.map(\.name) + ["claude-code", "codex"]
        return names.filter { typed.isEmpty || $0.lowercased().hasPrefix(typed) }
    }

    /// "Codex · gpt-5 · 调研员：5 次，回报 4，验收 2/3，退回 1".
    public static func reputation(_ row: LiveReputationRow, roles: [LiveDelegationRole]) -> String {
        var who = [agentLabel(row.agent)]
        if let model = row.model { who.append(model) }
        if let role = row.role { who.append(roles.first { $0.id == role }?.name ?? role) }
        var parts = ["\(row.total) 次", "回报 \(row.reported)"]
        if row.withAcceptance > 0 { parts.append("验收 \(row.accepted)/\(row.withAcceptance)") }
        if row.reworks > 0 { parts.append("退回 \(row.reworks)") }
        if row.failed > 0 { parts.append("失败 \(row.failed)") }
        return who.joined(separator: " · ") + "：" + parts.joined(separator: "，")
    }

    public static func permission(_ mode: String) -> String {
        switch mode {
        case "plan": return "只读"
        case "ask": return "每次询问"
        case "auto_edit": return "自动编辑"
        case "full_access": return "完全访问"
        default: return mode
        }
    }

    public static func agentLabel(_ agent: String) -> String {
        switch agent {
        case "claude": return "Claude Code"
        case "codex": return "Codex"
        default: return agent
        }
    }

    public static func audit(_ entry: LiveDelegationAudit) -> String {
        let action: String
        switch entry.action {
        case "start": action = "发起"
        case "refuse": action = "被拒绝"
        case "report": action = "回报"
        case "fail": action = "失败"
        case "stop": action = "停止"
        case "redo": action = "退回重做"
        case "send": action = "发给主会话"
        case "overBudget": action = "超出预算"
        default: action = entry.action
        }
        let actor = entry.actor == "user" ? "你" : (entry.actor == "aeroric" ? "Aeroric" : "会话")
        var parts = [actor + action]
        if !entry.permission.isEmpty { parts.append(DelegationText.permission(entry.permission)) }
        if !entry.detail.isEmpty { parts.append(entry.detail) }
        return parts.joined(separator: " · ")
    }
}
