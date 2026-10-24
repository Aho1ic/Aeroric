#if DEBUG
import Observation

@MainActor @Observable
final class NativeUsageStatisticsReviewStore {
    private(set) var presetID = "US1/baseline"
    private(set) var context = USReviewContext(scope: .task, period: .day, zone: .utc)
    var dimension: USReviewDimension = .executor
    var showConversion = false
    var classic = false
    var demoTransparency = false
    var demoContrast = false
    var demoMotion = false
    private(set) var records = NativeUsageStatisticsReviewFixtures.corpus()
    private(set) var dataRevision = 0
    private(set) var eventIDs: Set<String> = []
    private(set) var revisionHistory = ["A-r1 · F 1400 Token，输入1000含缓存200、输出400"]
    private(set) var conflictCandidates: [String] = []
    private(set) var successfulAsOf: String? = NativeUsageStatisticsReviewFixtures.initialAsOf
    private(set) var refreshFailed = false
    private(set) var hasSuccessfulReport = true
    private var prices: [USReviewContext: USReviewPrice] = [:]
    var price: USReviewPrice { prices[context] ?? NativeUsageStatisticsReviewFixtures.v1 }
    private(set) var priceDraft: USReviewPriceDraft?
    private(set) var reportedFCost: USReviewMoney?
    private(set) var snapshots: [USReviewSnapshot] = []
    private(set) var currentConfiguration = "Codex / API-A / Model-A"
    private(set) var notStarted = false
    private(set) var role: USReviewRole = .primary
    private(set) var preauthorized = false
    private(set) var scopeRequest = false
    private(set) var preview: USReviewPreview?
    private(set) var exportIntents: [USReviewExportIntent] = []
    var includeSafeLabel = false
    private(set) var cleanupDraft: USReviewCleanupDraft?
    private(set) var removedDetailIDs: Set<String> = []
    private var removedSummaries: [USReviewContext: Set<String>] = [:]
    var removedSummaryIDs: Set<String> { removedSummaries[context] ?? [] }
    private(set) var cleanupResult = "尚未清理；未配置自动到期或默认留存天数"
    private(set) var resourcePreset: USReviewResourcePreset = .sharedCap
    private(set) var projectLimit = 3600
    private(set) var quotaDraft: USReviewQuotaDraft?
    private(set) var execution: USReviewExecution = .stopped
    private(set) var continuePolicy: USReviewContinuePolicy = .manual
    private(set) var continuationBlocks: Set<USReviewContinuationBlock> = [.userStop]
    private(set) var continueIntent = false
    private(set) var topUpContinueIntent = false
    private(set) var admissionRechecked = false
    private(set) var actionMessage = "只在本窗口表达意图或注入标明的虚构事件"
    let protectedFacts = USReviewProtectedFacts()

    init() { createSnapshot() }

    var preset: USReviewPreset {
        NativeUsageStatisticsReviewFixtures.presets.first { $0.id == presetID }
            ?? NativeUsageStatisticsReviewFixtures.presets[0]
    }

    private var selectedRecords: [USReviewUsage] {
        records.filter { $0.isInScope(context.scope) && $0.occurrence.belongs(to: context) }
    }
    private var aggregateRecords: [USReviewUsage] {
        selectedRecords.filter { !removedSummaryIDs.contains($0.id) }
    }
    var visibleDetails: [USReviewUsage] {
        guard hasSuccessfulReport else { return [] }
        return selectedRecords.filter { !removedDetailIDs.contains($0.id) }
    }
    var fRecord: USReviewUsage? { records.first { $0.id == "F" } }

