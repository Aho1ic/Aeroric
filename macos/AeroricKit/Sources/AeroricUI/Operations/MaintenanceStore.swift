import Foundation

import AeroricClient
import AeroricProtocol

/// P8-04 刀四：维护面的视图状态（`AeroricUI/Operations` 七视图的唯一数据源）。
///
/// 分工与其它面板一致：**判定在 core**（清理策略、过期候选、周报区间、通知过期、
/// 更新可装性），这里只做三件事——缓存宿主应答、把用户动作转发成调用、记住错误。
/// 于是本类型里没有任何"再算一遍"的规则：`expiredTaskIds`/`shouldRunCleanup`
/// 那套逻辑在 `core/tasks/cleanup.rs` 与 `core/maintenance/cleanup.rs` 各一份，
/// Swift 侧一份也不抄。
@MainActor
public final class MaintenanceStore: ObservableObject {

    // MARK: - 状态

    public enum Range: Int, CaseIterable, Identifiable, Sendable {
        case day = 1
        case week = 7
        case fortnight = 14
        case month = 30

        public var id: Int { rawValue }

        /// 台账的 `range_days` 白名单就是这个集合（core 侧拒绝其它值）。
        public var label: String {
            switch self {
            case .day: "24 小时"
            case .week: "7 天"
            case .fortnight: "14 天"
            case .month: "30 天"
            }
        }
    }

    /// 用量面板的筛选族。`all` 之外的四个字面量与 core `UsageAgent` 的
    /// `agent` 参数逐字一致。
    public enum AgentFilter: String, CaseIterable, Identifiable, Sendable {
        case all, codex, claude, dsh, omp
        public var id: String { rawValue }
    }

    @Published public private(set) var statistics: MaintenanceClient.UsageStatistics = .empty
    @Published public private(set) var range: Range = .week
    @Published public private(set) var agentFilter: AgentFilter = .all
    @Published public private(set) var subscription: MaintenanceClient.SubscriptionUsage?
    @Published public private(set) var agentUsage: MaintenanceClient.AgentUsageSnapshot = .empty
    @Published public private(set) var feed: MaintenanceClient.NotificationFeed = .empty
    @Published public private(set) var degradations: [MaintenanceClient.StartupDegradation] = []
    @Published public private(set) var pendingUpdate: MaintenanceClient.PendingReleaseUpdate?
    @Published public private(set) var lastError: String?

    /// 刷新是否在进行（视图据此禁用按钮，不据"上一次成功"猜）。
    @Published public private(set) var isRefreshing = false

    /// 连接。为 nil 时所有动作是 no-op 并留一条错误——不假装成功。
    private let client: MaintenanceClient?

    public init(client: MaintenanceClient?) {
        self.client = client
    }

    // MARK: - 拉取

