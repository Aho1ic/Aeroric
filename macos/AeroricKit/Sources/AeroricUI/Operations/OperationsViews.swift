import SwiftUI

import AeroricClient

/// P8-04 Action 2：Operations 七视图（UsageDashboard / NotificationCenterView /
/// CleanupSettingsView / WeeklyReportView / PermissionSettingsView / ReleaseView /
/// DiagnosticsView）。
///
/// 三处刻意的"不显示"比显示更要紧：
/// - **没有数据就不显示 0**：`totals.requestCount == 0` 时用量面板给空态而不是
///   一排 0（0 成本与"还没扫到"是两回事）。
/// - **窗口未知就不显示占比**：`SessionMetrics.contextWindow == 0` 时隐藏上下文
///   百分比，而不是编一个分母。
/// - **离线/不可用时说明原因**：订阅额度与 daemon 类状态都带 reason 显示。
///
/// 所有破坏性/系统级动作都要一步以上的确认：清理策略是"打开开关不等于立刻删"，
/// 权限 Re-authorize 由 `PermissionSettingsView` 的内部确认门控（刀五），
/// 更新在 `ReleaseView` 里分成"准备"与"替换"两步。

// MARK: - 1. UsageDashboard

/// 用量面板：区间/族筛选、总量、逐日序列、四族分解、订阅额度。
public struct UsageDashboard: View {
    @ObservedObject var store: MaintenanceStore
    /// 需要"打开宿主某处"的动作（订阅额度不可用时的解释入口）；nil 表示不显示。
    public var onOpenHostSettings: (() -> Void)?

    public init(store: MaintenanceStore, onOpenHostSettings: (() -> Void)? = nil) {
        self.store = store
        self.onOpenHostSettings = onOpenHostSettings
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Picker("区间", selection: Binding(
                    get: { store.range },
                    set: { range in Task { await store.select(range: range) } }
                )) {
                    ForEach(MaintenanceStore.Range.allCases) { range in
                        Text(range.label).tag(range)
                    }
                }
                .pickerStyle(.segmented)
                .accessibilityIdentifier("usage.range")

                Picker("代理", selection: Binding(
                    get: { store.agentFilter },
                    set: { agent in Task { await store.select(agent: agent) } }
                )) {
                    ForEach(MaintenanceStore.AgentFilter.allCases) { agent in
                        Text(agent.rawValue).tag(agent)
                    }
                }
                .frame(width: 140)
                .accessibilityIdentifier("usage.agent")

                Button("刷新") { Task { await store.refreshIndex() } }
                    .disabled(store.isRefreshing)
                    .accessibilityIdentifier("usage.refresh")
            }

            if !store.statistics.hasData {
                Text("这段时间没有用量记录。")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("usage.empty")
            } else {
                let totals = store.statistics.totals
                Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 6) {
                    GridRow {
                        Text("请求数").foregroundStyle(AeroricColor.textTertiary)
                        Text("\(totals.requestCount)")
                    }
                    GridRow {
                        Text("Token").foregroundStyle(AeroricColor.textTertiary)
                        Text("\(totals.totalTokens)")
                    }
                    GridRow {
                        Text("费用").foregroundStyle(AeroricColor.textTertiary)
                        // 有推算单价的行时标注口径，避免把估算当账单。
                        Text(String(format: "$%.4f", totals.totalCost)
                            + (totals.estimatedRequestCount > 0
                                ? "（含 \(totals.estimatedRequestCount) 条按同档推算）" : ""))
                    }
                    GridRow {
                        Text("缓存命中率").foregroundStyle(AeroricColor.textTertiary)
                        Text(String(format: "%.0f%%", totals.cacheHitRate * 100))
                    }
                }
                .accessibilityIdentifier("usage.totals")

                UsageSeriesChart(days: store.statistics.series)