    var summary: USReviewSummary {
        let values = aggregateRecords.compactMap(\.tokens.value)
        let gap = selectedRecords.contains { removedSummaryIDs.contains($0.id) }
        let f = aggregateRecords.first { $0.id == "F" }
        var missing = aggregateRecords.filter { $0.tokens.value == nil }.map { "\($0.id)：\($0.relation)" }
        if !hasSuccessfulReport { missing.append("尚未取得成功报告，不以零代替") }
        if gap { missing.append("该范围历史汇总已移除，存在断点；不是零") }
        if context.period != .completeInterval { missing.append("跨期累计 X 缺拆分，未分配到此期间") }
        if context.scope == .task { missing.append("S 共享 500 Token 仅关联，未分摊，不计入任务") }
        let input = f?.input
        let cache = f?.cacheRead
        return .init(
            knownTokens: hasSuccessfulReport && !values.isEmpty ? values.reduce(0, +) : nil,
            recordCount: hasSuccessfulReport ? values.count : 0,
            freshInput: hasSuccessfulReport ? input.flatMap { i in cache.map { i - $0 } } : nil,
            cacheRead: hasSuccessfulReport ? cache : nil,
            output: hasSuccessfulReport ? f?.output : nil,
            missing: missing, coverage: "仅所选范围/期间的已知部分，非完整账目；未知不补 0",
            hasSummaryGap: gap
        )
    }

    var breakdown: [USReviewBreakdown] {
        guard hasSuccessfulReport else { return [] }
        var sums: [String: Int] = [:]
        for record in aggregateRecords {
            if let value = record.tokens.value {
                sums[record.dimension(dimension), default: 0] += value
            }
        }
        return sums.map { .init(title: $0.key, tokens: $0.value) }
            .sorted { $0.tokens == $1.tokens ? $0.title < $1.title : $0.tokens > $1.tokens }
    }

    var trend: [USReviewTrendBar] {
        if context.scope == .task, context.period == .day, context.zone == .utc {
            let s = summary
            var parts: [Int]?
            if let total = s.knownTokens, !s.hasSummaryGap {
                let fresh = s.freshInput ?? 0, output = s.output ?? 0, cache = s.cacheRead ?? 0
                parts = [fresh, output, cache, total - fresh - output - cache]
            }
            return [
                .init(id: "missing-07", label: "09.07–10", parts: nil, explanation: "缺数据，不是 0"),
                .init(id: "adjacent-11", label: "09.11", parts: [0, 0, 0, 2200],
                      explanation: "邻日独立汇总样例，未提供分项；不计入所选日卡片"),
                .init(id: "selected", label: "09.12", parts: parts,
                      explanation: s.hasSummaryGap ? "所选日存在历史汇总断点" : "所选日，仅 F 有完整输入/输出/缓存拆分"),
                .init(id: "adjacent-13", label: "09.13", parts: [0, 0, 0, 1500],
                      explanation: "邻日独立汇总样例，未提供分项；不计入所选日卡片"),
            ]
        }
        return breakdown.map {
            .init(id: $0.id, label: $0.title, parts: [0, 0, 0, $0.tokens],
                  explanation: "同批唯一 usageID 的所选期间单一切面，非相邻日趋势")
        }
    }

    var costs: USReviewCosts {
        guard hasSuccessfulReport else {
            return .init(amounts: [], reported: [], conversionUSD: nil,
                         limitations: ["尚未取得报告，费用未知，不是免费"],
                         subscription: "示例订阅 · 实际扣款/套餐抵扣/余额未知")
        }
        let ids = Set(aggregateRecords.map(\.id))
        var amounts: [USReviewMoney] = []
        if let f = aggregateRecords.first(where: { $0.id == "F" }), let estimate = price.estimate(f) {
            amounts.append(.init(usageID: "F", microUnits: estimate, currency: "USD",
                                 basis: .estimated, source: price.version + " · API-A / Model-A / F"))
        }
        if ids.contains("V") {
            amounts.append(.init(usageID: "V", microUnits: 3000, currency: "EUR",
                                 basis: .estimated, source: "示例价 V-v1 · API-B / Model-B"))
        }
        let reported = ids.contains("F") ? reportedFCost.map { [$0] } ?? [] : []
        let conversion = showConversion && !amounts.isEmpty
            ? amounts.reduce(0) { $0 + ($1.currency == "EUR" ? $1.microUnits * 110 / 100 : $1.microUnits) }
            : nil
        return .init(
            amounts: amounts, reported: reported, conversionUSD: conversion,
            limitations: ["缺价 / 必要分项 / 币种的费用仍未知；不套近似价",
                          "原币不直接相加；折算仅覆盖 F/V，固定示例 EUR→USD 1.10 · 2026-09-12",
                          "Token 已报告不使估算费用变成已报告或已核对；未知币种不造符号"],
            subscription: "示例订阅 · 月费、上游额度与按量参考分开；扣款/覆盖/余额/重置时间未知"
        )
    }

