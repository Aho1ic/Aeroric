import XCTest

@testable import AeroricClient
@testable import AeroricProtocol
@testable import AeroricUI

/// P8-04 刀四验收：Operations 七视图的**状态面**与 wire 解析。
///
/// 这里测的三件事都是"写错了不会编译报错、只会在运行时静默出错"的那一类：
/// ①JSONValue → 模型解析（字段名与 core 的 serde 输出必须逐字对上，`SessionMetrics`
/// 还是 snake_case 的历史形状）；②`UsageSource` 的 internally-tagged enum 形状；
/// ③周报区间的本地日历语义（周日起始/单日区间/ISO 周标题）。
///
/// 视图本身在 `OperationsViews.swift` 里对 store 是只读的，所以状态面的正确性就是
/// 视图的正确性——没有第二份拷贝的判定逻辑可以在这里断言（那是 core 的用例）。
@MainActor
final class MaintenanceStoreTests: XCTestCase {

    // MARK: - wire 解析

    private func statisticsJSON() -> JSONValue {
        .object([
            "rangeDays": .number("7"),
            "from": .string("2026-09-07"),
            "to": .string("2026-09-13"),
            "agent": .string("all"),
            "updatedAt": .number("1789000000000"),
            "totals": .object([
                "totalTokens": .number("12000"),
                "inputTokens": .number("9000"),
                "outputTokens": .number("3000"),
                "cacheCreationTokens": .number("500"),
                "cacheReadTokens": .number("2500"),
                "cacheHitRate": .number("0.2"),
                "requestCount": .number("42"),
                "totalCost": .number("1.25"),
                "pricedRequestCount": .number("40"),
                "estimatedRequestCount": .number("2"),
            ]),
            "series": .array([
                .object([
                    "date": .string("2026-09-07"),
                    "totalTokens": .number("6000"),
                    "requestCount": .number("20"),
                    "totalCost": .number("0.6"),
                ]),
                .object([
                    "date": .string("2026-09-07"),
                    "hour": .number("14"),
                    "totalTokens": .number("6000"),
                    "requestCount": .number("22"),
                    "totalCost": .number("0.65"),
                ]),
            ]),
            "breakdown": .object([
                "codex": .object(["totalTokens": .number("100"), "requestCount": .number("1"), "totalCost": .number("0.1")]),
                "claude": .object(["totalTokens": .number("200"), "requestCount": .number("2"), "totalCost": .number("0.2")]),
                "dsh": .object(["totalTokens": .number("0"), "requestCount": .number("0"), "totalCost": .number("0")]),
                "omp": .object(["totalTokens": .number("0"), "requestCount": .number("0"), "totalCost": .number("0")]),
            ]),
        ])
    }

    func testUsageStatisticsParsesTheCoreShape() throws {
        let parsed = try MaintenanceClient.decode(
            statisticsJSON(), as: MaintenanceClient.UsageStatistics.self
        )
        XCTAssertEqual(parsed.rangeDays, 7)
        XCTAssertEqual(parsed.totals.requestCount, 42)
        XCTAssertEqual(parsed.totals.totalCost, 1.25, accuracy: 0.0001)
        XCTAssertEqual(parsed.totals.estimatedRequestCount, 2)
        XCTAssertTrue(parsed.hasData)
        XCTAssertEqual(parsed.breakdown.claude.requestCount, 2)
        // 按天桶没有 hour；24 小时档才有。0 点不是默认值，是**没有这个键**。
        XCTAssertNil(parsed.series[0].hour)
        XCTAssertEqual(parsed.series[1].hour, 14)
    }

    func testUsageTotalsDegradeToZeroForMissingKeys() throws {
        // 只带 requestCount 的残缺应答：其余字段按 0 读（不是解析失败）。
        let parsed = try MaintenanceClient.decode(
            .object([
                "rangeDays": .number("7"), "from": .string(""), "to": .string(""),
                "agent": .string("all"), "updatedAt": .number("0"),
                "totals": .object(["requestCount": .number("3")]),
                "series": .array([]),
                "breakdown": .object([
                    "codex": .object([:]),
                    "claude": .object([:]),
                    "dsh": .object([:]),
                    "omp": .object([:]),
                ]),
            ]) as JSONValue,
            as: MaintenanceClient.UsageStatistics.self
        )
        XCTAssertEqual(parsed.totals.requestCount, 3)
        XCTAssertEqual(parsed.totals.totalCost, 0)
        XCTAssertEqual(parsed.breakdown.claude.requestCount, 0)
    }

