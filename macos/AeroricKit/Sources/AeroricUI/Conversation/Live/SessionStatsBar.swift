import SwiftUI

/// Under the composer (2026-10-28, replaces Q28's single usage line): three
/// capsules — 会话统计, Token 用量, 上下文 — each opening a detail card.
/// Unknown figures are left out rather than shown as zero.
struct SessionStatsBar: View {
    let usage: UsageSummary
    let stats: SessionStats
    /// The whole session's turns and tool steps (history plus live rows).
    let turns: Int
    let steps: Int

    /// Turns (the person's own messages) and tool steps in a history.
    static func counts(_ history: [SessionMessage]) -> (turns: Int, steps: Int) {
        var turns = 0
        var steps = 0
        for message in history {
            let hasText = message.content.contains { if case .text = $0 { return true }; return false }
            if message.role == "user" && hasText { turns += 1 }
            steps += message.content.filter { if case .toolUse = $0 { return true }; return false }.count
        }
        return (turns, steps)
    }

    enum Panel: Hashable { case stats, tokens, context }
    @State private var open: Panel?

    var body: some View {
        // 2026-10-29: always all three, whatever the session's state; an
        // unknown figure reads "—" rather than hiding its chip.
        HStack(spacing: AeroricSpace.s) {
            chip(.stats, systemImage: "gauge.with.dots.needle.33percent", text: SessionStatsText.statsChip(
                turns: turns, steps: steps, tokensPerSecond: stats.tokensPerSecond)) { statsCard }
            chip(.tokens, systemImage: "cylinder", text: SessionStatsText.tokensChip(
                total: usage.totalTokens, hitRate: usage.cacheHitRate)) { tokensCard }
            contextChip(usage.fraction)
        }
        .frame(maxWidth: .infinity)
        .font(.system(size: AeroricType.caption))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.live.usage")
    }

    // MARK: Chips

    private func binding(_ panel: Panel) -> Binding<Bool> {
        Binding(get: { open == panel }, set: { if !$0, open == panel { open = nil } })
    }

    private func chip(_ panel: Panel, systemImage: String, text: String,
                      @ViewBuilder card: @escaping () -> some View) -> some View {
        Button { open = open == panel ? nil : panel } label: {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: systemImage).font(.system(size: 11)).foregroundStyle(AeroricColor.textTertiary)
                Text(text).monospacedDigit()
            }
            .foregroundStyle(AeroricColor.textSecondary)
            .padding(.horizontal, AeroricSpace.m + 2)
            .frame(height: 24)
            .background(open == panel ? AeroricColor.layer2 : .clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
        .popover(isPresented: binding(panel), arrowEdge: .top) { card() }
        .accessibilityLabel(text)
    }