    var sourceRows: [String] {
        var rows = [
            summary.coverage,
            "Z：\(USReviewFormat.number(NativeUsageStatisticsReviewFixtures.zero.value)) Token · \(NativeUsageStatisticsReviewFixtures.zero.basis.rawValue)；\(NativeUsageStatisticsReviewFixtures.zero.coverage)",
            "五身份 Codex / Claude Code / OMP / DSH / custom 保留；Aeroric 是控制面，不是第五执行器",
        ]
        if removedDetailIDs.contains("F") {
            rows.append("F 统计明细已清，不再逐次追溯；保留汇总不重建明细")
        } else if let f = fRecord, hasSuccessfulReport {
            rows += [
                "F：\(USReviewFormat.number(f.tokens.value)) Token · \(f.tokens.basis.rawValue) · \(f.revision)",
                "F 输入 \(USReviewFormat.number(f.input)) 已含缓存 \(USReviewFormat.number(f.cacheRead))；新增输入 + 缓存读取 + 输出 = 总 Token",
                "\(f.occurrence.label)；收到时间单列，不挤入发生日",
            ]
        }
        rows += costs.limitations + conflictCandidates
        return rows
    }

    var freshness: String {
        if refreshFailed {
            return successfulAsOf.map { "刷新失败 · 保留旧成功值及截至 \($0)，未假更新" }
                ?? "刷新失败 · 尚无成功值，未知不是 0"
        }
        return successfulAsOf.map { "成功截至 \($0) · 固定虚构示例" } ?? "尚未取得成功报告"
    }
    var timeRows: [String] {
        [
            "显示期间：\(context.title)；原始发生依据与控制周期不变",
            "F：09.12 23:30 UTC → UTC+08 为 09.13 07:30；UTC−07 为 09.12 16:30",
            "可靠发生归期，收到时间单列；仅日期精度不造秒级时间",
            "X：09.11 → 09.13 跨期累计，缺拆分；相交不取全额或猜比例，完整包含才可作为已知部分",
            "并行角色各 600 秒：累计 1,200 秒（20 分钟），实际经过 600 秒（10 分钟）",
            "缺可靠区间 / 跨 Host 时钟不可比：保留观察缺口；不由 PTY 静默或进程存活猜推理时间",
        ]
    }
    var timeProjection: USReviewTimeProjection {
        .init(cumulativeSeconds: 1200, elapsedSeconds: 600, interval: context.interval,
              unallocatedRecordIDs: records.filter {
                  $0.isInScope(context.scope) && !$0.occurrence.belongs(to: context)
              }.map(\.id),
              limitations: ["累计执行时间不是墙钟经过时间", "跨 Host 时钟不可比与缺可靠区间不猜补"])
    }

    func selectScope(_ value: USReviewScope) { changeContext(scope: value) }
    func selectPeriod(_ value: USReviewPeriod) { changeContext(period: value) }
    func selectZone(_ value: USReviewZone) { changeContext(zone: value) }
    private func changeContext(scope: USReviewScope? = nil, period: USReviewPeriod? = nil,
                               zone: USReviewZone? = nil) {
        let next = USReviewContext(scope: scope ?? context.scope, period: period ?? context.period,
                                   zone: zone ?? context.zone)
        guard next != context else { return }
        context = next
        invalidateDrafts()
    }
    private func invalidateDrafts() {
        priceDraft = nil; cleanupDraft = nil; quotaDraft = nil
    }