                HStack(spacing: 16) {
                    ForEach(breakdownRows, id: \.label) { row in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.label).font(.caption).foregroundStyle(AeroricColor.textTertiary)
                            Text("\(row.totals.requestCount) 次")
                            Text(String(format: "$%.4f", row.totals.totalCost))
                                .font(.caption.monospaced())
                        }
                    }
                }
                .accessibilityIdentifier("usage.breakdown")
            }

            Divider()
            SubscriptionUsageSection(store: store, onOpenHostSettings: onOpenHostSettings)
        }
        .padding(16)
    }

    private var breakdownRows: [(label: String, totals: MaintenanceClient.UsageTotals)] {
        let breakdown = store.statistics.breakdown
        return [
            ("Codex", breakdown.codex),
            ("Claude", breakdown.claude),
            ("DSH", breakdown.dsh),
            ("OMP", breakdown.omp),
        ]
    }
}

/// 逐日序列：只画有数据的桶（24 小时档是 24 个，按天档是区间天数）。
struct UsageSeriesChart: View {
    let days: [MaintenanceClient.UsageDay]

    var body: some View {
        let peak = max(1, days.map(\.totalTokens).max() ?? 1)
        HStack(alignment: .bottom, spacing: 2) {
            ForEach(Array(days.enumerated()), id: \.offset) { _, day in
                VStack(spacing: 2) {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(AeroricColor.accent)
                        .frame(width: 8, height: max(1, CGFloat(60 * day.totalTokens / peak)))
                    Text(day.hour.map { "\($0)" } ?? String(day.date.suffix(2)))
                        .font(.system(size: 8))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
        }
        .frame(height: 84, alignment: .bottom)
        .accessibilityIdentifier("usage.series")
    }
}

/// 订阅额度（Claude / Codex）。两族都可能"不可用"，且各有原因。
struct SubscriptionUsageSection: View {
    @ObservedObject var store: MaintenanceStore
    var onOpenHostSettings: (() -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("订阅额度").font(.headline)
                Button("刷新") { Task { await store.loadSubscription() } }
                    .accessibilityIdentifier("usage.subscription-refresh")
                if let onOpenHostSettings {
                    Button("宿主设置", action: onOpenHostSettings)
                }
            }
            if let subscription = store.subscription {
                sourceRow("Claude", claudeText(subscription.claude))
                sourceRow("Codex", codexText(subscription.codex))
            } else {
                Text("尚未读取。")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
    }

    private func sourceRow(_ label: String, _ text: String?) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(label).frame(width: 60, alignment: .leading)
            if let text {
                Text(text).font(.caption.monospaced())
            } else {
                Text("不可用").font(.caption).foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .accessibilityIdentifier("usage.subscription-\(label.lowercased())")
    }

    /// `UsageSource` 是 core 的 internally-tagged enum，没有 `map`：不可用时把
    /// **原因**显示出来（不是"没有数据"），因为"宿主没有 Codex 会话"与
    /// "读取失败"要给用户完全不同的动作。
    private func claudeText(_ source: MaintenanceClient.UsageSource<MaintenanceClient.ClaudeUsage>) -> String? {
        switch source {
        case let .available(usage):
            let text = windowText(usage)
            return text.isEmpty ? "不可用" : text
        case let .unavailable(reason):
            return "不可用：" + reason
        }
    }

    private func codexText(_ source: MaintenanceClient.UsageSource<MaintenanceClient.CodexUsage>) -> String? {
        switch source {
        case let .available(usage):
            let text = windowText(usage)
            return text.isEmpty ? "不可用" : text
        case let .unavailable(reason):
            return "不可用：" + reason
        }
    }

    private func windowText(_ usage: MaintenanceClient.ClaudeUsage) -> String {
        [usage.fiveHour.map { "5h \($0.usedPercent)%" }, usage.sevenDay.map { "7d \($0.usedPercent)%" }]
            .compactMap { $0 }
            .joined(separator: " · ")
    }

    private func windowText(_ usage: MaintenanceClient.CodexUsage) -> String {
        let windows = [usage.primary.map { "primary \($0.usedPercent)%" },
                       usage.secondary.map { "secondary \($0.usedPercent)%" }]
            .compactMap { $0 }
            .joined(separator: " · ")
        return [usage.planType, windows.isEmpty ? nil : windows]
            .compactMap { $0 }
            .joined(separator: " · ")
    }
}

// MARK: - 2. NotificationCenterView

/// 通知中心：发布公告 + 更新提示。已读状态由宿主持有（本地不改标记）。
public struct NotificationCenterView: View {
    @ObservedObject var store: MaintenanceStore
    public var onSelect: ((MaintenanceClient.NotificationItem) -> Void)?

    public init(store: MaintenanceStore, onSelect: ((MaintenanceClient.NotificationItem) -> Void)? = nil) {
        self.store = store
        self.onSelect = onSelect
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("通知").font(.headline)
                Text("\(store.feed.unreadCount) 条未读")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textTertiary)
                Spacer()
                Button("刷新") { Task { await store.loadNotifications(force: true) } }
                    .accessibilityIdentifier("notifications.refresh")
                Button("全部标为已读") { Task { await store.markAllRead() } }
                    .disabled(store.feed.unreadCount == 0)
                    .accessibilityIdentifier("notifications.mark-all")
            }

            if store.feed.notifications.isEmpty {
                Text("没有通知。")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("notifications.empty")
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        ForEach(store.feed.notifications, id: \.id) { item in
                            NotificationRow(item: item) {
                                Task { await store.markRead(item.id) }
                                onSelect?(item)
                            }
                        }
                    }
                }
            }
        }
        .padding(16)
    }
}