    private func contextChip(_ fraction: Double?) -> some View {
        let tint = usage.isHigh ? AeroricColor.warning : AeroricColor.textSecondary
        let text = SessionStatsText.contextChip(fraction: fraction, used: usage.contextUsed)
        return Button { open = open == .context ? nil : .context } label: {
            HStack(spacing: AeroricSpace.s) {
                ZStack {
                    Circle().stroke(AeroricColor.separator2, lineWidth: 2)
                    Circle().trim(from: 0, to: fraction ?? 0)
                        .stroke(tint, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                }
                .frame(width: 11, height: 11)
                Text(text).monospacedDigit()
            }
            .foregroundStyle(tint)
            .padding(.horizontal, AeroricSpace.m + 2)
            .frame(height: 24)
            .background(open == .context ? AeroricColor.layer2 : .clear,
                        in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
        .popover(isPresented: binding(.context), arrowEdge: .top) { contextCard(fraction) }
        .accessibilityLabel("上下文已用 \(text)")
    }

    // MARK: Cards

    private var statsCard: some View {
        card(systemImage: "gauge.with.dots.needle.33percent", title: "会话统计", trailing: nil) {
            row("模型用时", stats.modelMs > 0 ? SessionStatsText.duration(stats.modelMs) : nil)
            row("工具调用用时", stats.toolMs > 0 ? SessionStatsText.duration(stats.toolMs) : nil)
            row("首 token 平均（TTFT）", stats.averageFirstTokenMs.map(SessionStatsText.duration))
            row("输出速度（TPS）", stats.tokensPerSecond.map { "\(Int($0.rounded())) tok/s" })
            if !stats.wholeSession, stats.turns < turns {
                Text("用时从本次打开会话起统计")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.top, AeroricSpace.xs)
            }
        }
    }

    private var tokensCard: some View {
        card(systemImage: "cylinder", title: "Token 用量",
             trailing: usage.totalTokens.map { "\(SessionStatsText.grouped($0)) tok" }) {
            row("缓存命中", usage.cacheHitRate.map { "\(Int(($0 * 100).rounded()))%" })
            row("未缓存输入", usage.uncachedInputTokens.map { "\(SessionStatsText.grouped($0)) tok" })
            row("缓存读取", usage.cachedInputTokens.map { "\(SessionStatsText.grouped($0)) tok" })
            row("输出", usage.outputTokens.map { "\(SessionStatsText.grouped($0)) tok" })
        }
    }

    private func contextCard(_ fraction: Double?) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text("上下文已用").foregroundStyle(AeroricColor.textSecondary)
                Text(fraction.map { "\(Int(($0 * 100).rounded()))%" } ?? "—").fontWeight(.medium)
                Spacer(minLength: AeroricSpace.l)
                if let used = usage.contextUsed, let limit = usage.contextLimit {
                    Text("~\(SessionStatsText.compact(used)) / \(SessionStatsText.compact(limit))").monospacedDigit()
                }
            }
            .font(.system(size: AeroricType.secondary))
            GeometryReader { proxy in
                ZStack(alignment: .leading) {
                    Capsule().fill(AeroricColor.layer3)
                    Capsule().fill(usage.isHigh ? AeroricColor.warning : AeroricColor.accent)
                        .frame(width: max(4, proxy.size.width * (fraction ?? 0)))
                        .opacity(fraction == nil ? 0 : 1)
                }
            }
            .frame(height: 6)
            if let used = usage.contextUsed, let limit = usage.contextLimit, limit >= used {
                row("剩余", "~\(SessionStatsText.compact(limit - used))")
            }
            if usage.isHigh {
                Text("上下文快满了，可用 /compact 压缩")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 300)
    }

    private func card(systemImage: String, title: String, trailing: String?,
                      @ViewBuilder rows: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: systemImage)
                Text(title).fontWeight(.medium)
                Spacer(minLength: AeroricSpace.l)
                if let trailing { Text(trailing).fontWeight(.medium).monospacedDigit() }
            }
            .font(.system(size: AeroricType.secondary))
            Divider()
            rows()
        }
        .padding(AeroricSpace.xl)
        .frame(width: 300)
    }

    @ViewBuilder private func row(_ label: String, _ value: String?) -> some View {
        HStack {
            Text(label).foregroundStyle(AeroricColor.textSecondary)
            Spacer(minLength: AeroricSpace.l)
            Text(value ?? "—").monospacedDigit()
        }
        .font(.system(size: AeroricType.secondary))
    }
}

/// Text for the 统计栏 (pure, tested).
enum SessionStatsText {
    static func statsChip(turns: Int, steps: Int, tokensPerSecond: Double?) -> String {
        let base = "\(turns) 轮 \(steps) 步"
        guard let tokensPerSecond else { return base }
        return base + " · \(Int(tokensPerSecond.rounded())) tok/s"
    }

    static func tokensChip(total: UInt64?, hitRate: Double?) -> String {
        guard let total else { return "— tok" }
        let base = "\(compact(total).uppercased()) tok"
        guard let hitRate else { return base }
        return base + " · 缓存命中 \(Int((hitRate * 100).rounded()))%"
    }

    /// `17%`; without a known window the used count (`43.6K`); else `—`.
    static func contextChip(fraction: Double?, used: UInt64?) -> String {
        if let fraction { return "\(Int((fraction * 100).rounded()))%" }
        if let used { return compact(used) }
        return "—"
    }

    /// `3 分 16 秒`, `38.4 秒`, `0.8 秒`.
    static func duration(_ ms: UInt64) -> String {
        let seconds = Double(ms) / 1000
        if seconds < 60 { return String(format: "%.1f 秒", seconds).replacingOccurrences(of: ".0 秒", with: " 秒") }
        let whole = Int(seconds.rounded())
        if whole < 3600 { return "\(whole / 60) 分 \(whole % 60) 秒" }
        return "\(whole / 3600) 小时 \(whole % 3600 / 60) 分"
    }

    static func compact(_ count: UInt64) -> String { UsageSummary.compact(count).uppercased() }

    static func grouped(_ count: UInt64) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.groupingSeparator = ","
        return formatter.string(from: NSNumber(value: count)) ?? "\(count)"
    }
}
