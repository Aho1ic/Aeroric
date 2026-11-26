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
    @State private var showsFiles = false
    @State private var showsEarlier = false
    @State private var showsFiltered = false

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
            if let coverage = card.coverage, let (line, tint) = ReviewText.coverage(coverage) {
                Button { showsFiles.toggle() } label: {
                    HStack(spacing: AeroricSpace.xs) {
                        Image(systemName: coverage.state == "complete" ? "checklist" : "exclamationmark.triangle")
                        Text(line).lineLimit(1).truncationMode(.tail)
                        Image(systemName: showsFiles ? "chevron.down" : "chevron.right")
                            .font(.system(size: AeroricType.caption - 2))
                    }
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(tint)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("这一轮改动的每个文件，审查者审了、看过、跳过还是没看")
                .accessibilityIdentifier("review.coverage")
                if showsFiles {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(coverage.files) { file in
                            Text(ReviewText.fileLine(file))
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(file.state == "missed" ? AeroricColor.warning : AeroricColor.textSecondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .help(file.path)
                        }
                    }
                    .padding(.leading, AeroricSpace.l)
                }
            }
            if let compare = card.compare, let line = ReviewText.compare(card) {
                Button { showsEarlier.toggle() } label: {
                    HStack(spacing: AeroricSpace.xs) {
                        Image(systemName: "arrow.left.arrow.right")
                        Text(line).lineLimit(1).truncationMode(.tail)
                        if !compare.resolved.isEmpty || !compare.unchecked.isEmpty {
                            Image(systemName: showsEarlier ? "chevron.down" : "chevron.right")
                                .font(.system(size: AeroricType.caption - 2))
                        }
                    }
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(compare.unchecked.isEmpty ? AeroricColor.textSecondary : AeroricColor.warning)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("和第 \(compare.previousRound) 轮比：哪些是新的、哪些还在、哪些改好了、哪些这轮没人再看")
                .accessibilityIdentifier("review.compare")
                if showsEarlier {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(ReviewText.earlier(compare), id: \.self) { line in
                            Text(line)
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(line.hasPrefix("未复查") ? AeroricColor.warning : AeroricColor.textSecondary)
                                .lineLimit(1)
                                .truncationMode(.tail)
                                .help(line)
                        }
                    }
                    .padding(.leading, AeroricSpace.l)
                }
            }
            if let check = card.factCheck, let (line, tone) = ReviewText.factCheck(check) {
                Button { showsFiltered.toggle() } label: {
                    HStack(spacing: AeroricSpace.xs) {
                        Image(systemName: "checkmark.shield")
                        Text(line).lineLimit(1).truncationMode(.tail)
                        if !check.filtered.isEmpty {
                            Image(systemName: showsFiltered ? "chevron.down" : "chevron.right")
                                .font(.system(size: AeroricType.caption - 2))
                        }
                    }
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(tone)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(check.reason ?? ReviewText.factCheckHelp(check))
                .accessibilityIdentifier("review.fact-check")
                if showsFiltered {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(check.filtered) { item in
                            VStack(alignment: .leading, spacing: 1) {
                                Text(ReviewText.line(item.finding))
                                    .foregroundStyle(AeroricColor.textSecondary)
                                    .strikethrough()
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                Text(ReviewText.filteredGround(item))
                                    .font(.system(size: AeroricType.caption, design: item.ground == "refuted" ? .monospaced : .default))
                                    .foregroundStyle(AeroricColor.textTertiary)
                                    .lineLimit(2)
                                    .truncationMode(.tail)
                            }
                            .font(.system(size: AeroricType.caption))
                            .help(item.finding.text)
                            .accessibilityIdentifier("review.fact-check.filtered.\(item.id)")
                        }
                    }
                    .padding(.leading, AeroricSpace.l)
                }
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
        // Review 2026-11-23 (1123c): a container, or the card's id replaces
        // every id inside it (the 通过 button read as `review.card`).
        .accessibilityElement(children: .contain)
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
                    if let snippet = finding.snippet {
                        Text(snippet)
                            .font(.system(size: AeroricType.code, design: .monospaced))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .textSelection(.enabled)
                            .padding(.vertical, 2)
                    }
                    ForEach(ReviewText.notes(finding), id: \.self) { note in
                        Text(note).foregroundStyle(AeroricColor.textTertiary)
                    }
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

    /// "client.rs:88 重试没有上限", with "（3 条相同）" when others were folded in.
    public static func line(_ finding: LiveReviewFinding) -> String {
        let text = [finding.location, finding.text].compactMap { $0 }.joined(separator: " ")
        return finding.merged > 0 ? "\(text)（\(finding.merged + 1) 条相同）" : text
    }

    /// E2: how the finding's place and its duplicates were settled.
    public static func notes(_ finding: LiveReviewFinding) -> [String] {
        var notes: [String] = []
        if let moved = finding.movedFrom {
            notes.append("位置已按代码修正（审查者写的是第 \(moved) 行）")
        }
        if finding.unlocated {
            notes.append("引用的代码在文件里找不到，位置以审查者写的为准")
        }
        if finding.merged > 0 {
            notes.append("合并了 \(finding.merged) 条说同一处的意见")
        }
        if let rule = finding.rule {
            notes.append("依据规则：\(rule)")
        }
        if let earlier = finding.repeats {
            notes.append("上一轮也提过（当时是 \(earlier)），还没改好")
        }
        return notes
    }

    /// E3: "新增 2 · 仍在 1 · 已解决 3 · 未复查 1"; nil when there is
    /// nothing to compare.
    public static func compare(_ card: LiveReviewCard) -> String? {
        guard let compare = card.compare else { return nil }
        let persisting = card.findings.filter { $0.repeats != nil }.count
        let parts = [("新增", card.findings.count - persisting), ("仍在", persisting),
                     ("已解决", compare.resolved.count), ("未复查", compare.unchecked.count)]
            .filter { $0.1 > 0 }
            .map { "\($0.0) \($0.1)" }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// E2 (2026-11-26): "已过滤 2 条被 diff 直接驳倒的意见"; nil when the
    /// check took nothing out and kept nothing back to say.
    public static func factCheck(_ check: LiveFactCheck) -> (String, Color)? {
        switch check.state {
        case "running": return ("正在核查意见…", AeroricColor.textSecondary)
        case "failed": return (check.reason ?? "核查没跑成；意见全部保留", AeroricColor.warning)
        default:
            if !check.filtered.isEmpty {
                return ("已过滤 \(check.filtered.count) 条被 diff 直接驳倒的意见", AeroricColor.textSecondary)
            }
            return ("核查过了，没有过滤意见", AeroricColor.textTertiary)
        }
    }

    public static func factCheckHelp(_ check: LiveFactCheck) -> String {
        var parts = ["分析模型对照改动核查了这一轮的意见；只删引用的代码不在改动里、或被改动里某一行直接驳倒的"]
        if let model = check.model { parts.append("模型：\(model)") }
        if check.guarded > 0 { parts.append("另有 \(check.guarded) 条模型想删、但没过 Aeroric 的检查，照样保留") }
        return parts.joined(separator: "；")
    }

    /// Why one finding was taken out.
    public static func filteredGround(_ item: LiveFactCheck.Filtered) -> String {
        item.ground == "notInDiff" ? item.evidence : "被这一行驳倒：\(item.evidence)"
    }

    /// E3: the last round's findings not said again, one line each.
    public static func earlier(_ compare: LiveRoundCompare) -> [String] {
        compare.resolved.map { "已解决 · \(line($0))" }
            + compare.unchecked.map { "未复查（这一轮没人再看这个文件） · \(line($0))" }
    }

    /// E1: what the round rests on, e.g. "审查覆盖 7/9 个文件（结果可能不完整）".
    public static func coverage(_ coverage: LiveReviewCoverage) -> (String, Color)? {
        let total = coverage.inScope.count
        switch coverage.state {
        case "complete": return ("审查覆盖 \(total)/\(total) 个文件", AeroricColor.textSecondary)
        case "partial":
            return ("审查覆盖 \(coverage.looked)/\(total) 个文件（结果可能不完整）", AeroricColor.warning)
        case "unreviewed": return ("没有审查：\(total) 个改动的文件一个都没看", AeroricColor.danger)
        case "empty": return ("改动的文件都不在审查范围", AeroricColor.textSecondary)
        default: return nil
        }
    }

    /// One file's end: "src/a.rs · 已审", "x.rs · 跳过：生成的代码".
    public static func fileLine(_ file: LiveReviewCoverage.File) -> String {
        let end: String
        switch file.state {
        case "reviewed": end = "已审"
        case "seen": end = "看过（审查者没有逐个交代）"
        case "skipped": end = "跳过：\(file.reason ?? "没写原因")"
        case "missed": end = "没看"
        case "excluded": end = "不在范围：\(file.reason ?? "")"
        default: end = file.state
        }
        return "\(file.path) · \(end)"
    }

    /// The progress page's one-line summary of the latest loop.
    public static func summary(_ card: LiveReviewCard) -> String {
        let asks = card.findings.filter { $0.decision == "ask" }.count
        var parts = ["第 \(card.round) 轮", state(card).0]
        if let coverage = card.coverage, coverage.state == "partial" || coverage.state == "unreviewed" {
            parts.append("覆盖 \(coverage.looked)/\(coverage.inScope.count)")
        }
        if !card.findings.isEmpty { parts.append("\(card.findings.count) 条") }
        if let unchecked = card.compare?.unchecked.count, unchecked > 0 {
            parts.append("\(unchecked) 条未复查")
        }
        if asks > 0 { parts.append("\(asks) 条需要你定") }
        return parts.joined(separator: " · ")
    }
}