    func receiveDuplicate() {
        guard eventIDs.insert("E-duplicate-F").inserted else { return }
        actionMessage = "同 usageID F / A-r1 重复到达，已去重，不新增使用"
    }
    func receiveRetry() { insertUsage(NativeUsageStatisticsReviewFixtures.retry, eventID: "E-retry") }
    func receiveNewReport() { insertUsage(NativeUsageStatisticsReviewFixtures.newReport, eventID: "E-new") }
    private func insertUsage(_ record: USReviewUsage, eventID: String) {
        guard eventIDs.insert(eventID).inserted, !records.contains(where: { $0.id == record.id }) else { return }
        records.append(record)
        dataRevision += 1; invalidateDrafts(); admissionRechecked = false
        actionMessage = "显式模拟已发生 \(record.id)；只新增该记录，不执行重试或恢复历史明细"
    }
    func changeCurrentConfiguration() {
        currentConfiguration = "custom / demo-current-API / demo-current-model"
        actionMessage = "只变当前配置示例；旧使用保留发生时身份"
    }
    func beginPriceDraft() {
        guard let f = fRecord, selectedRecords.contains(where: { $0.id == "F" }),
              !removedSummaryIDs.contains("F") else { return }
        priceDraft = .init(context: context, revision: dataRevision, usageRevision: f.revision,
                           price: NativeUsageStatisticsReviewFixtures.customPrice,
                           preview: NativeUsageStatisticsReviewFixtures.customPrice.estimate(f))
    }
    func cancelPriceDraft() { priceDraft = nil }
    func applyPriceDraft() {
        guard let draft = priceDraft, draft.context == context, draft.revision == dataRevision,
              draft.usageRevision == fRecord?.revision else { priceDraft = nil; return }
        prices[context] = draft.price; priceDraft = nil
        actionMessage = "只应用 F / API-A / Model-A 当前期间的自定义估算；不改报告、快照或控制"
    }
    func receiveLateCost() {
        guard eventIDs.insert("E-late-F-cost").inserted else { return }
        reportedFCost = .init(usageID: "F", microUnits: 4800, currency: "USD", basis: .reported,
                             source: "迟到费用报告 C-r2 · 2026-09-13 03:00 UTC · 未核对")
        successfulAsOf = "2026-09-13 03:00 UTC"; refreshFailed = false
        revisionHistory.append("C-r2：同笔 F 补费用报告 USD 0.0048，未核对；不增 Token")
        dataRevision += 1; invalidateDrafts()
    }
    func reviseOutput(up: Bool) {
        let revision = up ? "A-r2" : "A-r2d"
        guard eventIDs.insert("E-\(revision)").inserted,
              let index = records.firstIndex(where: { $0.id == "F" }) else { return }
        let output = up ? 500 : 300
        records[index].output = output
        records[index].tokens.value = 1000 + output
        records[index].revision = revision
        records[index].tokens.source = revision
        revisionHistory.append("\(revision)：同 usageID F 有据\(up ? "上调" : "下调")，输出 \(output)，非新调用")
        successfulAsOf = "2026-09-13 04:00 UTC"; refreshFailed = false
        dataRevision += 1; invalidateDrafts(); admissionRechecked = false
    }
    func receiveConflict() {
        guard eventIDs.insert("E-conflict").inserted else { return }
        conflictCandidates = ["候选 A-conflict：1700 Token，关联/口径不清；原 A-r1 等依据保留，未覆盖、未求和、待核对"]
        actionMessage = "来源冲突只记录候选；后到不自动更真，未升级任何核对状态"
    }
    func refresh() {
        actionMessage = "普通刷新只查看现有内存；没有新成功事件，不改截至、不复活明细、不执行任务"
    }
    func receiveRefreshFailure() { refreshFailed = true }

    func createSnapshot() {
        let retainedF = hasSuccessfulReport && !removedDetailIDs.contains("F")
            && aggregateRecords.contains { $0.id == "F" }
        snapshots.append(.init(id: "S\(snapshots.count)", context: context, asOf: successfulAsOf,
                               summary: summary, fTokens: retainedF ? fRecord?.tokens.value : nil,
                               fRevision: retainedF ? fRecord?.revision ?? "未知"
                                   : hasSuccessfulReport ? "该范围明细/汇总不可用，未重建" : "尚无成功报告，修订未知",
                               costs: costs, price: price))
    }

