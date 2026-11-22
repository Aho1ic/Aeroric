import AeroricClient
import AeroricProtocol
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

/// C4 (2026-11-19): the gateway's requests in words. Usage nobody reported
/// reads 用量未知, never 0.
public enum GatewayText {
    public typealias Usage = LiveGatewayUsage

    /// `共 120 次 · 失败 3 · 5 次用量未知 · 缓存写入 5 分钟 1.2K / 1 小时 800`.
    public static func summary(_ s: Usage.Summary) -> String {
        var parts = ["共 \(s.totalRequests) 次"]
        if s.failedRequests > 0 { parts.append("失败 \(s.failedRequests)") }
        if s.unknownUsageRequests > 0 { parts.append("\(s.unknownUsageRequests) 次用量未知") }
        parts.append("输入 \(SessionStatsText.compact(s.inputTokens)) · 输出 \(SessionStatsText.compact(s.outputTokens))")
        if s.cacheWrite5mTokens > 0 || s.cacheWrite1hTokens > 0 {
            parts.append("缓存写入 5 分钟 \(SessionStatsText.compact(s.cacheWrite5mTokens))"
                + " / 1 小时 \(SessionStatsText.compact(s.cacheWrite1hTokens))")
        }
        return parts.joined(separator: " · ")
    }

    /// E7: a purpose's name.
    public static func purposeName(_ purpose: String) -> String {
        switch purpose {
        case "answer": "回答"
        case "review": "审查"
        case "delegation": "委派"
        case "discussion": "讨论"
        case "rerun": "重跑"
        case "schedule": "定时任务"
        case "unattributed": "不属于任何会话"
        case "memory": "记忆整理"
        case "wiki": "Wiki 模块说明"
        default: "未记录用途"
        }
    }

    /// E7: `审查 · 12 次 · 输入 30K · 输出 4K · 失败的尝试 3 次（输入 9K · 输出 0）`;
    /// the failed tries say what they cost apart from the answering ones.
    public static func purpose(_ p: Usage.Purpose) -> String {
        var parts = [purposeName(p.purpose) + (p.direct ? "（直连分析模型）" : ""), "\(p.requests) 次"]
        if p.failedRequests > 0 { parts.append("失败 \(p.failedRequests)") }
        parts.append("输入 \(SessionStatsText.compact(p.inputTokens)) · 输出 \(SessionStatsText.compact(p.outputTokens))")
        if p.unknownUsageRequests > 0 { parts.append("\(p.unknownUsageRequests) 次用量未知") }
        if p.failedAttempts > 0 {
            parts.append("失败的尝试 \(p.failedAttempts) 次（输入 \(SessionStatsText.compact(p.failedAttemptInputTokens))"
                + " · 输出 \(SessionStatsText.compact(p.failedAttemptOutputTokens))）")
        }
        return parts.joined(separator: " · ")
    }

    /// `12.3K 入 · 2K 出`, or `用量未知`.
    public static func usage(source: String, input: UInt64, output: UInt64) -> String {
        switch source {
        case "unknown": "用量未知"
        case "estimated": "约 \(SessionStatsText.compact(input)) 入 · \(SessionStatsText.compact(output)) 出"
        default: "\(SessionStatsText.compact(input)) 入 · \(SessionStatsText.compact(output)) 出"
        }
    }

    /// One request's line: model, where it ended, status, usage, tries.
    public static func request(_ r: Usage.Request) -> String {
        var parts = [r.model.isEmpty ? "未知模型" : r.model]
        if let target = r.targetName, !target.isEmpty { parts.append(target) }
        parts.append(r.success ? "成功" : "失败 \(r.statusCode)")
        parts.append(usage(source: r.usageSource, input: r.inputTokens, output: r.outputTokens))
        let tried = r.attempts.filter { !$0.skipped }.count
        if tried > 1 { parts.append("试了 \(tried) 次") }
        return parts.joined(separator: " · ")
    }