struct NotificationRow: View {
    let item: MaintenanceClient.NotificationItem
    var onOpen: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Circle()
                .fill(item.isRead ? Color.clear : AeroricColor.accent)
                .frame(width: 6, height: 6)
                .padding(.top, 6)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).fontWeight(item.isRead ? .regular : .semibold)
                Text(item.body).font(.caption).foregroundStyle(AeroricColor.textSecondary)
                // 链接只显示**宿主放行过的** http(s) URL（`sanitize_url` 在 core，
                // 客户端拿到的 None 就是不显示，客户端不再过滤一遍）。
                if let url = item.url {
                    Text(url).font(.caption2).foregroundStyle(AeroricColor.textTertiary).lineLimit(1)
                }
            }
            Spacer()
            if !item.isRead {
                Button("标为已读") { onOpen() }
                    .buttonStyle(.link)
                    .accessibilityIdentifier("notifications.read-\(item.id)")
            }
        }
        .contentShape(Rectangle())
        .accessibilityIdentifier("notifications.row-\(item.id)")
    }
}

// MARK: - 3. CleanupSettingsView

/// 清理设置：开关、模式、周期/时点、保留期。
///
/// **"打开开关不等于立刻删"**是本面板的核心语义（core 的
/// `ShouldRun::NoPreviousRun`）：第一次到点只记时间戳，下一轮才真的删。视图把这条
/// 规则说给用户听，而不是让它在实现里默默生效。
public struct CleanupSettingsView: View {
    @State private var enabled: Bool
    @State private var mode: String
    @State private var weekday: Int
    @State private var hour: Int
    @State private var intervalDays: Int
    @State private var retainDays: Int
    public var onSave: ((CleanupDraft) -> Void)?

    public init(
        draft: CleanupDraft = .default,
        onSave: ((CleanupDraft) -> Void)? = nil
    ) {
        self._enabled = State(initialValue: draft.enabled)
        self._mode = State(initialValue: draft.mode)
        self._weekday = State(initialValue: draft.weekday)
        self._hour = State(initialValue: draft.hour)
        self._intervalDays = State(initialValue: draft.intervalDays)
        self._retainDays = State(initialValue: draft.retainDays)
        self.onSave = onSave
    }