    private var roleContext: USReviewContext {
        .init(scope: preauthorized ? .project : .task, period: context.period, zone: context.zone)
    }
    private var roleRecords: [USReviewUsage] {
        // Explicit grants, not userScope and never a subtraction from the user's global total.
        guard hasSuccessfulReport else { return [] }
        let allowed: Set<String>
        if preauthorized {
            allowed = ["F", "R", "V", "D", "S"]
        } else {
            switch role {
            case .primary: allowed = ["F", "R", "V", "D", "E-retry", "E-new"]
            case .reflection: allowed = ["V"]
            case .delegation: allowed = ["D"]
            case .governance: allowed = []
            }
        }
        // A user scope switch cannot restore or enlarge the role's own retained summary.
        let removed = removedSummaries[roleContext] ?? []
        return records.filter {
            allowed.contains($0.id) && !removed.contains($0.id) && $0.occurrence.belongs(to: roleContext)
        }
    }
    var roleSummary: USReviewRoleSummary {
        let visible = roleRecords
        let values = visible.compactMap(\.tokens.value)
        let total = hasSuccessfulReport && !values.isEmpty ? values.reduce(0, +) : nil
        let rows: [String] = role == .governance && !preauthorized
            ? ["共享资源受阻；需处理授权范围缺口，不提供他项名称、全局数或可推算差额"]
            : ["\(role.title)获准必要摘要 · 已知部分 \(USReviewFormat.number(total)) Token",
               "只有明确预置授权集合内的统计汇总，不提供原始记录或全局差额"]
        return .init(role: role, knownTokens: total, rows: rows,
                     coverage: "角色授权范围内的已知部分；覆盖不完整，未知不是零",
                     authorization: preauthorized ? "固定预置的有效跨任务授权覆盖；不重复索同一许可，非新授予"
                        : "任务 Full Access 不自动授全账目、外发或改预算权限")
    }
    func selectRole(_ value: USReviewRole) {
        guard value != role else { return }
        role = value; closePreview(); scopeRequest = false
    }
    func loadPreauthorizedScope(_ value: Bool) {
        preauthorized = value; closePreview(); scopeRequest = false
    }
    func requestBroaderScope() {
        scopeRequest = true
        actionMessage = "仅记录越界读取请求意图，未授新权限，不显示受限名称或差额"
    }
    func createPreview() {
        let r = roleSummary
        let granted = roleRecords
        let rolePrice = prices[roleContext] ?? NativeUsageStatisticsReviewFixtures.v1
        var originalCurrency: [String] = []
        var sources = granted.map(\.tokens.source)
        var revisions = granted.map(\.revision)
        if let f = granted.first(where: { $0.id == "F" }), let estimate = rolePrice.estimate(f) {
            let priceBasis = rolePrice == NativeUsageStatisticsReviewFixtures.v1
                ? "示例价 v1 · 2026-09-01" : "自定义示例价"
            originalCurrency.append("USD \(USReviewFormat.money(estimate)) · 估算")
            sources.append("\(priceBasis) · USD/1M \(rolePrice.freshPerMillion)/\(rolePrice.cachePerMillion)/\(rolePrice.outputPerMillion)")
            if let report = reportedFCost {
                originalCurrency.append(report.display + " · 已报告，未核对")
                sources.append(report.source)
                revisions.append("C-r2")
            }
        }
        if granted.contains(where: { $0.id == "V" }) {
            originalCurrency.append("EUR 0.0030 · 估算")
            sources.append("示例价 V-v1 · 2026-09-01")
        }
        let currencies = originalCurrency.isEmpty ? "尚未取得获准费用依据，不是零" : originalCurrency.joined(separator: "；")
        let sourceBasis = sources.isEmpty ? "尚无获准来源，不回显受限依据" : Array(Set(sources)).sorted().joined(separator: "；")
        let revisionBasis = revisions.isEmpty ? "尚无获准修订" : Array(Set(revisions)).sorted().joined(separator: " / ")
        let fields = ["指标", "期间", "原币", "来源", "覆盖", "修订口径"] + (includeSafeLabel ? ["安全角色标签"] : [])
        preview = .init(role: role, fields: fields, lines: [
            "指标：\(USReviewFormat.number(r.knownTokens)) Token",
            "期间：\(context.title)", "原币：\(currencies)；仅有据部分，不合成账单",
            "来源：\(sourceBasis)；固定虚构依据，报告/估算不互相升级",
            "覆盖：\(r.coverage)；费用仅获准可算部分，其余未知",
            "修订口径：\(revisionBasis)；创建时的值拷贝，未含原始记录",
        ] + (includeSafeLabel ? ["安全角色标签：\(role.title)"] : []))
        exportIntents = []
    }
    func expressExportIntent(_ intent: USReviewExportIntent) {
        guard preview != nil else { return }
        if !exportIntents.contains(intent) { exportIntents.append(intent) }
    }
    func closePreview() { preview = nil; exportIntents = [] }

