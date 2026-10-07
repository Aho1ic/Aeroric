import AeroricProtocol
import Foundation

/// P8-04 刀四：维护面与用量族的类型化客户端（宿主 `usage.*` / `maintenance.*` /
/// `diagnostics.*`）。wire 解析只在 AeroricClient；视图经 `MaintenanceStore` 消费。
///
/// 这一层的形状由两条既有约定决定：
/// - **字段名与 core 的 serde 输出逐字对齐**（`SessionMetrics` 是 snake_case——
///   它是 P2 起就上线的形状，不改；其余 `camelCase`）。名字写错不会编译报错，
///   只会在运行时静默读成默认值，所以每个解析函数都有对应用例。
/// - **缺失即降级，不抛**：通知/用量这些面在宿主能力不全时返回空表而不是错误，
///   只有信封形状不符才 `malformedReply`。
public enum MaintenanceClientError: Error, Sendable, Equatable {
    case malformedReply(String)
}

public struct MaintenanceClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - Shapes：用量统计

    /// `usage.read_usage_statistics` 的应答（core `metrics::UsageStatistics`）。
    public struct UsageTotals: Sendable, Equatable, Decodable {
        public var totalTokens: Int
        public var inputTokens: Int
        public var outputTokens: Int
        public var cacheCreationTokens: Int
        public var cacheReadTokens: Int
        public var cacheHitRate: Double
        public var requestCount: Int
        public var totalCost: Double
        public var pricedRequestCount: Int
        public var estimatedRequestCount: Int

        public static let zero = UsageTotals(
            totalTokens: 0, inputTokens: 0, outputTokens: 0, cacheCreationTokens: 0,
            cacheReadTokens: 0, cacheHitRate: 0, requestCount: 0, totalCost: 0,
            pricedRequestCount: 0, estimatedRequestCount: 0
        )

        public init(
            totalTokens: Int, inputTokens: Int, outputTokens: Int, cacheCreationTokens: Int,
            cacheReadTokens: Int, cacheHitRate: Double, requestCount: Int, totalCost: Double,
            pricedRequestCount: Int, estimatedRequestCount: Int
        ) {
            self.totalTokens = totalTokens
            self.inputTokens = inputTokens
            self.outputTokens = outputTokens
            self.cacheCreationTokens = cacheCreationTokens
            self.cacheReadTokens = cacheReadTokens
            self.cacheHitRate = cacheHitRate
            self.requestCount = requestCount
            self.totalCost = totalCost
            self.pricedRequestCount = pricedRequestCount
            self.estimatedRequestCount = estimatedRequestCount
        }

        /// 缺键按 0 读。统计面是**派生数据**：宿主升级期间少一个键，面板该少显示一个
        /// 数字而不是整块失败（这一条有对应反例用例）。
        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            func int(_ key: CodingKeys) -> Int {
                (try? container.decodeIfPresent(Int.self, forKey: key)) .flatMap { $0 } ?? 0
            }
            func double(_ key: CodingKeys) -> Double {
                (try? container.decodeIfPresent(Double.self, forKey: key)).flatMap { $0 } ?? 0
            }
            self.init(
                totalTokens: int(.totalTokens), inputTokens: int(.inputTokens),
                outputTokens: int(.outputTokens), cacheCreationTokens: int(.cacheCreationTokens),
                cacheReadTokens: int(.cacheReadTokens), cacheHitRate: double(.cacheHitRate),
                requestCount: int(.requestCount), totalCost: double(.totalCost),
                pricedRequestCount: int(.pricedRequestCount),
                estimatedRequestCount: int(.estimatedRequestCount)
            )
        }

        private enum CodingKeys: String, CodingKey {
            case totalTokens, inputTokens, outputTokens, cacheCreationTokens, cacheReadTokens
            case cacheHitRate, requestCount, totalCost, pricedRequestCount, estimatedRequestCount
        }
    }

    public struct UsageDay: Sendable, Equatable, Decodable {
        public var date: String
        /// 24 小时档才有；按天档为 nil。**不显示假 0**——0 点桶与"没有桶"是两回事。
        public var hour: Int?
        public var totalTokens: Int
        public var requestCount: Int
        public var totalCost: Double

        private enum CodingKeys: String, CodingKey {
            case date, hour, totalTokens, requestCount, totalCost
        }

        public init(
            date: String, hour: Int?, totalTokens: Int, requestCount: Int, totalCost: Double
        ) {
            self.date = date
            self.hour = hour
            self.totalTokens = totalTokens
            self.requestCount = requestCount
            self.totalCost = totalCost
        }

        /// `hour` 缺失就是 nil（按天档），不是 0 点；其余缺键按 0。
        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.init(
                date: (try? container.decodeIfPresent(String.self, forKey: .date)) .flatMap { $0 } ?? "",
                hour: try? container.decodeIfPresent(Int.self, forKey: .hour),
                totalTokens: (try? container.decodeIfPresent(Int.self, forKey: .totalTokens)).flatMap { $0 } ?? 0,
                requestCount: (try? container.decodeIfPresent(Int.self, forKey: .requestCount)).flatMap { $0 } ?? 0,
                totalCost: (try? container.decodeIfPresent(Double.self, forKey: .totalCost)).flatMap { $0 } ?? 0
            )
        }
    }

    public struct UsageBreakdown: Sendable, Equatable, Decodable {
        public var codex: UsageTotals
        public var claude: UsageTotals
        public var dsh: UsageTotals
        public var omp: UsageTotals

        public init(codex: UsageTotals, claude: UsageTotals, dsh: UsageTotals, omp: UsageTotals) {
            self.codex = codex
            self.claude = claude
            self.dsh = dsh
            self.omp = omp
        }

        private enum CodingKeys: String, CodingKey {
            case codex, claude, dsh, omp
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            func totals(_ key: CodingKeys) -> UsageTotals {
                (try? container.decodeIfPresent(UsageTotals.self, forKey: key)).flatMap { $0 } ?? .zero
            }
            self.init(codex: totals(.codex), claude: totals(.claude), dsh: totals(.dsh), omp: totals(.omp))
        }
    }

    public struct UsageStatistics: Sendable, Equatable, Decodable {
        public var rangeDays: Int
        public var from: String
        public var to: String
        public var agent: String
        public var updatedAt: Int
        public var totals: UsageTotals
        public var series: [UsageDay]
        public var breakdown: UsageBreakdown

        /// 空统计（连接未建立或宿主未答时显示"无数据"，不显示 0 成本）。
        public static let empty = UsageStatistics(
            rangeDays: 7, from: "", to: "", agent: "all", updatedAt: 0,
            totals: .zero, series: [],
            breakdown: UsageBreakdown(codex: .zero, claude: .zero, dsh: .zero, omp: .zero)
        )

        public var hasData: Bool { totals.requestCount > 0 }

        public init(
            rangeDays: Int, from: String, to: String, agent: String, updatedAt: Int,
            totals: UsageTotals, series: [UsageDay], breakdown: UsageBreakdown
        ) {
            self.rangeDays = rangeDays
            self.from = from
            self.to = to
            self.agent = agent
            self.updatedAt = updatedAt
            self.totals = totals
            self.series = series
            self.breakdown = breakdown
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.init(
                rangeDays: (try? container.decodeIfPresent(Int.self, forKey: .rangeDays)).flatMap { $0 } ?? 7,
                from: (try? container.decodeIfPresent(String.self, forKey: .from)).flatMap { $0 } ?? "",
                to: (try? container.decodeIfPresent(String.self, forKey: .to)).flatMap { $0 } ?? "",
                agent: (try? container.decodeIfPresent(String.self, forKey: .agent)).flatMap { $0 } ?? "all",
                updatedAt: (try? container.decodeIfPresent(Int.self, forKey: .updatedAt)).flatMap { $0 } ?? 0,
                totals: (try? container.decodeIfPresent(UsageTotals.self, forKey: .totals)).flatMap { $0 } ?? .zero,
                series: (try? container.decodeIfPresent([UsageDay].self, forKey: .series)).flatMap { $0 } ?? [],
                breakdown: (try? container.decodeIfPresent(UsageBreakdown.self, forKey: .breakdown)).flatMap { $0 }
                    ?? UsageBreakdown(codex: .zero, claude: .zero, dsh: .zero, omp: .zero)
            )
        }

        private enum CodingKeys: String, CodingKey {
            case rangeDays, from, to, agent, updatedAt, totals, series, breakdown
        }
    }

    /// `usage.read_session_metrics` 的应答。**snake_case**：core 的 `SessionMetrics`
    /// 从 P2 起就是这个形状（RunningView 的既有消费面），这里不擅自改名。
    public struct SessionMetrics: Sendable, Equatable, Decodable {
        public var toolCalls: Int
        public var durationSecs: Double
        public var totalTokens: Int
        public var contextTokens: Int
        /// 模型窗口大小；0 表示这个会话族不暴露（Claude），UI 据此隐藏占比而不是编造。
        public var contextWindow: Int

        private enum CodingKeys: String, CodingKey {
            case toolCalls = "tool_calls"
            case durationSecs = "duration_secs"
            case totalTokens = "total_tokens"
            case contextTokens = "context_tokens"
            case contextWindow = "context_window"
        }

        /// 上下文占比；窗口为 0 时为 nil（不编造百分比）。
        public var contextUsage: Double? {
            guard contextWindow > 0 else { return nil }
            return min(1, Double(contextTokens) / Double(contextWindow))
        }
    }

    /// `usage.load_agent_usage_snapshot` / `record_agent_config_usage` 的应答。
    public struct AgentUsageStats: Sendable, Equatable, Decodable {
        public var recentCount: Int
        public var totalCount: Int
        public var lastUsedAt: Int

        public init(recentCount: Int, totalCount: Int, lastUsedAt: Int) {
            self.recentCount = recentCount
            self.totalCount = totalCount
            self.lastUsedAt = lastUsedAt
        }

        private enum CodingKeys: String, CodingKey {
            case recentCount, totalCount, lastUsedAt
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.init(
                recentCount: (try? container.decodeIfPresent(Int.self, forKey: .recentCount)).flatMap { $0 } ?? 0,
                totalCount: (try? container.decodeIfPresent(Int.self, forKey: .totalCount)).flatMap { $0 } ?? 0,
                lastUsedAt: (try? container.decodeIfPresent(Int.self, forKey: .lastUsedAt)).flatMap { $0 } ?? 0
            )
        }
    }

    public struct AgentUsageSnapshot: Sendable, Equatable, Decodable {
        public var agents: [String: AgentUsageStats]
        public var windowDays: Int
        public var computedAt: Int

        public static let empty = AgentUsageSnapshot(agents: [:], windowDays: 7, computedAt: 0)

        public init(agents: [String: AgentUsageStats], windowDays: Int, computedAt: Int) {
            self.agents = agents
            self.windowDays = windowDays
            self.computedAt = computedAt
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.init(
                agents: (try? container.decodeIfPresent([String: AgentUsageStats].self, forKey: .agents))
                    .flatMap { $0 } ?? [:],
                windowDays: (try? container.decodeIfPresent(Int.self, forKey: .windowDays)).flatMap { $0 } ?? 7,
                computedAt: (try? container.decodeIfPresent(Int.self, forKey: .computedAt)).flatMap { $0 } ?? 0
            )
        }

        private enum CodingKeys: String, CodingKey {
            case agents, windowDays, computedAt
        }
    }

    /// `usage.read_usage_snapshot` 的应答（订阅额度）。
    public struct UsageWindow: Sendable, Equatable, Decodable {
        public var usedPercent: Int
        public var remainingPercent: Int
        public var resetAt: Int?
    }

    public struct ClaudeUsage: Sendable, Equatable, Decodable {
        public var fiveHour: UsageWindow?
        public var sevenDay: UsageWindow?
    }

    public struct CodexUsage: Sendable, Equatable, Decodable {
        public var email: String?
        public var planType: String?
        public var primary: UsageWindow?
        public var secondary: UsageWindow?
    }

    /// `UsageSource<T>` 是 serde 的 internally-tagged enum：
    /// `{"status":"available","data":{…}}` / `{"status":"unavailable","reason":"…"}`。
    public enum UsageSource<Available: Sendable & Decodable & Equatable>: Sendable, Equatable {
        case available(Available)
        case unavailable(String)

        private enum CodingKeys: String, CodingKey {
            case status, data, reason
        }
    }

    public struct SubscriptionUsage: Sendable, Equatable {
        public var claude: UsageSource<ClaudeUsage>
        public var codex: UsageSource<CodexUsage>
        public var fetchedAt: Int
    }

    // MARK: - Shapes：通知与更新

    public struct NotificationItem: Sendable, Equatable, Decodable {
        public var id: String
        public var level: String
        public var title: String
        public var body: String
        public var bodyZh: String?
        public var url: String?
        public var createdAt: String
        public var isRead: Bool
        public var releaseTag: String?
        public var newerThanCurrent: Bool
        public var updateInstallSupported: Bool
    }

    public struct NotificationFeed: Sendable, Equatable, Decodable {
        public var notifications: [NotificationItem]
        public var unreadCount: Int

        public static let empty = NotificationFeed(notifications: [], unreadCount: 0)

        public init(notifications: [NotificationItem], unreadCount: Int) {
            self.notifications = notifications
            self.unreadCount = unreadCount
        }

        public init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            self.init(
                notifications: (try? container.decodeIfPresent([NotificationItem].self, forKey: .notifications))
                    .flatMap { $0 } ?? [],
                unreadCount: (try? container.decodeIfPresent(Int.self, forKey: .unreadCount)).flatMap { $0 } ?? 0
            )
        }

        private enum CodingKeys: String, CodingKey {
            case notifications, unreadCount
        }
    }

    public struct PendingReleaseUpdate: Sendable, Equatable, Decodable {
        public var tagName: String
        public var assetName: String
        public var installerPath: String
        public var readyToRestart: Bool
        public var checksumVerified: Bool
        public var helperStatus: String
        public var error: String?
    }

    // MARK: - Shapes：诊断

    /// `diagnostics.list_startup_degradations` 的一行（core `StartupDegradation`）。
    public struct StartupDegradation: Sendable, Equatable, Decodable {
        public var component: String
        public var reason: String
        public var fallback: String
    }

    // MARK: - usage.*

    public func readUsageStatistics(rangeDays: Int, agent: String) async throws -> UsageStatistics {
        try await call(
            "usage.read_usage_statistics",
            params: .object([
                "rangeDays": .number(String(rangeDays)),
                "agent": .string(agent),
            ]),
            as: UsageStatistics.self
        )
    }

    public func readSessionMetrics(sessionPath: String) async throws -> SessionMetrics {
        try await call(
            "usage.read_session_metrics",
            params: .object(["sessionPath": .string(sessionPath)]),
            as: SessionMetrics.self
        )
    }

    /// 显式刷新（绕过节流）。返回是否真的有变化。
    public func refreshUsageStatisticsIndex() async throws -> Bool {
        let frame = try await connection.request("usage.refresh_usage_statistics_index")
        if case let .bool(changed) = frame.result { return changed }
        throw MaintenanceClientError.malformedReply("refresh reply is not a boolean")
    }

    public func recordAgentConfigUsage(agent: String) async throws -> AgentUsageSnapshot {
        try await call(
            "usage.record_agent_config_usage",
            params: .object(["agent": .string(agent)]),
            as: AgentUsageSnapshot.self
        )
    }

    public func loadAgentUsageSnapshot() async throws -> AgentUsageSnapshot {
        try await call("usage.load_agent_usage_snapshot", as: AgentUsageSnapshot.self)
    }

    /// 订阅额度快照。**只有 Claude 半边可用也是正常应答**（无 Codex 会话时
    /// 无头宿主把 Codex 半边标 `unavailable`），所以这里不把 `unavailable` 当错误。
    public func readSubscriptionUsage() async throws -> SubscriptionUsage {
        let frame = try await connection.request("usage.read_usage_snapshot")
        let claude: UsageSource<ClaudeUsage> =
            try Self.decodeSource(frame.result["claude"], as: ClaudeUsage.self)
        let codex: UsageSource<CodexUsage> =
            try Self.decodeSource(frame.result["codex"], as: CodexUsage.self)
        let fetchedAt = frame.result["fetchedAt"]?.numberText.flatMap(Int.init) ?? 0
        return SubscriptionUsage(claude: claude, codex: codex, fetchedAt: fetchedAt)
    }

    // MARK: - maintenance.*

    public func notifications(force: Bool) async throws -> NotificationFeed {
        try await call(
            "maintenance.get_notifications",
            params: .object(["force": .bool(force)]),
            as: NotificationFeed.self
        )
    }

    public func markNotificationRead(id: String) async throws {
        _ = try await connection.request(
            "maintenance.mark_notification_read",
            params: .object(["id": .string(id)])
        )
    }

    public func markAllNotificationsRead() async throws {
        _ = try await connection.request("maintenance.mark_all_notifications_read")
    }

    /// 待装更新；`nil` 表示盘上没有待装记录（`result` 为 null 是正常形状）。
    public func pendingReleaseUpdate(tagName: String?) async throws -> PendingReleaseUpdate? {
        var params: [String: JSONValue] = [:]
        if let tagName { params["tagName"] = .string(tagName) }
        let frame = try await connection.request(
            "maintenance.get_pending_release_update",
            params: .object(params)
        )
        if case .null = frame.result { return nil }
        return try Self.decode(frame.result, as: PendingReleaseUpdate.self)
    }

    /// 准备更新（下载＋校验＋记账）。**不替换运行中的 app**——那是 GUI 适配器在
    /// 用户确认之后做的另一步。
    public func prepareReleaseUpdate(tagName: String) async throws -> PendingReleaseUpdate {
        try await call(
            "maintenance.prepare_release_update",
            params: .object(["tagName": .string(tagName)]),
            as: PendingReleaseUpdate.self
        )
    }

    /// 周报：由宿主生成并写盘，返回实际写入路径。
    public func generateWeeklyReport(request: JSONValue) async throws -> String {
        let frame = try await connection.request(
            "maintenance.generate_weekly_report",
            params: .object(["request": request])
        )
        if case let .string(path) = frame.result { return path }
        throw MaintenanceClientError.malformedReply("weekly report reply is not a path")
    }

    // MARK: - diagnostics.*

    public func listStartupDegradations() async throws -> [StartupDegradation] {
        let frame = try await connection.request("diagnostics.list_startup_degradations")
        guard case let .array(items) = frame.result else {
            throw MaintenanceClientError.malformedReply("degradation list is not an array")
        }
        return try items.compactMap { item in
            guard case .object = item else { throw MaintenanceClientError.malformedReply("row is not an object") }
            return try Self.decode(item, as: StartupDegradation.self)
        }
    }

    // MARK: - 解析

    private func call<T: Decodable>(
        _ method: String,
        params: JSONValue = .null,
        as type: T.Type
    ) async throws -> T {
        let frame = try await connection.request(method, params: params)
        return try Self.decode(frame.result, as: type)
    }

    /// JSONValue → Codable。`JSONSerialization` 不接受非对象根（数组/字符串根
    /// 在别的命令里出现过），所以非容器形状直接报形状错而不是编一个空值。
    static func decode<T: Decodable>(_ value: JSONValue, as type: T.Type) throws -> T {
        let object = value.foundationObject
        guard JSONSerialization.isValidJSONObject(object) else {
            throw MaintenanceClientError.malformedReply("reply is not a JSON object")
        }
        do {
            return try JSONDecoder().decode(T.self, from: JSONSerialization.data(withJSONObject: object))
        } catch {
            throw MaintenanceClientError.malformedReply(String(describing: error))
        }
    }

    /// `UsageSource<T>`：未知 `status` 归到 `unavailable`（新宿主的新状态不该让
    /// 整个面板失败）。
    static func decodeSource<T: Decodable & Sendable & Equatable>(
        _ value: JSONValue?,
        as type: T.Type
    ) throws -> UsageSource<T> {
        guard let value else {
            throw MaintenanceClientError.malformedReply("usage source is missing")
        }
        let status: String
        if case let .string(text) = value["status"] {
            status = text
        } else {
            status = "unavailable"
        }
        if status == "available", let data = value["data"] {
            return .available(try decode(data, as: type))
        }
        if case let .string(reason) = value["reason"] {
            return .unavailable(reason)
        }
        return .unavailable(status)
    }
}

// MARK: - UsageSource 的 Decodable 实现（internally tagged）

extension MaintenanceClient.UsageSource: Decodable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let status = (try? container.decode(String.self, forKey: .status)) ?? "unavailable"
        if status == "available", let data = try? container.decode(Available.self, forKey: .data) {
            self = .available(data)
            return
        }
        let reason = (try? container.decode(String.self, forKey: .reason)) ?? status
        self = .unavailable(reason)
    }
}
