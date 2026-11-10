import AeroricClient
import Foundation
import Observation
import SwiftUI

/// R3-3a (2026-11-10): what a kept change costs, per agent configuration and
/// model — the Agent 配置 page's 改动成本 tab. The host sums each session's
/// turns by what became of their changes (`session.changeCost`); this only
/// words them.
public enum ChangeCostText {
    public typealias Row = SessionsClient.ChangeCostRow
    public typealias Bucket = SessionsClient.ChangeCostBucket

    /// `CPA · gpt-6.1-sol`; `codex（未选配置）· …` for sessions started
    /// without an agent configuration.
    public static func title(_ row: Row) -> String {
        let config = row.config ?? row.configId ?? "\(row.agent)（未选配置）"
        return "\(config) · \(row.model.isEmpty ? "未记模型" : row.model)"
    }

    /// Accepted out of the turns the person decided on: `80% · 4/5 轮`;
    /// nil before any was accepted or undone.
    public static func acceptRate(_ row: Row) -> String? {
        let decided = row.accepted.turns + row.undone.turns
        guard decided > 0 else { return nil }
        let percent = Int((Double(row.accepted.turns) / Double(decided) * 100).rounded())
        return "\(percent)% · \(row.accepted.turns)/\(decided) 轮"
    }

    /// One accepted turn on average: `≈98K tok · ≈$0.120`.
    public static func perAccepted(_ row: Row) -> String? {
        let bucket = row.accepted
        guard bucket.turns > 0 else { return nil }
        var text = "≈\(SessionStatsText.compact(UInt64(bucket.tokens / bucket.turns))) tok"
        if let cost = bucket.costUsd {
            text += " · ≈\(TurnBillText.money(cost / Double(bucket.turns)))"
        }
        return text
    }

    /// A bucket's total: `2 轮 · 190K tok · ≈$0.250`; nil when empty.
    public static func total(_ bucket: Bucket) -> String? {
        guard bucket.turns > 0 else { return nil }
        var text = "\(bucket.turns) 轮 · \(SessionStatsText.compact(UInt64(bucket.tokens))) tok"
        if let cost = bucket.costUsd { text += " · ≈\(TurnBillText.money(cost))" }
        return text
    }

    public static let note = "按回合算：接受的回合是有效花费，撤销的算浪费，没改文件的回合单独列。"
        + "token 含缓存读取；模型不在价目表里时只显示 token。只统计这台 Mac 上的会话。"
}

/// The 改动成本 tab's data: read when the tab opens and when the range
/// changes.
@MainActor @Observable
public final class ChangeCostBook {
    public enum Range: Int, CaseIterable, Sendable {
        case day = 1, week = 7, fortnight = 14, month = 30

        public var label: String {
            switch self {
            case .day: "24 小时"
            case .week: "7 天"
            case .fortnight: "14 天"
            case .month: "30 天"
            }
        }
    }

    public var range: Range = .week
    public private(set) var rows: [SessionsClient.ChangeCostRow] = []
    public private(set) var loading = false
    public private(set) var loaded = false
    public var errorText: String?

    @ObservationIgnored private var client: SessionsClient?

    public init() {}

    public func attach(client: SessionsClient?) {
        self.client = client
    }

    public func refresh() async {
        guard let client else { return }
        loading = true
        defer { loading = false }
        do {
            rows = try await client.changeCost(rangeDays: range.rawValue)
            errorText = nil
        } catch {
            errorText = "没读到改动成本：\(error.localizedDescription)"
        }
        loaded = true
    }
}

struct ChangeCostPane: View {
    @Bindable var book: ChangeCostBook

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                header
                if book.rows.isEmpty {
                    if book.loaded {
                        AeroricEmptyState(systemImage: "chart.bar", title: "这段时间没有可统计的回合",
                                          message: "在这台 Mac 上跑过的会话会出现在这里。")
                            .padding(.top, 60)
                            .accessibilityIdentifier("agent-config.change-cost.empty")
                    }
                } else {
                    ForEach(Array(book.rows.enumerated()), id: \.offset) { index, row in
                        ChangeCostCard(row: row)
                            .accessibilityIdentifier("agent-config.change-cost.row.\(index)")
                    }
                    Text(ChangeCostText.note)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let error = book.errorText {
                    Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .task { await book.refresh() }
        .accessibilityIdentifier("agent-config.change-cost")
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            VStack(alignment: .leading, spacing: 2) {
                Text("改动成本").font(.system(size: AeroricType.headline, weight: .semibold))
                Text("每个配置和模型，留下一次改动花多少、浪费多少")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Spacer()
            if book.loading { ProgressView().controlSize(.small) }
            CapsuleSlider(selection: Binding(
                get: { book.range },
                set: { range in
                    book.range = range
                    Task { await book.refresh() }
                }
            ), groupTitle: "区间", identifier: "agent-config.change-cost.range",
               segments: ChangeCostBook.Range.allCases.map { .init(value: $0, title: $0.label) })
            .fixedSize()
        }
    }
}

struct ChangeCostCard: View {
    let row: SessionsClient.ChangeCostRow

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text(ChangeCostText.title(row))
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: AeroricSpace.m)
                Text(ChangeCostText.acceptRate(row).map { "接受率 \($0)" } ?? "还没有接受或撤销")
                    .font(.system(size: AeroricType.caption).monospacedDigit())
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.vertical, 3)
                    .background(Capsule().fill(AeroricColor.accent.opacity(0.15)))
            }
            // Four equal columns, so the same figure sits in the same place
            // on every card (目检 2026-11-10: sized to content they drifted).
            HStack(alignment: .top, spacing: AeroricSpace.l) {
                item("每个接受回合", ChangeCostText.perAccepted(row))
                item("撤销浪费", ChangeCostText.total(row.undone))
                item("待验收", ChangeCostText.total(row.pending))
                item("没产出改动", ChangeCostText.total(row.noChange))
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.m))
        .accessibilityElement(children: .combine)
    }

    private func item(_ label: String, _ value: String?) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            Text(value ?? "—")
                .font(.system(size: AeroricType.secondary).monospacedDigit())
                .foregroundStyle(value == nil ? AeroricColor.textTertiary : AeroricColor.text)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