    public var body: some View {
        Form {
            Toggle("启用自动清理", isOn: $enabled)
                .accessibilityIdentifier("cleanup.enabled")
            Picker("模式", selection: $mode) {
                Text("每周").tag("weekly")
                Text("按间隔").tag("interval")
            }
            .accessibilityIdentifier("cleanup.mode")

            if mode == "weekly" {
                Picker("星期", selection: $weekday) {
                    ForEach(Array(weekdayNames.enumerated()), id: \.offset) { index, name in
                        Text(name).tag(index)
                    }
                }
                .accessibilityIdentifier("cleanup.weekday")
                Stepper("时刻：\(hour):00", value: $hour, in: 0...23)
                    .accessibilityIdentifier("cleanup.hour")
            } else {
                Stepper("间隔：\(intervalDays) 天", value: $intervalDays, in: 1...90)
                    .accessibilityIdentifier("cleanup.interval")
            }

            Stepper("对话保留：\(retainDays) 天", value: $retainDays, in: 1...365)
                .accessibilityIdentifier("cleanup.retain")

            Text("只会清理已完成/失败/取消的任务；收藏、还能续跑、以及未处理的 worktree 不动。刚打开开关时先记时点，下一个周期才真的清。")
                .font(.caption)
                .foregroundStyle(AeroricColor.textTertiary)
                .accessibilityIdentifier("cleanup.note")

            Button("保存") { onSave?(current) }
                .accessibilityIdentifier("cleanup.save")
        }
        .padding(16)
        .accessibilityIdentifier("operations.cleanup")
    }

    private var weekdayNames: [String] {
        ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]
    }

    private var current: CleanupDraft {
        CleanupDraft(
            enabled: enabled, mode: mode, weekday: weekday, hour: hour,
            intervalDays: intervalDays, retainDays: retainDays
        )
    }
}

/// 清理策略的草稿（与 `settings.json` 的 `auto_cleanup_settings` 同字段）。
public struct CleanupDraft: Sendable, Equatable {
    public var enabled: Bool
    public var mode: String
    public var weekday: Int
    public var hour: Int
    public var intervalDays: Int
    public var retainDays: Int

    public static let `default` = CleanupDraft(
        enabled: false, mode: "weekly", weekday: 0, hour: 20, intervalDays: 7, retainDays: 30
    )

    public init(
        enabled: Bool, mode: String, weekday: Int, hour: Int, intervalDays: Int, retainDays: Int
    ) {
        self.enabled = enabled
        self.mode = mode
        self.weekday = weekday
        self.hour = hour
        self.intervalDays = intervalDays
        self.retainDays = retainDays
    }
}

// MARK: - 4. WeeklyReportView

/// 周报：区间预览、任务数、落盘路径、生成。
public struct WeeklyReportView: View {
    @ObservedObject var store: MaintenanceStore
    public var window: WeekWindow
    public var taskCount: Int
    @State private var outputPath: String
    @State private var lastWritten: String?
    @State private var generating = false
    public var tasksForRequest: () -> [WeeklyReportTaskPayload]

    public init(
        store: MaintenanceStore,
        window: WeekWindow,
        taskCount: Int,
        outputPath: String = "",
        tasksForRequest: @escaping () -> [WeeklyReportTaskPayload] = { [] }
    ) {
        self.store = store
        self.window = window
        self.taskCount = taskCount
        self._outputPath = State(initialValue: outputPath)
        self.tasksForRequest = tasksForRequest
    }