    /// `第 1 次 · 连接 A · 429 · 0.4 秒 · 用量未知 · slow down`.
    public static func attempt(_ a: Usage.Attempt) -> String {
        var parts = ["第 \(a.seq) 次", a.targetName ?? "未知连接"]
        if a.skipped {
            parts.append("熔断中，跳过")
            return parts.joined(separator: " · ")
        }
        parts.append(a.statusCode.map(String.init) ?? "没有回应")
        parts.append(String(format: "%.1f 秒", Double(a.latencyMs) / 1000))
        parts.append(usage(source: a.usageSource, input: a.inputTokens, output: a.outputTokens))
        if let error = a.errorSummary, !error.isEmpty { parts.append(error) }
        return parts.joined(separator: " · ")
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
    /// C4: the gateway's latest requests.
    public private(set) var gateway: LiveGatewayUsage?
    public private(set) var gatewayError: String?

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

    /// C4: read when the tab opens.
    public func refreshGateway() async {
        guard let client else { return }
        do {
            gateway = try await client.gatewayRequests(limit: 30)
            gatewayError = nil
        } catch {
            gatewayError = "没读到网关请求：\(error.localizedDescription)"
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
    @Environment(BudgetBook.self) private var budget: BudgetBook?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                if let budget { BudgetSection(book: budget) }
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
                GatewayRequestsSection(usage: book.gateway, error: book.gatewayError)
                    .padding(.top, AeroricSpace.m)
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
            async let gateway: Void = book.refreshGateway()
            _ = await (quota, cost, gateway)
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

/// C4: the gateway's latest requests; one opens to show each try.
struct GatewayRequestsSection: View {
    let usage: LiveGatewayUsage?
    let error: String?
    @State private var open: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            VStack(alignment: .leading, spacing: 2) {
                Text("网关请求").font(.system(size: AeroricType.headline, weight: .semibold))
                Text("经 Aeroric 网关的请求，每次尝试去了哪里、各花了多少；没报用量的写“用量未知”，不算进合计")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error {
                Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
            }
            if let usage {
                Text(GatewayText.summary(usage.summary))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(usage.summary.unknownUsageRequests > 0 ? AeroricColor.warning : AeroricColor.textSecondary)
                    .monospacedDigit()
                    .accessibilityIdentifier("agent-config.gateway.summary")
                if !usage.purposes.isEmpty { purposes(usage.purposes) }
                if usage.requests.isEmpty {
                    Text("还没有经过网关的请求。用 Agent 配置启动的会话会出现在这里。")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                ForEach(usage.requests) { request in row(request) }
            }
        }
        .accessibilityIdentifier("agent-config.gateway")
    }

    /// E7: the last 30 days by purpose; a purpose with failed tries is in
    /// the warning colour, so what failed is not hidden by what answered.
    private func purposes(_ rows: [LiveGatewayUsage.Purpose]) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("近 30 天按用途").font(.system(size: AeroricType.caption, weight: .semibold))
                .foregroundStyle(AeroricColor.textSecondary)
            ForEach(rows) { row in
                Text(GatewayText.purpose(row))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(row.failedAttempts > 0 ? AeroricColor.warning : AeroricColor.textSecondary)
                    .monospacedDigit()
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .accessibilityIdentifier("agent-config.gateway.purposes")
    }

    @ViewBuilder private func row(_ request: LiveGatewayUsage.Request) -> some View {
        let canOpen = request.attempts.count > 1
        Button {
            if open.remove(request.id) == nil { open.insert(request.id) }
        } label: {
            HStack(spacing: AeroricSpace.s) {
                Text(GatewayText.request(request))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(request.success ? AeroricColor.textSecondary : AeroricColor.danger)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: AeroricSpace.s)
                if canOpen {
                    Image(systemName: open.contains(request.id) ? "chevron.down" : "chevron.right")
                        .font(.system(size: AeroricType.caption - 2))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!canOpen)
        .monospacedDigit()
        .help(request.errorSummary ?? "")
        if canOpen, open.contains(request.id) {
            ForEach(request.attempts) { attempt in
                Text(GatewayText.attempt(attempt))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .monospacedDigit()
                    .lineLimit(2)
                    .padding(.leading, AeroricSpace.l)
            }
        }
    }
}
