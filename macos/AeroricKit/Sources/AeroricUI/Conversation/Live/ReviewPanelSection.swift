import SwiftUI
import AeroricProtocol

/// B4-1: what the progress page knows about a task's review loop.
public struct ReviewPanelModel: Equatable, Sendable {
    /// nil for a task that is never reviewed (a reviewer, a side branch).
    public var settings: LiveReviewSettings?
    public var candidates: [LiveReviewerCandidate]
    /// Whom `auto` picks now; nil when no harness can review.
    public var auto: LiveReviewerCandidate?
    public var card: LiveReviewCard?
    /// This task reviews that one.
    public var reviewOf: String?
    /// E4: the review rules in force for the task's project.
    public var rules: LiveReviewRules?

    public init(settings: LiveReviewSettings?, candidates: [LiveReviewerCandidate] = [],
                auto: LiveReviewerCandidate? = nil, card: LiveReviewCard? = nil, reviewOf: String? = nil) {
        self.settings = settings
        self.candidates = candidates
        self.auto = auto
        self.card = card
        self.reviewOf = reviewOf
    }

    /// The reviewer picker's closed title.
    public var reviewerTitle: String {
        switch settings?.reviewer ?? .auto {
        case .auto:
            guard let auto else { return "自动 · 本机没有可用的审查者" }
            return "自动 · \(auto.label)\(auto.sameFamily ? "（同族）" : "")"
        case let .named(agent, _, model):
            let label = candidates.first { $0.agent == agent }?.label ?? agent
            return model.map { "\(label) · \($0)" } ?? label
        }
    }

    /// E4: "审查规则 · 内置 9 条 · 项目 2 条（关了 1 条内置）".
    public var rulesTitle: String? {
        guard let rules else { return nil }
        let builtin = rules.rules.filter { $0.source == "builtin" }
        let on = builtin.filter { !$0.disabled }.count
        let project = rules.rules.count - builtin.count
        var title = "审查规则 · 内置 \(on) 条"
        if project > 0 { title += " · 项目 \(project) 条" }
        if on < builtin.count { title += "（关了 \(builtin.count - on) 条内置）" }
        return title
    }

    /// E4: where the project's own rules come from.
    public var rulesHint: String? {
        guard let rules else { return nil }
        switch rules.state {
        case "missing": return "项目可以在 \(rules.file) 里加规则、关掉或替换内置规则"
        case "loaded": return "项目规则来自 \(rules.file)；审查时按改动前的版本"
        case "invalid": return "规则文件读不懂，只用了内置规则：\(rules.error ?? rules.file)"
        case "unavailable": return "远程项目读不到规则文件，只用内置规则"
        default: return nil
        }
    }

    /// E4: one rule, e.g. "rust · 重要 · *.rs" or "money · 阻断 · src/billing/**（项目）".
    public static func ruleLine(_ rule: LiveReviewRules.Rule) -> String {
        var line = "\(rule.id) · \(ReviewText.severity(rule.severity).0) · \(rule.paths.joined(separator: ", "))"
        if rule.source == "project" { line += "（项目）" }
        if rule.disabled { line += "（已关闭）" }
        return line
    }

    /// One line under the picker.
    public var reviewerHint: String {
        if case .auto = settings?.reviewer ?? .auto, auto?.sameFamily == true {
            return "本机只有同族可用；只读、只看 diff"
        }
        return "与当前会话不同族时更有效；只读、只看 diff"
    }
}

public enum ReviewPanelAction: Equatable, Sendable {
    case setEnabled(Bool)
    case pick(LiveReviewerChoice)
    case request
    case open(String)
}

struct ReviewPanelSection: View {
    let model: ReviewPanelModel
    let onAction: (ReviewPanelAction) -> Void
    @State private var showsRules = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            CheckpointSectionLabel(title: "审查")
            if let parent = model.reviewOf {
                Button { onAction(.open(parent)) } label: {
                    CheckpointRow(symbol: "eye", text: "这是一个审查会话，只读") {
                        Text("打开被审的会话")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.accent)
                            .fixedSize()
                    }
                }
                .buttonStyle(.plain)
            } else if let settings = model.settings {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: "eye.trianglebadge.exclamationmark")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 16)
                    Text("审查回环")
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Toggle("审查回环", isOn: Binding(get: { settings.enabled },
                                                   set: { onAction(.setEnabled($0)) }))
                        .toggleStyle(.switch)
                        .controlSize(.mini)
                        .labelsHidden()
                        .accessibilityIdentifier("progress.review.toggle")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
                .help("打开后，每个改了文件的回合结束时，由另一族的 Agent 只读审查改动；会用你的账号调用模型")
                HStack(spacing: AeroricSpace.m) {
                    CapsulePicker(title: model.reviewerTitle, systemImage: "person.crop.circle.badge.checkmark") {
                        Button("自动（优先不同族）") { onAction(.pick(.auto)) }
                        if !model.candidates.isEmpty { Divider() }
                        ForEach(model.candidates, id: \.agent) { candidate in
                            Button(candidate.sameFamily ? "\(candidate.label)（同族）" : candidate.label) {
                                onAction(.pick(.named(agent: candidate.agent)))
                            }
                        }
                    }
                    .accessibilityIdentifier("progress.review.reviewer")
                    Spacer(minLength: 0)
                    Button("请人审查") { onAction(.request) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .disabled(model.card?.isOpen == true)
                        .help("现在审查上一个改了文件的回合")
                        .accessibilityIdentifier("progress.review.request")
                }
                Text(model.reviewerHint)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                if let title = model.rulesTitle, let rules = model.rules {
                    Button { showsRules.toggle() } label: {
                        HStack(spacing: AeroricSpace.xs) {
                            Image(systemName: "list.bullet.rectangle")
                            Text(title).lineLimit(1).truncationMode(.tail)
                            Image(systemName: showsRules ? "chevron.down" : "chevron.right")
                                .font(.system(size: AeroricType.caption - 2))
                        }
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(rules.state == "invalid" ? AeroricColor.warning : AeroricColor.textSecondary)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("按文件路径匹配的审查规则；只有命中改动文件的规则会交给审查者")
                    .accessibilityIdentifier("progress.review.rules")
                    if showsRules {
                        VStack(alignment: .leading, spacing: 2) {
                            if let hint = model.rulesHint {
                                Text(hint)
                                    .foregroundStyle(rules.state == "invalid" ? AeroricColor.warning : AeroricColor.textTertiary)
                                    .lineLimit(2)
                                    .textSelection(.enabled)
                            }
                            ForEach(rules.rules) { rule in
                                Text(ReviewPanelModel.ruleLine(rule))
                                    .foregroundStyle(rule.disabled ? AeroricColor.textTertiary : AeroricColor.textSecondary)
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                    .help(rule.text)
                            }
                        }
                        .font(.system(size: AeroricType.caption))
                        .padding(.leading, AeroricSpace.l)
                    }
                }
                if let card = model.card {
                    Button {
                        if let reviewer = card.reviewerTaskId { onAction(.open(reviewer)) }
                    } label: {
                        CheckpointRow(symbol: "eye", text: ReviewText.summary(card),
                                      highlight: card.needsUser ? AeroricColor.warning : nil) {
                            if card.reviewerTaskId != nil {
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}