    public var body: some View {
        Form {
            LabeledContent("区间", value: "\(window.label) · \(window.days) 天")
                .accessibilityIdentifier("weekly.window")
            LabeledContent("任务数", value: "\(taskCount)")
                .accessibilityIdentifier("weekly.count")
            TextField("输出路径（.md）", text: $outputPath)
                .accessibilityIdentifier("weekly.output")
            if taskCount == 0 {
                Text("这个区间没有任务，周报会是空的——宿主会拒绝生成。")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("weekly.empty-note")
            }
            Button("生成周报") {
                generating = true
                Task {
                    lastWritten = await store.generateWeeklyReport(
                        window: window,
                        locale: localeIdentifier,
                        outputPath: outputPath,
                        tasks: tasksForRequest()
                    )
                    generating = false
                }
            }
            // 空任务、空路径都不发请求：core 会拒，但让用户先看见原因更省事。
            .disabled(generating || taskCount == 0 || outputPath.trimmingCharacters(in: .whitespaces).isEmpty)
            .accessibilityIdentifier("weekly.generate")
            if generating {
                ProgressView().accessibilityIdentifier("weekly.progress")
            }
            if let lastWritten {
                Text("已写入 \(lastWritten)")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("weekly.written")
            }
            if let error = store.lastError {
                Text(error).font(.caption).foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("weekly.error")
            }
        }
        .padding(16)
        .accessibilityIdentifier("operations.weekly")
    }

    /// 报告正文的语言（md 是产物，按生成时的界面语言写死一套文案）。
    private var localeIdentifier: String {
        Locale.current.identifier.replacingOccurrences(of: "_", with: "-")
    }
}

// MARK: - 5. ReleaseView

/// 版本与更新：当前版本、可用更新、**两步**（准备 → 用户确认 → 替换）。
///
/// 一个不变量：**准备只是下载与校验**。替换运行中的 app 需要重启进程，所以它由
/// 用户在读完"将要发生什么"之后显式触发；`prepareUpdate` 永远不会自己走到那一步。
public struct ReleaseView: View {
    @ObservedObject var store: MaintenanceStore
    public var currentVersion: String
    /// 用户确认后交给 GUI 适配器执行替换（Tauri 命令 / 原生 app 重建）。
    public var onInstall: ((String) -> Void)?
    @State private var confirmed = false

    public init(
        store: MaintenanceStore,
        currentVersion: String,
        onInstall: ((String) -> Void)? = nil
    ) {
        self.store = store
        self.currentVersion = currentVersion
        self.onInstall = onInstall
    }

    public var body: some View {
        Form {
            LabeledContent("当前版本", value: currentVersion)
                .accessibilityIdentifier("release.current")

            let updateCandidates = store.feed.notifications.filter(\.newerThanCurrent)
            if updateCandidates.isEmpty {
                Text("没有比当前更新的版本。")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("release.uptodate")
            } else {
                ForEach(updateCandidates, id: \.id) { candidate in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(candidate.releaseTag ?? candidate.id)
                            if !candidate.updateInstallSupported {
                                // 这台机器上没有可装的资产（架构/发行版不匹配）。
                                Text("这个版本没有适配本机的安装包")
                                    .font(.caption)
                                    .foregroundStyle(AeroricColor.warning)
                            }
                        }
                        Spacer()
                        Button("下载并校验") {
                            Task { await store.prepareUpdate(tagName: candidate.releaseTag ?? "") }
                        }
                        .disabled(candidate.releaseTag == nil || !candidate.updateInstallSupported)
                        .accessibilityIdentifier("release.prepare-\(candidate.id)")
                    }
                }
            }

            if let pending = store.pendingUpdate {
                Divider()
                LabeledContent("待安装", value: pending.tagName)
                    .accessibilityIdentifier("release.pending")
                LabeledContent("校验", value: pending.checksumVerified ? "已通过" : "未通过")
                    .accessibilityIdentifier("release.checksum")
                Text("替换会退出并重启应用。正在运行的任务会被中断——先确认没有任务在跑。")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textTertiary)
                Toggle("我已确认要重启并替换", isOn: $confirmed)
                    .accessibilityIdentifier("release.confirm")
                Button("重启并安装") {
                    guard confirmed, pending.readyToRestart else { return }
                    onInstall?(pending.tagName)
                }
                .disabled(!confirmed || !pending.readyToRestart)
                .accessibilityIdentifier("release.install")
            }

            if let error = store.lastError {
                Text(error).font(.caption).foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("release.error")
            }
        }
        .padding(16)
        .accessibilityIdentifier("operations.release")
    }
}

