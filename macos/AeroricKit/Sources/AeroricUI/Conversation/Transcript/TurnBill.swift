import AeroricProtocol
import SwiftUI

/// R3-1 (2026-11-09): a finished turn's own bill, read back from the agent's
/// transcript by the host (`session.history` turn stats) — so old sessions
/// have one too. Figures the transcript lacks read "—"; an amount is shown
/// only for a model on the price list, and always as an estimate.
public enum TurnBillText {
    /// Tool rows the card shows before "还有 N 条".
    public static let visibleTools = 30

    /// Whether the turn has anything to bill.
    public static func hasBill(_ stat: SessionTranscriptStats.TurnStat?) -> Bool {
        guard let stat else { return false }
        return stat.tokens > 0 || stat.inputTokens != nil || !stat.tools.isEmpty
    }

    /// `12.3K tok · ≈$0.012`; tokens only for an unpriced model.
    public static func chip(_ stat: SessionTranscriptStats.TurnStat) -> String {
        var parts: [String] = []
        if stat.tokens > 0 { parts.append("\(SessionStatsText.compact(stat.tokens)) tok") }
        if let cost = stat.costUsd { parts.append("≈\(money(cost))") }
        if parts.isEmpty { parts.append("\(stat.tools.count + stat.moreTools) 次工具") }
        return parts.joined(separator: " · ")
    }

    /// `$0.012`; `<$0.001` for less.
    public static func money(_ usd: Double) -> String {
        if usd < 0.001 { return "<$0.001" }
        if usd < 1 { return String(format: "$%.3f", usd) }
        return String(format: "$%.2f", usd)
    }

    /// The part of the prompt read from the cache, `72%`.
    public static func hitRate(_ stat: SessionTranscriptStats.TurnStat) -> String? {
        guard let input = stat.inputTokens, input > 0, let cached = stat.cachedInputTokens else { return nil }
        return "\(Int((Double(min(cached, input)) / Double(input) * 100).rounded()))%"
    }

    public static func tokens(_ count: UInt64?) -> String? {
        count.map { "\(SessionStatsText.grouped($0)) tok" }
    }

    /// `23 次 · 2 次失败 · 共 41 秒`.
    public static func toolsSummary(_ stat: SessionTranscriptStats.TurnStat) -> String {
        let count = stat.tools.count + stat.moreTools
        var parts = ["\(count) 次"]
        let failed = stat.tools.filter(\.failed).count
        if failed > 0 { parts.append("\(failed) 次失败") }
        let spent = stat.tools.reduce(UInt64(0)) { $0 + (duration($1) ?? 0) }
        if spent > 0 { parts.append("共 \(SessionStatsText.duration(spent))") }
        return parts.joined(separator: " · ")
    }

    public static func duration(_ span: SessionTranscriptStats.ToolSpan) -> UInt64? {
        guard let ended = span.endedAt, ended >= span.startedAt else { return nil }
        return ended - span.startedAt
    }

    public enum Speed: Equatable { case fast, medium, slow, failed, unfinished }

    /// Under a second fast, under ten seconds medium, else slow.
    public static func speed(_ span: SessionTranscriptStats.ToolSpan) -> Speed {
        if span.failed { return .failed }
        guard let ms = duration(span) else { return .unfinished }
        return ms < 1000 ? .fast : ms < 10_000 ? .medium : .slow
    }

    /// Where a span's bar sits in the turn, as fractions of its length.
    /// A span without an end runs to the turn's end.
    public static func bar(_ span: SessionTranscriptStats.ToolSpan, turnStart: UInt64,
                           turnEnd: UInt64) -> (start: Double, width: Double) {
        let total = Double(max(turnEnd, turnStart + 1) - turnStart)
        let from = Double(min(max(span.startedAt, turnStart), turnEnd) - turnStart)
        let to = Double(min(max(span.endedAt ?? turnEnd, span.startedAt), turnEnd) - turnStart)
        return (from / total, max(0, to - from) / total)
    }
}

/// The capsule under a turn's reply; opens the bill.
struct TurnBillChip: View {
    let stat: SessionTranscriptStats.TurnStat
    @State private var open = false

    var body: some View {
        Button { open.toggle() } label: {
            HStack(spacing: AeroricSpace.xs) {
                Image(systemName: "cylinder").font(.system(size: 10))
                Text(TurnBillText.chip(stat)).monospacedDigit()
            }
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 20)
            .background(open ? AeroricColor.layer2 : .clear, in: Capsule(style: .continuous))
            .overlay(Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1))
            .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .help("本回合账单")
        .popover(isPresented: $open, arrowEdge: .top) { TurnBillCard(stat: stat) }
        .accessibilityLabel("本回合账单 \(TurnBillText.chip(stat))")
        .accessibilityIdentifier("session.turn.bill")
    }
}

