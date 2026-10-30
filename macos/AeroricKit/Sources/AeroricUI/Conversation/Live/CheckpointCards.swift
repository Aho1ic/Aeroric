import SwiftUI
import AeroricProtocol

// B4′ (2026-10-30): the handoff card, the clarification card and a side
// branch's result card, drawn in the session timeline; and the rows the
// progress page shares with them. Every row is a capsule, one line, cut at
// the end; the badge says where the fact came from.

/// 已验证 / 推断 / 未知, set by the host from the entry's source.
struct EpistemicBadge: View {
    let status: String

    var body: some View {
        CheckpointBadge(text: Self.label(status), tint: Self.tint(status))
    }

    static func label(_ status: String) -> String {
        switch status {
        case "verified": "已验证"
        case "unknown": "未知"
        default: "推断"
        }
    }

    static func tint(_ status: String) -> Color {
        switch status {
        case "verified": AeroricColor.success
        case "unknown": AeroricColor.warning
        default: AeroricColor.accent
        }
    }
}

struct CheckpointBadge: View {
    let text: String
    let tint: Color

    var body: some View {
        Text(text)
            .font(.system(size: AeroricType.caption, weight: .medium))
            .foregroundStyle(tint)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 20)
            .background(Capsule(style: .continuous).fill(tint.opacity(0.12)))
    }
}

/// One fact: symbol, one line of text, a badge.
struct CheckpointRow<Badge: View>: View {
    let symbol: String
    let text: String
    var highlight: Color?
    @ViewBuilder let badge: () -> Badge

    var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: symbol)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(highlight ?? AeroricColor.textSecondary)
                .frame(width: 16)
            Text(text)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(highlight ?? AeroricColor.text)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(text)
            badge()
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 30)
        .background(Capsule(style: .continuous).fill((highlight ?? AeroricColor.layer2).opacity(highlight == nil ? 1 : 0.12)))
    }
}

struct CheckpointSectionLabel: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textSecondary)
            .lineLimit(1)
            .padding(.top, AeroricSpace.s)
    }
}

enum CheckpointText {
    static func reason(_ reason: String) -> String {
        switch reason {
        case "modelSwitch": "换模型"
        case "sourceMove": "换来源"
        case "afterCompact": "压缩后续接"
        case "delegate": "委派"
        case "resume": "恢复会话"
        default: "交接"
        }
    }

    static func effectSymbol(_ kind: String) -> String {
        switch kind {
        case "commit": "arrow.triangle.branch"
        case "changes", "fileChange": "doc.badge.ellipsis"
        case "mcp": "puzzlepiece.extension"
        default: "terminal"
        }
    }

    static func intent(_ state: String) -> (String, Color) {
        switch state {
        case "denied": ("已拒绝", AeroricColor.textSecondary)
        case "executed": ("已执行", AeroricColor.success)
        case "outcomeUnknown": ("结果未知", AeroricColor.warning)
        default: ("待审批", AeroricColor.accent)
        }
    }

    static func obligation(_ state: String) -> (String, String, Color) {
        switch state {
        case "passed": ("checkmark.circle", "检查通过", AeroricColor.success)
        case "failed": ("xmark.circle", "未通过", AeroricColor.danger)
        case "unknown": ("questionmark.circle", "未知", AeroricColor.warning)
        default: ("circle", "待做", AeroricColor.accent)
        }
    }

    /// "k 项待核对": unknown effects, unknown intents and open obligations.
    static func attention(_ cp: LiveCheckpoint) -> Int {
        cp.unknown.count
            + cp.intents.filter { $0.state == "outcomeUnknown" }.count
            + cp.obligations.filter { $0.state == "open" || $0.state == "unknown" }.count
    }
}