    func beginCleanup(_ layer: USReviewCleanupLayer) {
        let ids = Set(selectedRecords.filter {
            layer == .details ? !removedDetailIDs.contains($0.id) : !removedSummaryIDs.contains($0.id)
        }.map(\.id))
        cleanupDraft = .init(context: context, revision: dataRevision, layer: layer, recordIDs: ids,
                             impact: layer == .protectedRecords
                                ? "必要控制/核对记录不可清；成果、任务、原会话、监督/验收、预算、重试与 Stop 保留"
                                : "\(context.scope.title) · \(context.title) · \(layer.title) · \(ids.count) 条内存记录；不影响源/磁盘/备份/外部副本")
    }
    func cancelCleanup() { cleanupDraft = nil }
    func applyCleanup() {
        guard let draft = cleanupDraft, draft.context == context, draft.revision == dataRevision else {
            cleanupDraft = nil; return
        }
        switch draft.layer {
        case .details:
            removedDetailIDs.formUnion(draft.recordIDs)
            cleanupResult = "仅指定内存统计明细已清，汇总保留；不能逐次追溯，普通刷新不重建"
        case .summary:
            removedSummaries[draft.context, default: []].formUnion(draft.recordIDs)
            cleanupResult = "仅指定范围/期间汇总已移除，显示历史断点；不是 0，不补连续曲线"
        case .protectedRecords:
            cleanupResult = "必要控制/核对记录受保护，未清理；全部工作依据与 Stop 保留"
        }
        cleanupDraft = nil; dataRevision += 1; invalidateDrafts()
    }