// MARK: - 6. DiagnosticsView

/// 启动降级账：只读。空表＝一切正常（与 core 的 `list_startup_degradations` 同语义）。
public struct DiagnosticsView: View {
    @ObservedObject var store: MaintenanceStore

    public init(store: MaintenanceStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("启动诊断").font(.headline)
                Spacer()
                Button("重新读取") { Task { await store.loadDegradations() } }
                    .accessibilityIdentifier("diagnostics.refresh")
            }
            if store.degradations.isEmpty {
                Text("没有降级记录。")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("diagnostics.empty")
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        ForEach(Array(store.degradations.enumerated()), id: \.offset) { _, row in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(row.component).fontWeight(.semibold)
                                Text(row.reason).font(.caption).foregroundStyle(AeroricColor.textSecondary)
                                Text("已退到 \(row.fallback)")
                                    .font(.caption2)
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier("diagnostics.row-\(row.component)")
                        }
                    }
                }
            }
        }
        .padding(16)
    }
}

// MARK: - 7. PermissionSettingsView

/// 系统权限面板的**外壳**：渲染宿主给的状态、转发请求/打开设置/重启。
///
/// 身份的呈现（`unstableSignature`/`notBundled` 的解释、Re-authorize 的两段确认与
/// 它到底会 reset 哪些服务）在 `PermissionPresentation`（刀五）。这里只保证：
/// **没有授权就不显示成已授权**、**Re-authorize 不是一次点击**。
public struct PermissionSettingsView: View {
    public var report: PermissionReport?
    public var onRequest: ((String) -> Void)?
    public var onOpenSettings: ((String) -> Void)?
    public var onReauthorize: ((ReauthorizePlan) -> Void)?
    @State private var pendingReauthorize: ReauthorizePlan?

    public init(
        report: PermissionReport?,
        onRequest: ((String) -> Void)? = nil,
        onOpenSettings: ((String) -> Void)? = nil,
        onReauthorize: ((ReauthorizePlan) -> Void)? = nil
    ) {
        self.report = report
        self.onRequest = onRequest
        self.onOpenSettings = onOpenSettings
        self.onReauthorize = onReauthorize
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let report {
                if let plan = report.identity.plan {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(plan.headline).fontWeight(.semibold)
                            .foregroundStyle(report.identity.warning == nil ? AeroricColor.textSecondary : AeroricColor.warning)
                            .accessibilityIdentifier("permissions.identity-headline")
                        Text(plan.explanation)
                            .font(.caption)
                            .foregroundStyle(AeroricColor.textSecondary)
                            .accessibilityIdentifier("permissions.identity-explanation")
                        if let reauthorize = plan.reauthorize {
                            Button("重新授权…") { pendingReauthorize = reauthorize }
                                .accessibilityIdentifier("permissions.reauthorize")
                        }
                    }
                }

                ForEach(report.permissions, id: \.id) { permission in
                    HStack(alignment: .top) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(permission.id)
                            if let detail = permission.detail {
                                Text(detail).font(.caption).foregroundStyle(AeroricColor.textTertiary)
                            }
                        }
                        Spacer()
                        Text(permission.statusLabel)
                            .font(.caption)
                            .foregroundStyle(permission.status == .granted ? AeroricColor.success : AeroricColor.textTertiary)
                            .accessibilityIdentifier("permissions.status-\(permission.id)")
                        if permission.canRequestInApp {
                            Button("请求") { onRequest?(permission.id) }
                                .accessibilityIdentifier("permissions.request-\(permission.id)")
                        } else if permission.canOpenSettings {
                            Button("打开设置") { onOpenSettings?(permission.id) }
                                .accessibilityIdentifier("permissions.open-\(permission.id)")
                        }
                    }
                }
            } else {
                Text("权限状态尚未读取。")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("permissions.unknown")
            }
        }
        .padding(16)
        // 二段确认：先把"会 reset 哪些服务、用哪个身份、之后要重新请求什么"说清楚。
        .sheet(item: $pendingReauthorize) { plan in
            VStack(alignment: .leading, spacing: 12) {
                Text("重新授权").font(.headline)
                Text(plan.summary).font(.callout)
                Text("将会清除：\(plan.services.joined(separator: "、"))")
                    .font(.caption.monospaced())
                    .accessibilityIdentifier("permissions.reauthorize-services")
                Text("身份：\(plan.bundleIdentifier)")
                    .font(.caption.monospaced())
                    .accessibilityIdentifier("permissions.reauthorize-bundle")
                Text("之后需要重新请求：\(plan.reRequest.joined(separator: "、"))")
                    .font(.caption)
                    .accessibilityIdentifier("permissions.reauthorize-followup")
                HStack {
                    Button("取消") { pendingReauthorize = nil }
                    Spacer()
                    Button("确认清除并重新请求") {
                        onReauthorize?(plan)
                        pendingReauthorize = nil
                    }
                    .accessibilityIdentifier("permissions.reauthorize-confirm")
                }
            }
            .padding(20)
            .frame(minWidth: 420)
        }
        .accessibilityIdentifier("operations.permissions")
    }
}

