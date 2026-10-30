import SwiftUI
import AeroricProtocol

/// B4-1: one review loop in the timeline — the round, who reviewed, each
/// finding with its severity and the writer's answer, and what the user can
/// do next. Rows are capsules; nothing wraps.
struct ReviewCard: View {
    let card: LiveReviewCard
    let paused: Bool
    let onSend: () -> Void
    let onRetry: () -> Void
    let onPause: (Bool) -> Void
    let onOpen: (String) -> Void
    @State private var expanded: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "eye.trianglebadge.exclamationmark").foregroundStyle(AeroricColor.textSecondary)
                Text("审查 · 第 \(card.round) 轮")
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(1)
                    .fixedSize()
                Text(ReviewText.reviewer(card))
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: .infinity, alignment: .leading)
                let (label, tint) = ReviewText.state(card)
                CheckpointBadge(text: label, tint: tint)
            }
            if let summary = card.summary {
                Text("总评：\(summary)")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .help(summary)
            }
            ForEach(card.findings, id: \.id) { finding in
                findingRow(finding)
            }
            if let raw = card.raw {
                Text(raw)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(4)
                    .textSelection(.enabled)
            }
            HStack(spacing: AeroricSpace.m) {
                if card.state == "waitingSend" {
                    Button("发给写代码的会话", action: onSend)
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("review.send")
                }
                if card.isOpen {
                    Button(paused ? "之后每轮自动发" : "之后先别自动发") { onPause(!paused) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .help(paused ? "审查意见出来后直接发给写代码的会话" : "审查意见出来后先停在这里，由你决定是否发出")
                        .accessibilityIdentifier("review.pause")
                }
                if card.state == "failed" || card.state == "interrupted" {
                    Button("重新审查", action: onRetry)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("review.retry")
                }
                Spacer(minLength: 0)
                if let reviewer = card.reviewerTaskId {
                    Button("打开审查会话") { onOpen(reviewer) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("review.open")
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(AeroricColor.separator2, lineWidth: 0.5))
        .accessibilityIdentifier("review.card")
    }

    @ViewBuilder private func findingRow(_ finding: LiveReviewFinding) -> some View {
        let open = expanded.contains(finding.id)
        VStack(alignment: .leading, spacing: 2) {
            Button {
                if open { expanded.remove(finding.id) } else { expanded.insert(finding.id) }
            } label: {
                HStack(spacing: AeroricSpace.m) {
                    let (word, tint) = ReviewText.severity(finding.severity)
                    CheckpointBadge(text: word, tint: tint)
                    Text(ReviewText.line(finding))
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if let (answer, tint) = ReviewText.decision(finding) {
                        CheckpointBadge(text: answer, tint: tint)
                    }
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            if open {
                VStack(alignment: .leading, spacing: 2) {
                    Text(finding.text).textSelection(.enabled)
                    if let reason = finding.reason {
                        Text("答复：\(reason)").foregroundStyle(AeroricColor.textSecondary).textSelection(.enabled)
                    }
                }
                .font(.system(size: AeroricType.caption))
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.xs)
            }
        }
    }
}

public enum ReviewText {
    public static func reviewer(_ card: LiveReviewCard) -> String {
        guard !card.reviewerLabel.isEmpty else { return "正在找审查者" }
        return card.sameFamily ? "\(card.reviewerLabel)（同族）" : card.reviewerLabel
    }

    public static func state(_ card: LiveReviewCard) -> (String, Color) {
        switch card.state {
        case "reviewing": return ("审查中", AeroricColor.accent)
        case "waitingSend": return ("已暂停，等你发出", AeroricColor.textSecondary)
        case "waitingCoder": return ("等写代码的会话答复", AeroricColor.warning)
        case "passed": return ("通过", AeroricColor.success)
        case "settled":
            return card.rejectedBlocker ? ("已结束 · 阻断被驳回，请看一眼", AeroricColor.warning)
                : ("已结束", AeroricColor.textSecondary)
        case "escalated": return ("需要你处理", AeroricColor.danger)
        case "interrupted": return ("已中断", AeroricColor.textSecondary)
        case "failed": return ("审查失败", AeroricColor.danger)
        default: return (card.state, AeroricColor.textSecondary)
        }
    }

    public static func severity(_ severity: String) -> (String, Color) {
        switch severity {
        case "blocker": return ("阻断", AeroricColor.danger)
        case "major": return ("重要", AeroricColor.warning)
        default: return ("次要", AeroricColor.textSecondary)
        }
    }

    public static func decision(_ finding: LiveReviewFinding) -> (String, Color)? {
        switch finding.decision {
        case "adopt": return ("已采纳", AeroricColor.success)
        case "reject":
            return (finding.reason.map { "驳回 · \(short($0))" } ?? "已驳回", AeroricColor.textSecondary)
        case "ask": return ("需要你定", AeroricColor.accent)
        default: return nil
        }
    }

    /// A badge keeps one short line; the full reason shows when the row opens.
    static func short(_ text: String) -> String {
        text.count > 12 ? String(text.prefix(12)) + "…" : text
    }

    /// "client.rs:88 重试没有上限".
    public static func line(_ finding: LiveReviewFinding) -> String {
        [finding.location, finding.text].compactMap { $0 }.joined(separator: " ")
    }

    /// The progress page's one-line summary of the latest loop.
    public static func summary(_ card: LiveReviewCard) -> String {
        let asks = card.findings.filter { $0.decision == "ask" }.count
        var parts = ["第 \(card.round) 轮", state(card).0]
        if !card.findings.isEmpty { parts.append("\(card.findings.count) 条") }
        if asks > 0 { parts.append("\(asks) 条需要你定") }
        return parts.joined(separator: " · ")
    }
}