    func testSessionMetricsKeepsTheSnakeCaseShape() throws {
        let parsed = try MaintenanceClient.decode(
            .object([
                "tool_calls": .number("7"),
                "duration_secs": .number("12.5"),
                "total_tokens": .number("9000"),
                "context_tokens": .number("4000"),
                "context_window": .number("16000"),
            ]),
            as: MaintenanceClient.SessionMetrics.self
        )
        XCTAssertEqual(parsed.toolCalls, 7)
        XCTAssertEqual(parsed.durationSecs, 12.5, accuracy: 0.0001)
        XCTAssertEqual(parsed.contextTokens, 4000)
        XCTAssertEqual(parsed.contextUsage ?? 0, 0.25, accuracy: 0.0001)
    }

    func testContextUsageIsNilWhenTheFamilyDoesNotExposeAWindow() throws {
        // Claude 会话不暴露窗口：0 ⇒ 隐藏占比，而不是除以 0 编一个百分比。
        let parsed = try MaintenanceClient.decode(
            .object([
                "tool_calls": .number("1"), "duration_secs": .number("1"),
                "total_tokens": .number("10"), "context_tokens": .number("10"),
                "context_window": .number("0"),
            ]),
            as: MaintenanceClient.SessionMetrics.self
        )
        XCTAssertNil(parsed.contextUsage)
    }

    func testUsageSourceSplitsAvailableFromUnavailable() throws {
        let available = try MaintenanceClient.decodeSource(
            .object([
                "status": .string("available"),
                "data": .object([
                    "fiveHour": .object([
                        "usedPercent": .number("30"), "remainingPercent": .number("70"),
                    ]),
                    "sevenDay": .null,
                ]),
            ]),
            as: MaintenanceClient.ClaudeUsage.self
        )
        guard case let .available(claude) = available else {
            return XCTFail("expected available, got \(available)")
        }
        XCTAssertEqual(claude.fiveHour?.usedPercent, 30)
        XCTAssertEqual(claude.fiveHour?.remainingPercent, 70)
        XCTAssertNil(claude.sevenDay?.resetAt)

        let unavailable = try MaintenanceClient.decodeSource(
            .object(["status": .string("unavailable"), "reason": .string("no Codex session")]),
            as: MaintenanceClient.CodexUsage.self
        )
        guard case let .unavailable(reason) = unavailable else {
            return XCTFail("expected unavailable, got \(unavailable)")
        }
        XCTAssertEqual(reason, "no Codex session")

        // 未知状态归到不可用（新宿主的新状态不该让面板崩）。
        let unknown = try MaintenanceClient.decodeSource(
            .object(["status": .string("pending-review")]),
            as: MaintenanceClient.CodexUsage.self
        )
        guard case let .unavailable(text) = unknown else {
            return XCTFail("expected unavailable, got \(unknown)")
        }
        XCTAssertEqual(text, "pending-review")
    }

    func testNotificationFeedParsesFlagsAndSanitizedURL() throws {
        let parsed = try MaintenanceClient.decode(
            .object([
                "notifications": .array([
                    .object([
                        "id": .string("release-7"), "level": .string("info"),
                        "title": .string("Aeroric 9.9.9"), "body": .string("notes"),
                        "createdAt": .string("2026-09-08T00:00:00Z"), "isRead": .bool(false),
                        "newerThanCurrent": .bool(true),
                        "updateInstallSupported": .bool(true),
                    ]),
                ]),
                "unreadCount": .number("1"),
            ]),
            as: MaintenanceClient.NotificationFeed.self
        )
        XCTAssertEqual(parsed.unreadCount, 1)
        XCTAssertEqual(parsed.notifications.first?.id, "release-7")
        XCTAssertTrue(parsed.notifications.first?.newerThanCurrent ?? false)
        XCTAssertTrue(parsed.notifications.first?.updateInstallSupported ?? false)
        // 没有 url 键 ⇒ nil（宿主已把 javascript:/data: 过滤掉了，客户端不再过滤）。
        XCTAssertNil(parsed.notifications.first?.url)
        XCTAssertNil(parsed.notifications.first?.bodyZh)
    }

    func testStartupDegradationRowsParse() throws {
        let rows = try MaintenanceClient.decode(
            .array([
                .object([
                    "component": .string("dbx-state"),
                    "reason": .string("disk full"),
                    "fallback": .string(":memory:"),
                ]),
            ]),
            as: [MaintenanceClient.StartupDegradation].self
        )
        XCTAssertEqual(rows.first?.component, "dbx-state")
        XCTAssertEqual(rows.first?.fallback, ":memory:")
    }

    // MARK: - 周报区间（本地日历）