struct TurnBillCard: View {
    let stat: SessionTranscriptStats.TurnStat
    @State private var showAll = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "cylinder")
                Text("本回合账单").fontWeight(.medium)
                Spacer(minLength: AeroricSpace.l)
                if let cost = stat.costUsd {
                    Text("≈\(TurnBillText.money(cost))").fontWeight(.medium).monospacedDigit()
                }
            }
            .font(.system(size: AeroricType.secondary))
            Divider()
            row("模型", stat.model)
            row("输入", TurnBillText.tokens(stat.inputTokens))
            row("其中缓存读取", stat.cachedInputTokens.map { cached in
                [TurnBillText.tokens(cached), TurnBillText.hitRate(stat)].compactMap { $0 }.joined(separator: " · ")
            })
            row("输出", TurnBillText.tokens(stat.outputTokens))
            if let ended = stat.endedAt, ended >= stat.startedAt {
                row("用时", SessionStatsText.duration(ended - stat.startedAt))
            }
            Text(stat.costUsd == nil
                 ? (stat.model == nil ? "会话文件里没有记模型，不估算金额" : "价目表里没有这个模型，不估算金额")
                 : "金额按价目表估算，以服务商账单为准")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            if !stat.tools.isEmpty {
                Divider()
                HStack {
                    Text("工具调用").fontWeight(.medium)
                    Spacer(minLength: AeroricSpace.l)
                    Text(TurnBillText.toolsSummary(stat)).foregroundStyle(AeroricColor.textSecondary)
                        .monospacedDigit()
                }
                .font(.system(size: AeroricType.secondary))
                waterfall
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 380)
    }

    private var waterfall: some View {
        let end = max(stat.endedAt ?? 0, stat.tools.compactMap(\.endedAt).max() ?? 0,
                      stat.tools.map(\.startedAt).max() ?? 0)
        let spans = showAll ? stat.tools : Array(stat.tools.prefix(TurnBillText.visibleTools))
        let hidden = stat.tools.count - spans.count + stat.moreTools
        return ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                ForEach(Array(spans.enumerated()), id: \.offset) { _, span in
                    spanRow(span, end: end)
                }
                if hidden > 0 {
                    Button(showAll || stat.tools.count <= TurnBillText.visibleTools
                           ? "另有 \(hidden) 条没有记录明细" : "还有 \(hidden) 条") { showAll = true }
                        .buttonStyle(.plain)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.accent)
                        .disabled(showAll || stat.tools.count <= TurnBillText.visibleTools)
                        .accessibilityIdentifier("session.turn.bill.more")
                }
            }
        }
        .frame(maxHeight: 320)
    }

    private func spanRow(_ span: SessionTranscriptStats.ToolSpan, end: UInt64) -> some View {
        let bar = TurnBillText.bar(span, turnStart: stat.startedAt, turnEnd: end)
        let speed = TurnBillText.speed(span)
        return HStack(spacing: AeroricSpace.s) {
            Text(span.name)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(width: 96, alignment: .leading)
                .foregroundStyle(speed == .failed ? AeroricColor.danger : AeroricColor.textSecondary)
            GeometryReader { proxy in
                ZStack(alignment: .leading) {
                    Capsule().fill(AeroricColor.layer2)
                    Capsule().fill(color(speed))
                        .frame(width: max(3, proxy.size.width * bar.width))
                        .offset(x: proxy.size.width * bar.start)
                        .opacity(speed == .unfinished ? 0.4 : 1)
                }
            }
            .frame(height: 6)
            Text(TurnBillText.duration(span).map(SessionStatsText.duration) ?? "未返回")
                .monospacedDigit()
                .foregroundStyle(AeroricColor.textTertiary)
                .frame(width: 52, alignment: .trailing)
        }
        .font(.system(size: AeroricType.caption))
        .help([span.detail, span.failed ? "失败" : nil].compactMap { $0 }.joined(separator: " · "))
    }

    private func color(_ speed: TurnBillText.Speed) -> Color {
        switch speed {
        case .fast: AeroricColor.textTertiary
        case .medium: AeroricColor.accent
        case .slow: AeroricColor.warning
        case .failed: AeroricColor.danger
        case .unfinished: AeroricColor.textTertiary
        }
    }

    @ViewBuilder private func row(_ label: String, _ value: String?) -> some View {
        HStack {
            Text(label).foregroundStyle(AeroricColor.textSecondary)
            Spacer(minLength: AeroricSpace.l)
            Text(value ?? "—").monospacedDigit().lineLimit(1)
        }
        .font(.system(size: AeroricType.secondary))
    }
}