    var resourceProjection: USReviewResourceProjection {
        let projectKnown = resourcePreset == .displayUnknown || resourcePreset == .requiredUnknown ? nil : 3600
        var reasons: [String] = []
        switch resourcePreset {
        case .sharedCap:
            if projectLimit <= 3600 { reasons.append("项目 P-A 共享上限已到；任务局部有余不解除该限制") }
        case .warning, .displayUnknown, .recovered: break
        case .requiredUnknown: reasons.append("必需准入/硬控制计量未知；不是已花完")
        case .concurrency: reasons.append("并发 2 / 2：仅拦新准入，合法在途不为腾名额强停")
        case .capability: reasons.append("必需硬限能力不足，不能用提示词伪装硬控制")
        }
        if execution == .stopRequested || execution == .winding || execution == .unknown {
            reasons.append("原执行未核实安全收束，禁止重叠替代写入")
        }
        return .init(
            projectKnown: projectKnown, projectLimit: projectLimit, taskKnown: projectKnown == nil ? nil : 3100,
            taskLimit: 5000, newAdmissionBlocked: !reasons.isEmpty, reasons: reasons,
            independentWork: "仅命中适用范围；其他独立安全工作不泛化为全部停止",
            basis: "\(protectedFacts.resourceCycle) · Token · 固定完整控制计量预设，非上方不完整展示统计；\(resourcePreset == .warning ? "预警阈值 3500 为固定示例，非产品默认；" : "")未设置其他某类上限不表示无限；币种/单位/周期不混算",
            execution: execution, verification: protectedFacts.supervision + " / " + protectedFacts.acceptance
        )
    }
    func selectResourcePreset(_ value: USReviewResourcePreset) {
        resourcePreset = value; quotaDraft = nil; admissionRechecked = false
        projectLimit = value == .recovered || value == .warning || value == .displayUnknown ? 4500 : 3600
        execution = value == .concurrency || value == .warning ? .running : .stopped
    }
    func simulateLimitRequest() { execution = .stopRequested; admissionRechecked = false }
    func simulateWinding() {
        guard execution == .stopRequested else { return }
        execution = .winding
    }
    func simulateStopVerification(known: Bool) {
        guard execution == .stopRequested || execution == .winding else { return }
        execution = known ? .stopped : .unknown; admissionRechecked = false
    }
    func beginQuotaDraft() {
        guard context.scope != .global else { return }
        quotaDraft = .init(context: context, revision: dataRevision, project: "P-A",
                           previousLimit: projectLimit, proposedLimit: 4500)
    }
    func cancelQuotaDraft() { quotaDraft = nil }
    func applyQuotaDraft() {
        guard let draft = quotaDraft, draft.context == context, draft.revision == dataRevision,
              draft.previousLimit == projectLimit else { quotaDraft = nil; return }
        projectLimit = draft.proposedLimit; quotaDraft = nil; admissionRechecked = false
        actionMessage = "仅修改项目 P-A 示例策略；未重启、未恢复 Stop、未清重试、未升级监督或验收"
    }
    func expressContinue(topUp: Bool = false) {
        if topUp { topUpContinueIntent = true; beginQuotaDraft() }
        else { continueIntent = true }
        admissionRechecked = false
        actionMessage = topUp ? "补额草稿 + 继续意图均未执行；仍需完整重核" : "仅继续意图；无新执行、调额或重新授权"
    }
    func loadContinuationFixture(policy: USReviewContinuePolicy, block: USReviewContinuationBlock?) {
        continuePolicy = policy; continuationBlocks = block.map { [$0] } ?? []
        resourcePreset = .recovered; projectLimit = 4500; execution = .stopped
        continueIntent = false; topUpContinueIntent = false; admissionRechecked = false; quotaDraft = nil
        actionMessage = "载入明确的虚构重核条件，非更改真实 Stop 或授权；工作依据锚点仍原样"
    }
    var continuationReasons: [String] {
        var blocks = continuationBlocks
        if continuePolicy == .manual && !continueIntent && !topUpContinueIntent { blocks.insert(.missingContinue) }
        if execution != .stopped { blocks.insert(.originalExecution) }
        return USReviewContinuationBlock.allCases.filter { blocks.contains($0) }.map(\.title)
            + resourceProjection.reasons
    }
    var admissionCanContinue: Bool { admissionRechecked && continuationReasons.isEmpty }
    func recheckAdmission() {
        admissionRechecked = true
        actionMessage = continuationReasons.isEmpty
            ? "尚需完成工作：样例准入可继续（未执行）；没有重放已完成/未知操作，最终验收仍待完成"
            : "完整模拟重核未通过；具体原因逐项保留，不因最近一条意图覆盖"
    }

