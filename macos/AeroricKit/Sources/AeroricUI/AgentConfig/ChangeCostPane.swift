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

/// R3-3b (2026-11-10): the Codex subscription's windows and whether they
/// last at the current pace (`usage.quotaPace`).
public enum QuotaText {
    public typealias Pace = MaintenanceClient.QuotaWindowPace

    /// `5 小时`, `每周`.
    public static func windowLabel(_ pace: Pace) -> String {
        switch pace.windowMinutes {
        case 7 * 24 * 60: "每周"
        case let m where m % (24 * 60) == 0: "\(m / (24 * 60)) 天"
        case let m where m % 60 == 0: "\(m / 60) 小时"
        case let m: "\(m) 分钟"
        }
    }

    /// `已用 16% · 窗口过了 58% · 12:49 重置`.
    public static func usage(_ pace: Pace, now: Date = Date(), calendar: Calendar = .current) -> String {
        "已用 \(pace.usedPercent)% · 窗口过了 \(pace.elapsedPercent)% · "
            + "\(time(pace.resetAt, now: now, calendar: calendar)) 重置"
    }

    /// `偏快`, `正常`, `读数不够`.
    public static func verdict(_ pace: Pace) -> String {
        switch pace.verdict {
        case "fast": "偏快"
        case "ok": "正常"
        default: "读数不够"
        }
    }

    public static func detail(_ pace: Pace, now: Date = Date(), calendar: Calendar = .current) -> String {
        switch pace.verdict {
        case "fast":
            if pace.usedPercent >= 100 {
                return "已用完，\(time(pace.resetAt, now: now, calendar: calendar)) 重置"
            }
            guard let at = pace.exhaustAt else { return "按现在的速度会在重置前用完" }
            return "按现在的速度约 \(time(at, now: now, calendar: calendar)) 用完，"
                + "比重置早 \(span(seconds: pace.resetAt - at))"
        case "ok":
            return "按现在的速度，重置时约剩 \(pace.leftAtReset ?? 0)%"
        default:
            return "窗口刚开始，过一会儿再看"
        }
    }

    public static func basis(_ pace: Pace) -> String {
        pace.recent ? "按最近一小时的速度推算" : "按这个窗口的平均速度推算"
    }

    /// `12:49` today, `11月12日 16:11` otherwise.
    static func time(_ seconds: Int, now: Date, calendar: Calendar) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        let formatter = DateFormatter()
        formatter.calendar = calendar
        formatter.timeZone = calendar.timeZone
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = calendar.isDate(date, inSameDayAs: now) ? "HH:mm" : "M月d日 HH:mm"
        return formatter.string(from: date)
    }

    /// `1 天 3 小时`, `2 小时 5 分钟`, `40 分钟`.
    static func span(seconds: Int) -> String {
        let minutes = max(0, seconds) / 60
        let (days, hours, mins) = (minutes / 1440, minutes % 1440 / 60, minutes % 60)
        if days > 0 { return hours > 0 ? "\(days) 天 \(hours) 小时" : "\(days) 天" }
        if hours > 0 { return mins > 0 ? "\(hours) 小时 \(mins) 分钟" : "\(hours) 小时" }
        return "\(max(1, mins)) 分钟"
    }
}

/// The 用量与成本 tab's data: the quota is read when the tab opens, the
/// change cost also when the range changes.
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
    public private(set) var quota: MaintenanceClient.QuotaPace?
    public private(set) var quotaError: String?

    @ObservationIgnored private var client: SessionsClient?
    @ObservationIgnored private var maintenance: MaintenanceClient?

    public init() {}

    public func attach(client: SessionsClient?, maintenance: MaintenanceClient? = nil) {
        self.client = client
        self.maintenance = maintenance
    }

    public func refreshQuota() async {
        guard let maintenance else { return }
        do {
            quota = try await maintenance.quotaPace()
            quotaError = nil
        } catch {
            quotaError = "没读到订阅额度：\(error.localizedDescription)"
        }
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
                QuotaCard(quota: book.quota, error: book.quotaError)
                header
                    .padding(.top, AeroricSpace.m)
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
        .task {
            async let quota: Void = book.refreshQuota()
            async let cost: Void = book.refresh()
            _ = await (quota, cost)
        }
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

/// The Codex subscription card at the top of 用量与成本.
struct QuotaCard: View {
    let quota: MaintenanceClient.QuotaPace?
    let error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text("Codex 订阅额度").font(.system(size: AeroricType.headline, weight: .semibold))
                Text("本机 Codex 登录\(quota?.planType.map { "（\($0)）" } ?? "")")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                Spacer()
            }
            if let error {
                Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
            } else if let quota, quota.status != "available" {
                Text("读不到：\(quota.reason ?? "未知原因")")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("agent-config.quota.unavailable")
            } else if let windows = quota?.windows, !windows.isEmpty {
                ForEach(windows, id: \.window) { pace in
                    row(pace)
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("agent-config.quota.\(pace.window)")
                }
                Text("只提示，不会替你切换账号或模型。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            } else if quota != nil {
                Text("订阅接口没给出窗口时长，算不出节奏。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.m))
        .accessibilityIdentifier("agent-config.quota")
    }

    private func row(_ pace: MaintenanceClient.QuotaWindowPace) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: AeroricSpace.m) {
                Text(QuotaText.windowLabel(pace))
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .frame(width: 56, alignment: .leading)
                Text(QuotaText.usage(pace))
                    .font(.system(size: AeroricType.secondary).monospacedDigit())
                Spacer(minLength: AeroricSpace.m)
                Text(QuotaText.verdict(pace))
                    .font(.system(size: AeroricType.caption))
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.vertical, 3)
                    .background(Capsule().fill(
                        (pace.verdict == "fast" ? AeroricColor.warning : AeroricColor.accent).opacity(0.18)))
            }
            Text("\(QuotaText.detail(pace))；\(QuotaText.basis(pace))")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(pace.verdict == "fast" ? AeroricColor.warning : AeroricColor.textSecondary)
                .padding(.leading, 56 + AeroricSpace.m)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