    public func loadStatistics() async {
        guard let client else { return mark("未连接宿主") }
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            statistics = try await client.readUsageStatistics(
                rangeDays: range.rawValue,
                agent: agentFilter.rawValue
            )
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func select(range: Range) async {
        self.range = range
        await loadStatistics()
    }

    public func select(agent: AgentFilter) async {
        self.agentFilter = agent
        await loadStatistics()
    }

    /// 显式刷新：绕过节流重建索引，然后回读统计（**先建索引再读**——反过来会
    /// 拿到上一轮的数字）。
    public func refreshIndex() async {
        guard let client else { return mark("未连接宿主") }
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            _ = try await client.refreshUsageStatisticsIndex()
            statistics = try await client.readUsageStatistics(
                rangeDays: range.rawValue,
                agent: agentFilter.rawValue
            )
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func loadSubscription() async {
        guard let client else { return mark("未连接宿主") }
        do {
            subscription = try await client.readSubscriptionUsage()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func loadAgentUsage() async {
        guard let client else { return mark("未连接宿主") }
        do {
            agentUsage = try await client.loadAgentUsageSnapshot()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func loadNotifications(force: Bool) async {
        guard let client else { return mark("未连接宿主") }
        do {
            feed = try await client.notifications(force: force)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func loadDegradations() async {
        guard let client else { return mark("未连接宿主") }
        do {
            degradations = try await client.listStartupDegradations()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func loadPendingUpdate() async {
        guard let client else { return mark("未连接宿主") }
        do {
            pendingUpdate = try await client.pendingReleaseUpdate(tagName: nil)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    // MARK: - 动作

    public func markRead(_ id: String) async {
        guard let client else { return mark("未连接宿主") }
        do {
            try await client.markNotificationRead(id: id)
            // 回读而不是本地改标记：未读数由宿主算（已读集合按远端存活对账）。
            feed = try await client.notifications(force: false)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func markAllRead() async {
        guard let client else { return mark("未连接宿主") }
        do {
            try await client.markAllNotificationsRead()
            feed = try await client.notifications(force: false)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 准备更新。**只下载与校验**；替换运行中的 app 由 GUI 适配器在用户二次确认后做
    /// （计划 Action 5 把两件事分开）。
    public func prepareUpdate(tagName: String) async {
        guard let client else { return mark("未连接宿主") }
        do {
            pendingUpdate = try await client.prepareReleaseUpdate(tagName: tagName)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 生成周报；返回写盘路径（nil 表示失败，错误在 `lastError`）。
    public func generateWeeklyReport(
        window: WeekWindow,
        locale: String,
        outputPath: String,
        tasks: [WeeklyReportTaskPayload]
    ) async -> String? {
        guard let client else {
            mark("未连接宿主")
            return nil
        }
        do {
            let path = try await client.generateWeeklyReport(
                request: WeeklyReportRequestPayload(
                    fromMs: window.fromMs,
                    toMs: window.toMs,
                    weekLabel: window.label,
                    locale: locale,
                    outputPath: outputPath,
                    tasks: tasks
                ).jsonValue
            )
            lastError = nil
            return path
        } catch {
            lastError = String(describing: error)
            return nil
        }
    }

    /// 记一次配置使用（首页选择器排序依据；宿主落盘）。
    public func recordAgentUsage(_ agentID: String) async {
        guard let client else { return mark("未连接宿主") }
        do {
            agentUsage = try await client.recordAgentConfigUsage(agent: agentID)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func clearError() {
        lastError = nil
    }

    private func mark(_ message: String) {
        lastError = message
    }
}

// MARK: - 周报区间（纯函数，与 core 的 `week_window` 同语义）

/// 本地日历的周区间。`fromMs` 是起始日 00:00，`toMs` 是结束日 23:59:59.999。
///
/// 与 core `maintenance::reports::week_window` 同一算法——**但两侧都只算不判**：
/// Swift 算出来是为了给用户看"这份周报覆盖哪几天"，真正的筛选与标题由宿主在
/// 生成时按注入的日历再算一次（ADR-010 的本地日历缝）。
public struct WeekWindow: Sendable, Equatable {
    public var fromMs: Int
    public var toMs: Int
    /// `YYYY-Www`（由区间中点算 ISO 周；周日起始时避免标题早一周）。
    public var label: String

    public init(fromMs: Int, toMs: Int, label: String) {
        self.fromMs = fromMs
        self.toMs = toMs
        self.label = label
    }

    /// 包含 `date` 的那个自定义周。`weekStartDay`/`weekEndDay` 用 0=周日 .. 6=周六。
    public static func containing(
        _ date: Date,
        weekStartDay: Int = 1,
        weekEndDay: Int = 0,
        calendar: Calendar = .current
    ) -> WeekWindow {
        let span = ((weekEndDay - weekStartDay + 7) % 7) + 1
        let startOfDay = calendar.startOfDay(for: date)
        let weekday = calendar.component(.weekday, from: startOfDay) - 1 // 1=周日 → 0
        let daysSinceStart = (weekday - weekStartDay + 7) % 7
        let from = calendar.date(byAdding: .day, value: -daysSinceStart, to: startOfDay) ?? startOfDay
        let next = calendar.date(byAdding: .day, value: span, to: from) ?? from
        let to = next.addingTimeInterval(-0.001)
        let midpoint = from.addingTimeInterval(to.timeIntervalSince(from) / 2)
        return WeekWindow(
            fromMs: Int(from.timeIntervalSince1970 * 1000),
            toMs: Int(to.timeIntervalSince1970 * 1000),
            label: isoWeekLabel(midpoint, calendar: calendar)
        )
    }

    /// `YYYY-Www`（ISO-8601：周一为首、周四定周）。
    static func isoWeekLabel(_ date: Date, calendar: Calendar = .current) -> String {
        var iso = Calendar(identifier: .iso8601)
        iso.timeZone = calendar.timeZone
        let week = iso.component(.weekOfYear, from: date)
        let year = iso.component(.yearForWeekOfYear, from: date)
        return String(format: "%04d-W%02d", year, week)
    }

    /// 区间天数（含两端）。
    public var days: Int {
        max(0, (toMs - fromMs + 1) / 86_400_000)
    }
}

// MARK: - 周报请求载荷（与 core `WeeklyReportRequest` 逐字段同名）

/// 一条任务的报告输入。字段名与宿主 serde 的 camelCase 输出一致；**不在这里判**——
/// 缺 `sessionPath` 时宿主把这条降级成标题行，Swift 不预判"能不能摘要"。
public struct WeeklyReportTaskPayload: Sendable, Equatable {
    public var projectName: String
    public var projectPath: String
    public var title: String
    public var agent: String
    public var status: String
    public var createdAt: Int
    public var completedAt: Int?
    public var failureReason: String?
    public var sessionPath: String?
    public var sessionFamily: String?
    public var additions: Int?
    public var deletions: Int?

    public init(
        projectName: String, projectPath: String, title: String, agent: String, status: String,
        createdAt: Int, completedAt: Int? = nil, failureReason: String? = nil,
        sessionPath: String? = nil, sessionFamily: String? = nil,
        additions: Int? = nil, deletions: Int? = nil
    ) {
        self.projectName = projectName
        self.projectPath = projectPath
        self.title = title
        self.agent = agent
        self.status = status
        self.createdAt = createdAt
        self.completedAt = completedAt
        self.failureReason = failureReason
        self.sessionPath = sessionPath
        self.sessionFamily = sessionFamily
        self.additions = additions
        self.deletions = deletions
    }

    var jsonValue: JSONValue {
        var fields: [String: JSONValue] = [
            "projectName": .string(projectName),
            "projectPath": .string(projectPath),
            "title": .string(title),
            "agent": .string(agent),
            "status": .string(status),
            "createdAt": .number(String(createdAt)),
        ]
        if let completedAt { fields["completedAt"] = .number(String(completedAt)) }
        if let failureReason { fields["failureReason"] = .string(failureReason) }
        if let sessionPath { fields["sessionPath"] = .string(sessionPath) }
        if let sessionFamily { fields["sessionFamily"] = .string(sessionFamily) }
        if let additions { fields["additions"] = .number(String(additions)) }
        if let deletions { fields["deletions"] = .number(String(deletions)) }
        return .object(fields)
    }
}

public struct WeeklyReportRequestPayload: Sendable, Equatable {
    public var fromMs: Int
    public var toMs: Int
    public var weekLabel: String
    public var locale: String
    public var outputPath: String
    public var tasks: [WeeklyReportTaskPayload]

    public init(
        fromMs: Int, toMs: Int, weekLabel: String, locale: String, outputPath: String,
        tasks: [WeeklyReportTaskPayload]
    ) {
        self.fromMs = fromMs
        self.toMs = toMs
        self.weekLabel = weekLabel
        self.locale = locale
        self.outputPath = outputPath
        self.tasks = tasks
    }

    var jsonValue: JSONValue {
        .object([
            "fromMs": .number(String(fromMs)),
            "toMs": .number(String(toMs)),
            "weekLabel": .string(weekLabel),
            "locale": .string(locale),
            "outputPath": .string(outputPath),
            "tasks": .array(tasks.map(\.jsonValue)),
        ])
    }
}