    func reset() { loadPreset("US1/baseline") }
    func loadPreset(_ id: String) {
        guard NativeUsageStatisticsReviewFixtures.presets.contains(where: { $0.id == id }) else { return }
        presetID = id; context = .init(scope: .task, period: .day, zone: .utc)
        dimension = .executor; showConversion = false
        classic = false; demoTransparency = false; demoContrast = false; demoMotion = false
        records = NativeUsageStatisticsReviewFixtures.corpus()
        dataRevision = 0; eventIDs = []; revisionHistory = ["A-r1 · F 1400 Token，输入1000含缓存200、输出400"]
        conflictCandidates = []; successfulAsOf = NativeUsageStatisticsReviewFixtures.initialAsOf
        refreshFailed = false; hasSuccessfulReport = true
        prices = [:]; priceDraft = nil; reportedFCost = nil
        snapshots = []; currentConfiguration = "Codex / API-A / Model-A"; notStarted = false
        role = .primary; preauthorized = false; scopeRequest = false
        closePreview(); includeSafeLabel = false; cleanupDraft = nil; removedDetailIDs = []; removedSummaries = [:]
        cleanupResult = "尚未清理；未配置自动到期或默认留存天数"
        resourcePreset = .sharedCap; projectLimit = 3600; quotaDraft = nil; execution = .stopped
        continuePolicy = .manual; continuationBlocks = [.userStop]
        continueIntent = false; topUpContinueIntent = false; admissionRechecked = false
        actionMessage = "已载入固定虚构预设，已丢弃局部草稿、预览、事件与修改"
        createSnapshot()
        switch id {
        case "US1/duplicate": receiveDuplicate()
        case "US1/retry": receiveRetry()
        case "US1/not-started": notStarted = true; changeCurrentConfiguration()
        case "US2/price-draft": beginPriceDraft()
        case "US2/price-cancelled": beginPriceDraft(); cancelPriceDraft()
        case "US2/price-applied": beginPriceDraft(); applyPriceDraft()
        case "US2/conversion": showConversion = true
        case "US2/conflict", "US3/conflict": receiveConflict()
        case "US3/late", "US3/new-snapshot-late":
            receiveLateCost(); if id.contains("new-snapshot") { createSnapshot() }
        case "US3/revise-up", "US3/new-snapshot-up":
            reviseOutput(up: true); if id.contains("new-snapshot") { createSnapshot() }
        case "US3/revise-down", "US3/new-snapshot-down":
            reviseOutput(up: false); if id.contains("new-snapshot") { createSnapshot() }
        case "US3/failure-stale": receiveRefreshFailure()
        case "US3/failure-empty":
            hasSuccessfulReport = false; successfulAsOf = nil; snapshots = []; receiveRefreshFailure()
        case "US3/time-windows": records.append(NativeUsageStatisticsReviewFixtures.crossPeriod)
        case "US4/user": selectScope(.global)
        case "US4/roles": selectRole(.governance)
        case "US4/preauthorized": selectRole(.governance); loadPreauthorizedScope(true)
        case "US4/blocked": selectRole(.governance); requestBroaderScope()
        case "US4/export-preview": createPreview()
        case "US4/file-intent": createPreview(); expressExportIntent(.file)
        case "US4/send-intent": createPreview(); expressExportIntent(.external)
        case "US4/closed": createPreview(); closePreview()
        case "US5/draft-details": beginCleanup(.details)
        case "US5/draft-project-period": selectScope(.project); beginCleanup(.details)
        case "US5/project-removed": selectScope(.project); beginCleanup(.details); applyCleanup()
        case "US5/project-cancelled": selectScope(.project); beginCleanup(.details); cancelCleanup()
        case "US5/draft-summary": beginCleanup(.summary)
        case "US5/cancelled": beginCleanup(.details); cancelCleanup()
        case "US5/details-removed", "US5/draft-summary-after-details", "US5/cancel-summary-after-details",
             "US5/summary-gap-after-details", "US5/refreshed", "US5/new-report":
            beginCleanup(.details); applyCleanup()
            if id.contains("summary") {
                beginCleanup(.summary)
                if id == "US5/cancel-summary-after-details" { cancelCleanup() }
                if id == "US5/summary-gap-after-details" { applyCleanup() }
            }
            if id == "US5/refreshed" { refresh() }
            if id == "US5/new-report" { receiveNewReport() }
        case "US5/summary-gap": beginCleanup(.summary); applyCleanup()
        case "US5/protected": beginCleanup(.protectedRecords)
        case "US6/warning": selectResourcePreset(.warning)
        case "US6/unknown": selectResourcePreset(.displayUnknown)
        case "US6/concurrency": selectResourcePreset(.concurrency)
        case "US6/limit-request": simulateLimitRequest()
        case "US6/winding": simulateLimitRequest(); simulateWinding()
        case "US6/stopped": simulateLimitRequest(); simulateWinding(); simulateStopVerification(known: true)
        case "US6/stop-unknown": simulateLimitRequest(); simulateWinding(); simulateStopVerification(known: false)
        case "US6/capability": selectResourcePreset(.capability)
        case "US6/quota-draft": beginQuotaDraft()
        case "US6/quota-applied": beginQuotaDraft(); applyQuotaDraft()
        case "US6/continue-intent": expressContinue()
        case "US6/topup-intent": expressContinue(topUp: true)
        case "US6/auto-success":
            loadContinuationFixture(policy: .automatic, block: nil); recheckAdmission()
        case "US6/auto-blocked":
            loadContinuationFixture(policy: .automatic, block: .userStop); recheckAdmission()
        default: break
        }
    }
}
#endif