/// The facts of a checkpoint, grouped; shared by the handoff card and the
/// progress page.
struct CheckpointFacts: View {
    let checkpoint: LiveCheckpoint
    var showsIntents = true

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let goal = checkpoint.goal {
                CheckpointSectionLabel(title: "目标")
                CheckpointRow(symbol: "scope", text: goal.text) { EpistemicBadge(status: goal.status) }
            }
            if !checkpoint.decisions.isEmpty {
                CheckpointSectionLabel(title: "已确认决定")
                ForEach(Array(checkpoint.decisions.enumerated()), id: \.offset) { _, entry in
                    CheckpointRow(symbol: "checkmark.seal", text: entry.text) { EpistemicBadge(status: entry.status) }
                }
            }
            if !checkpoint.effects.isEmpty {
                CheckpointSectionLabel(title: checkpoint.effectsFolded > 0
                    ? "已发生改动（另有 \(checkpoint.effectsFolded) 条较早的）" : "已发生改动")
                ForEach(Array(checkpoint.effects.enumerated()), id: \.offset) { _, effect in
                    CheckpointRow(symbol: CheckpointText.effectSymbol(effect.kind),
                                  text: "\(effect.summary) · 回合 \(effect.turn)") {
                        CheckpointBadge(text: effect.evidence.isEmpty ? "无证据" : "有证据",
                                        tint: effect.evidence.isEmpty ? AeroricColor.textSecondary : AeroricColor.success)
                    }
                }
            }
            if !checkpoint.unknown.isEmpty {
                CheckpointSectionLabel(title: "结果未知")
                ForEach(Array(checkpoint.unknown.enumerated()), id: \.offset) { _, effect in
                    CheckpointRow(symbol: "exclamationmark.triangle", text: effect.summary, highlight: AeroricColor.warning) {
                        CheckpointBadge(text: "先核对", tint: AeroricColor.warning)
                    }
                }
            }
            if showsIntents, !checkpoint.intents.isEmpty {
                CheckpointSectionLabel(title: "拟执行（审批前记录）")
                ForEach(checkpoint.intents, id: \.requestId) { intent in
                    let (label, tint) = CheckpointText.intent(intent.state)
                    CheckpointRow(symbol: "hourglass", text: intent.summary) { CheckpointBadge(text: label, tint: tint) }
                }
            }
            if !checkpoint.obligations.isEmpty {
                let done = checkpoint.obligations.filter { $0.state == "passed" }.count
                CheckpointSectionLabel(title: "未完成义务 \(done)/\(checkpoint.obligations.count)")
                ForEach(checkpoint.obligations, id: \.id) { obligation in
                    let (symbol, label, tint) = CheckpointText.obligation(obligation.state)
                    CheckpointRow(symbol: symbol, text: obligation.text) { CheckpointBadge(text: label, tint: tint) }
                }
            }
            if !checkpoint.constraints.isEmpty {
                CheckpointSectionLabel(title: "约束")
                ForEach(Array(checkpoint.constraints.enumerated()), id: \.offset) { _, entry in
                    CheckpointRow(symbol: "lock", text: entry.text) { EpistemicBadge(status: entry.status) }
                }
            }
        }
    }
}

/// ① The brief this session was given, as it stood when it was handed over.
struct HandoffCard: View {
    let reason: String
    let checkpoint: LiveCheckpoint
    @State private var expanded: Bool?

    private var isExpanded: Bool { expanded ?? checkpoint.needsAttention }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Button {
                withAnimation(.easeOut(duration: 0.15)) { expanded = !isExpanded }
            } label: {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: "arrow.left.arrow.right")
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text("交接 · \(CheckpointText.reason(reason)) · 第 \(checkpoint.turn) 回合")
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    let attention = CheckpointText.attention(checkpoint)
                    if attention > 0 {
                        CheckpointBadge(text: "\(attention) 项待核对", tint: AeroricColor.warning)
                    }
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("session.live.handoff")
            if isExpanded {
                CheckpointFacts(checkpoint: checkpoint, showsIntents: false)
            }
        }
        .padding(AeroricSpace.l)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(AeroricColor.separator2, lineWidth: 0.5))
    }
}

/// ③ The clarification turn's criteria; nothing runs until they are accepted.
struct ClarificationCard: View {
    let clarification: LiveClarification
    let onAccept: (_ acceptance: [LiveAcceptance], _ constraints: [String]) -> Void
    @State private var ticked: Set<Int> = []
    @State private var initialised = false
    @State private var extra = ""
    @State private var constraint = ""
    @State private var added: [LiveAcceptance] = []
    @State private var constraints: [String] = []
    @State private var accepted = false