// MARK: - 权限面的呈现类型（刀五的 `PermissionPresentation` 产出）

public enum PermissionStatus: String, Sendable, Codable {
    case granted, notGranted, unknown

    /// 未知**不显示成已授权**（`.unknown` 与 `.notGranted` 都按"未授权"外观走，
    /// 但文案不同——前者是"探测不到"，后者是"系统说没给"）。
    public var isGranted: Bool { self == .granted }

    public var label: String {
        switch self {
        case .granted: "已授权"
        case .notGranted: "未授权"
        case .unknown: "无法判定"
        }
    }
}

public struct PermissionReport: Sendable, Equatable {
    public struct Identity: Sendable, Equatable {
        /// 代码签名身份带来的告警；nil＝这次构建的授权是稳定的。
        public var warning: String?
        public var bundleIdentifier: String
        /// 呈现计划（由 `PermissionPresentation` 构造）。
        public var plan: PermissionPlan?
    }

    public struct Entry: Sendable, Equatable {
        public var id: String
        public var status: PermissionStatus
        public var detail: String?
        public var canRequestInApp: Bool
        public var canOpenSettings: Bool
        public var canReset: Bool

        public var statusLabel: String { status.label }
    }

    public var platform: String
    public var permissions: [Entry]
    public var identity: Identity
}

/// 身份的呈现计划：说清"为什么看起来两处不一致"，以及重新授权会发生什么。
public struct PermissionPlan: Sendable, Equatable, Identifiable {
    public var id: String { bundleIdentifier }
    public var headline: String
    public var explanation: String
    /// nil 表示这个身份不支持/不需要重新授权（不给一个点了没用的按钮）。
    public var reauthorize: ReauthorizePlan?
    public var bundleIdentifier: String

    public init(
        headline: String, explanation: String, reauthorize: ReauthorizePlan?,
        bundleIdentifier: String
    ) {
        self.headline = headline
        self.explanation = explanation
        self.reauthorize = reauthorize
        self.bundleIdentifier = bundleIdentifier
    }
}

public struct ReauthorizePlan: Sendable, Equatable, Identifiable {
    public var id: String { bundleIdentifier }
    public var summary: String
    /// 这次会 `tccutil reset` 的服务（空表示没有可清除的记录）。
    public var services: [String]
    /// 清除之后需要重新请求的权限 id。
    public var reRequest: [String]
    public var bundleIdentifier: String

    public init(
        summary: String, services: [String], reRequest: [String], bundleIdentifier: String
    ) {
        self.summary = summary
        self.services = services
        self.reRequest = reRequest
        self.bundleIdentifier = bundleIdentifier
    }
}