    /// 固定 `Calendar`（UTC+8）避免用例跟着跑测机器漂——与 core 侧用例同一个理由。
    private func calendar() -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 8 * 3600)!
        calendar.firstWeekday = 1
        return calendar
    }

    private func date(_ year: Int, _ month: Int, _ day: Int, _ hour: Int = 12) -> Date {
        calendar().date(from: DateComponents(year: year, month: month, day: day, hour: hour))!
    }

    func testWeekWindowFollowsTheConfiguredStartDay() {
        let wednesday = date(2026, 9, 9)
        let sunday = WeekWindow.containing(wednesday, weekStartDay: 0, weekEndDay: 6, calendar: calendar())
        XCTAssertEqual(sunday.days, 7)
        let monday = WeekWindow.containing(wednesday, weekStartDay: 1, weekEndDay: 0, calendar: calendar())

        // 周日起始与周一起始落在不同的日子，但都覆盖 7 天。
        XCTAssertEqual(monday.days, 7)
        XCTAssertNotEqual(sunday.fromMs, monday.fromMs)
        // 单日区间。
        let single = WeekWindow.containing(wednesday, weekStartDay: 3, weekEndDay: 3, calendar: calendar())
        XCTAssertEqual(single.days, 1)
        XCTAssertEqual(single.toMs - single.fromMs, 86_400_000 - 1)
    }

    func testWeekLabelUsesTheIsoWeekOfTheWindowMidpoint() {
        let window = WeekWindow.containing(
            date(2026, 9, 9), weekStartDay: 0, weekEndDay: 6, calendar: calendar()
        )
        // 周日→周六的区间有 6/7 天落在下一个 ISO 周，取中点才不会写出早一周的标题。
        XCTAssertEqual(window.label, "2026-W37")
    }

    func testCleanupDraftDefaultsMatchTheShippingSettings() {
        let draft = CleanupDraft.default
        XCTAssertFalse(draft.enabled, "默认关闭——清理是破坏性动作")
        XCTAssertEqual(draft.mode, "weekly")
        XCTAssertEqual(draft.weekday, 0, "0=周日")
        XCTAssertEqual(draft.hour, 20)
        XCTAssertEqual(draft.retainDays, 30)
        XCTAssertEqual(draft.intervalDays, 7)
    }

    // MARK: - Store 的状态面

    func testStoreWithoutConnectionReportsInsteadOfPretending() async {
        let store = MaintenanceStore(client: nil)
        XCTAssertNil(store.subscription)
        XCTAssertFalse(store.statistics.hasData)

        await store.loadStatistics()
        XCTAssertEqual(store.lastError, "未连接宿主")
        XCTAssertFalse(store.isRefreshing, "失败的刷新不留进行中标记")

        // 生成周报返回 nil 而不是空路径（调用方据此不显示"已写入"）。
        let path = await store.generateWeeklyReport(
            window: WeekWindow.containing(date(2026, 9, 9), calendar: calendar()),
            locale: "zh-CN",
            outputPath: "/tmp/x.md",
            tasks: []
        )
        XCTAssertNil(path)
        XCTAssertEqual(store.lastError, "未连接宿主")

        store.clearError()
        XCTAssertNil(store.lastError)
    }

    func testReportTaskPayloadOmitsAbsentOptionalFields() {
        let withOptionals = WeeklyReportTaskPayload(
            projectName: "Alpha", projectPath: "/repo", title: "T", agent: "claude",
            status: "done", createdAt: 1, completedAt: 2, failureReason: "boom",
            sessionPath: "/tmp/s.jsonl", sessionFamily: "codex", additions: 3, deletions: 4
        )
        guard case let .object(full) = withOptionals.jsonValue else {
            return XCTFail("payload must be an object")
        }
        XCTAssertEqual(Set(full.keys), Set([
            "projectName", "projectPath", "title", "agent", "status", "createdAt",
            "completedAt", "failureReason", "sessionPath", "sessionFamily",
            "additions", "deletions",
        ]))

        // 缺的可选项**不进 wire**（空壳键会让宿主把"没摘要"读成"摘要为空"）。
        let bare = WeeklyReportTaskPayload(
            projectName: "Alpha", projectPath: "/repo", title: "T", agent: "claude",
            status: "done", createdAt: 1
        )
        guard case let .object(minimal) = bare.jsonValue else {
            return XCTFail("payload must be an object")
        }
        XCTAssertEqual(Set(minimal.keys), Set([
            "projectName", "projectPath", "title", "agent", "status", "createdAt",
        ]))
    }

    func testAgentUsageSnapshotParsesTheSortWindow() throws {
        let parsed = try MaintenanceClient.decode(
            .object([
                "agents": .object([
                    "codex": .object([
                        "recentCount": .number("3"),
                        "totalCount": .number("17"),
                        "lastUsedAt": .number("1789000000000"),
                    ]),
                ]),
                "windowDays": .number("7"),
                "computedAt": .number("1789000000000"),
            ]),
            as: MaintenanceClient.AgentUsageSnapshot.self
        )
        XCTAssertEqual(parsed.windowDays, 7)
        XCTAssertEqual(parsed.agents["codex"]?.recentCount, 3)
        XCTAssertEqual(parsed.agents["codex"]?.totalCount, 17)
        XCTAssertTrue(MaintenanceClient.AgentUsageSnapshot.empty.agents.isEmpty)
    }
}