    private var all: [LiveAcceptance] { clarification.acceptance + added }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "checklist").foregroundStyle(AeroricColor.textSecondary)
                Text("开工前确认 · 验收条款")
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if accepted { CheckpointBadge(text: "已开工", tint: AeroricColor.success) }
            }
            if !clarification.requirements.isEmpty {
                CheckpointSectionLabel(title: "推断的需求")
                ForEach(Array(clarification.requirements.enumerated()), id: \.offset) { _, text in
                    CheckpointRow(symbol: "lightbulb", text: text) { EpistemicBadge(status: "inferred") }
                }
            }
            if let raw = clarification.raw, all.isEmpty {
                Text(raw)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
            }
            CheckpointSectionLabel(title: "验收条款")
            ForEach(Array(all.enumerated()), id: \.offset) { index, item in
                Button {
                    if ticked.contains(index) { ticked.remove(index) } else { ticked.insert(index) }
                } label: {
                    CheckpointRow(symbol: ticked.contains(index) ? "checkmark.square.fill" : "square",
                                  text: item.check.map { "\(item.text) · \($0)" } ?? item.text) {
                        CheckpointBadge(text: item.check == nil ? "需人工核对" : "可执行",
                                        tint: item.check == nil ? AeroricColor.accent : AeroricColor.success)
                    }
                }
                .buttonStyle(.plain)
                .disabled(accepted)
            }
            if !accepted {
                HStack(spacing: AeroricSpace.m) {
                    CapsuleField(placeholder: "补一条验收条款", text: $extra, height: 26)
                        .onSubmit(addCriterion)
                    Button("添加", action: addCriterion)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
                HStack(spacing: AeroricSpace.m) {
                    CapsuleField(placeholder: "约束，例如：不要推送", text: $constraint, height: 26)
                        .onSubmit(addConstraint)
                    Button("加约束", action: addConstraint)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
                ForEach(Array(constraints.enumerated()), id: \.offset) { _, text in
                    CheckpointRow(symbol: "lock", text: text) { EpistemicBadge(status: "verified") }
                }
            }
            if !clarification.questions.isEmpty {
                CheckpointSectionLabel(title: "想问你的")
                ForEach(Array(clarification.questions.enumerated()), id: \.offset) { _, text in
                    CheckpointRow(symbol: "questionmark.bubble", text: text) { EmptyView() }
                }
            }
            HStack {
                Spacer()
                Button(accepted ? "已按勾选开工" : "按勾选开工") {
                    accepted = true
                    onAccept(ticked.sorted().compactMap { all.indices.contains($0) ? all[$0] : nil }, constraints)
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                .fixedSize()
                .disabled(accepted)
                .accessibilityIdentifier("session.live.clarify.accept")
            }
        }
        .padding(AeroricSpace.l)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(AeroricColor.separator2, lineWidth: 0.5))
        .onAppear {
            guard !initialised else { return }
            initialised = true
            ticked = Set(clarification.acceptance.indices)
        }
    }

    private func addCriterion() {
        let text = extra.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        added.append(LiveAcceptance(text: text))
        ticked.insert(clarification.acceptance.count + added.count - 1)
        extra = ""
    }

    private func addConstraint() {
        let text = constraint.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        constraints.append(text)
        constraint = ""
    }
}

/// A side branch's conclusion; its process stays in the branch.
struct BranchResultCard: View {
    let result: LiveBranchResult
    let onSend: (String) -> Void
    @State private var sent = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "arrow.triangle.branch").foregroundStyle(AeroricColor.textSecondary)
                Text("侧支 · \(result.title)")
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if result.files > 0 {
                    CheckpointBadge(text: "\(result.files) 个文件 +\(result.additions) −\(result.deletions)",
                                    tint: AeroricColor.textSecondary)
                }
            }
            Text(result.summary)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.text)
                .textSelection(.enabled)
            HStack {
                Spacer()
                Button(sent ? "已发给主会话" : "发给主会话") {
                    sent = true
                    onSend("侧支「\(result.title)」的结论：\n\(result.summary)")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .disabled(sent)
            }
        }
        .padding(AeroricSpace.l)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(AeroricColor.separator2, lineWidth: 0.5))
    }
}
