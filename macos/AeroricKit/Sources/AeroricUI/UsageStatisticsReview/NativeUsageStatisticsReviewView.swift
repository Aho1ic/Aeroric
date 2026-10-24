#if DEBUG
import AppKit
import Charts
import SwiftUI

/// The approved Settings visual context, not the product Settings route.
public struct NativeUsageStatisticsReviewView: View {
    @State private var store = NativeUsageStatisticsReviewStore()
    @State private var reviewExpanded = false
    @State private var detailsExpanded = false
    @State private var sourcesExpanded = false
    @State private var timeExpanded = false
    @State private var visibilityExpanded = false
    @State private var retentionExpanded = false
    @State private var resourcesExpanded = false
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorSchemeContrast) private var contrast

    public init() {}

    private var presentation: USReviewPresentation {
        .resolve(classic: store.classic, systemTransparency: reduceTransparency,
                 systemContrast: contrast == .increased, systemMotion: reduceMotion,
                 demoTransparency: store.demoTransparency, demoContrast: store.demoContrast,
                 demoMotion: store.demoMotion)
    }
    private var surface: SurfacePreference { presentation.opaque ? .classic : .glass }
    private var technicalFont: Font {
        NSFont(name: "JetBrains Mono", size: 11) != nil
            ? .custom("JetBrains Mono", size: 11, relativeTo: .caption)
            : .system(.caption, design: .monospaced)
    }
    private let partNames = ["新增输入", "输出", "缓存读取", "未拆分"]
    private let partColors: [Color] = [.dsAccent, Color(nsColor: .systemPurple),
                                       Color(nsColor: .systemTeal), .dsTextMuted]

    public var body: some View {
        GeometryReader { geometry in
            HStack(spacing: NativePanelGeometry.panelGap) {
                if geometry.size.width >= 850 { settingsSidebar.frame(width: 196) }
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        heading
                        filters
                        metricCards(availableWidth: geometry.size.width - (geometry.size.width >= 850 ? 268 : 64))
                        chartPanel
                        disclosure("使用明细与归属", id: "details", expanded: $detailsExpanded) { details }
                        disclosure("来源与费用", id: "sources", expanded: $sourcesExpanded) { sources }
                        disclosure("报告与时间", id: "time", expanded: $timeExpanded) { timeDetails }
                        Text("统计设置").font(.headline).padding(.top, 8)
                        disclosure("可见范围与内存预览", id: "visibility", expanded: $visibilityExpanded) { visibility }
                        disclosure("数据留存与清理", id: "retention", expanded: $retentionExpanded) { retention }
                        disclosure("资源限制与执行核验", id: "resources", expanded: $resourcesExpanded) { resources }
                        disclosure("离线审阅控制", id: "review", expanded: $reviewExpanded) { reviewControls }
                    }
                    .padding(24)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .background(Color.dsCanvas, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            }
            .padding(NativePanelGeometry.panelInset)
        }
        .font(.system(size: 13))
        .foregroundStyle(Color.dsText)
        .background(NativePanelFoundation(preference: surface, reduceTransparency: presentation.opaque))
        .transaction {
            if presentation.disableAnimation { $0.animation = nil; $0.disablesAnimations = true }
        }
    }

    private var settingsSidebar: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("设置", systemImage: "gearshape").font(.title3.weight(.semibold)).padding(12)
            NativeSelectionList(selection: Binding<String?>(get: { "usage" }, set: { _ in })) {
                Label("通用", systemImage: "switch.2").tag("general").disabled(true)
                Label("外观", systemImage: "paintpalette").tag("appearance").disabled(true)
                Label("执行器", systemImage: "terminal").tag("agents").disabled(true)
                Label("使用统计", systemImage: "chart.bar.xaxis").tag("usage")
                    .accessibilityIdentifier(id("settings.usage"))
                Label("高级", systemImage: "slider.horizontal.3").tag("advanced").disabled(true)
            }
            .listStyle(.sidebar)
            note("设置视觉上下文；其他分类未接入。\n独立离线样例，不是正式设置窗口。", "settings.boundary")
                .padding(12)
        }
        .background(Color.dsSurface, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }

    private var heading: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("设置 / 使用统计").font(.caption).foregroundStyle(Color.dsTextSecondary)
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("使用统计").font(.system(size: 22, weight: .semibold))
                        .accessibilityIdentifier(id("title"))
                    Text("用量、来源与覆盖，按需查看。").foregroundStyle(Color.dsTextSecondary)
                }
                Spacer()
                NativeIconButton(systemImage: "arrow.clockwise", label: "刷新内存统计",
                                 identifier: id("refresh"), help: "只查看现有内存，不重执行或重建明细") { store.refresh() }
            }
            note(NativeUsageStatisticsReviewFixtures.notice, "boundary")
        }
    }

    private var filters: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) { scopePicker; Spacer(minLength: 8); periodPicker; zonePicker }
            VStack(alignment: .leading, spacing: 8) { scopePicker; HStack { periodPicker; zonePicker } }
        }
    }
    private var scopePicker: some View {
        NativeSegmentedPicker(
            selection: Binding(get: { store.context.scope }, set: { store.selectScope($0) }),
            groupTitle: "统计范围",
            segments: USReviewScope.allCases.map { .init(value: $0, title: LocalizedStringKey($0.title)) }
        ).frame(maxWidth: 340).accessibilityIdentifier(id("scope"))
    }
    private var periodPicker: some View {
        Picker("期间", selection: Binding(get: { store.context.period }, set: { store.selectPeriod($0) })) {
            ForEach(USReviewPeriod.allCases) { Text($0.title).tag($0) }
        }.pickerStyle(.menu).fixedSize().accessibilityIdentifier(id("period"))
    }
    private var zonePicker: some View {
        Picker("统计时区", selection: Binding(get: { store.context.zone }, set: { store.selectZone($0) })) {
            ForEach(USReviewZone.allCases) { Text($0.title).tag($0) }
        }.pickerStyle(.menu).fixedSize().accessibilityIdentifier(id("timezone"))
    }

    private func metricCards(availableWidth: CGFloat) -> some View {
        let s = store.summary
        let columns = availableWidth >= 740 ? 4 : availableWidth >= 380 ? 2 : 1
        return VStack(alignment: .leading, spacing: 8) {
            note("\(store.context.title) · \(s.coverage)", "coverage")
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), alignment: .top), count: columns), spacing: 8) {
                metric("已知 Token", USReviewFormat.number(s.knownTokens), "所选期间 · 覆盖不完整", "summary", "chart.bar")
                metric("新增输入", USReviewFormat.number(s.freshInput), "仅 F 有分项，非范围总输入", "input", "arrow.down")
                metric("输出", USReviewFormat.number(s.output), "仅 F 有分项，其余未知", "output", "arrow.up")
                metric("缓存读取", USReviewFormat.number(s.cacheRead), "已包含在 F 输入中，不重复加", "cache", "tray")
                metric("缓存创建", "未知", "未提供报告，不补 0", "cacheCreation", "questionmark")
                metric("整体缓存命中率", "未知", "缺完整分母，不推算百分比", "cacheRatio", "percent")
                metric("已知使用记录", USReviewFormat.number(s.knownTokens == nil ? nil : s.recordCount),
                       "usageID 去重，不是任务数", "records", "square.stack")
                metric("已定价部分", store.costs.amounts.map(\.display).joined(separator: "\n").nilIfEmpty ?? "未知",
                       "仅可算部分 · 估算非账单 · 原币分列", "costs", "creditcard", compact: true)
            }
        }
    }

    private func metric(_ title: String, _ value: String, _ detail: String, _ key: String,
                        _ icon: String, compact: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(title, systemImage: icon).font(.caption).foregroundStyle(Color.dsTextSecondary)
            Text(value)
                .font(.system(size: compact ? 18 : 27, weight: .medium, design: .rounded)).monospacedDigit()
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier(id("card.\(key).value"))
            Text(detail).font(.system(size: 10)).foregroundStyle(Color.dsTextSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(14).frame(maxWidth: .infinity, minHeight: 108, alignment: .topLeading)
        .background(Color.dsSurface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
            .strokeBorder(Color.dsBorder, lineWidth: presentation.strongContrast ? 1 : 0.5))
    }

    private var chartPanel: some View {
        panel {
            HStack {
                Text("构成与归属").font(.headline)
                Spacer()
                Picker("切面", selection: $store.dimension) {
                    ForEach(USReviewDimension.allCases) { Text($0.title).tag($0) }
                }.pickerStyle(.menu).frame(maxWidth: 170).accessibilityIdentifier(id("dimension"))
            }
            Chart {
                ForEach(store.trend) { bar in
                    if let parts = bar.parts {
                        ForEach(Array(parts.enumerated()), id: \.offset) { index, amount in
                            BarMark(x: .value("期间 / 切面", bar.label), y: .value("Token", amount), stacking: .standard)
                                .foregroundStyle(by: .value("构成", partNames[index]))
                                .accessibilityLabel("\(bar.label)，\(partNames[index])，\(amount) Token")
                        }
                    }
                }
            }
            .chartForegroundStyleScale(domain: partNames, range: partColors)
            .chartYAxisLabel("Token · 已知部分")
            .chartLegend(position: .bottom, alignment: .leading)
            .frame(height: 210)
            .accessibilityIdentifier(id("trend"))
            ForEach(store.trend) { bar in
                note("\(bar.label) · \(bar.total.map { USReviewFormat.number($0) + " Token" } ?? "—") · \(bar.explanation)", "trend.\(bar.id)")
            }
            Divider()
            Text("\(store.dimension.title)分布").fontWeight(.medium)
            ForEach(store.breakdown) { row in
                HStack(spacing: 10) {
                    Text(row.title).font(technicalFont).frame(width: 110, alignment: .leading)
                    ProgressView(value: Double(row.tokens), total: Double(max(store.breakdown.map(\.tokens).max() ?? 1, 1)))
                        .tint(Color.dsAccent)
                    Text(USReviewFormat.number(row.tokens)).font(.system(.body, design: .rounded)).monospacedDigit()
                        .frame(width: 62, alignment: .trailing)
                }.accessibilityIdentifier(id("breakdown.\(row.id)"))
            }
            note("横条相对本组最大项，不是费用占比。各切面是同一批已知部分，不再彼此相加。", "breakdown.basis")
            ForEach(Array(store.summary.missing.enumerated()), id: \.offset) { index, missing in
                note(missing, "missing.\(index)")
            }
        }
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: 12) {
            if store.visibleDetails.isEmpty {
                note("无可见统计明细；可能尚未取得或已清理。不是零，不从保留汇总重建。", "details.empty")
            }
            ForEach(store.visibleDetails) { usage in
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(usage.id) · \(usage.executor)").font(technicalFont)
                        Text(usage.relation).font(.caption).foregroundStyle(Color.dsTextSecondary)
                        Text("\(usage.api ?? "缺 API") / \(usage.model ?? "缺模型") · \(usage.revision)").font(technicalFont)
                    }
                    Spacer()
                    VStack(alignment: .trailing, spacing: 4) {
                        Text(USReviewFormat.number(usage.tokens.value)).monospacedDigit()
                        Text(usage.tokens.basis.rawValue).font(.caption).foregroundStyle(Color.dsTextSecondary)
                    }
                }.accessibilityIdentifier(id("detail.\(usage.id)"))
                Divider()
            }
            if store.notStarted {
                note("另一个被拒 / 未启动尝试：没有使用或费用记录，不将未知费用补零。历史 F/R 等仍保留。", "attempts.notStarted")
            }
            note("当前配置示例：\(store.currentConfiguration)；旧记录不重标。", "identity")
            HStack {
                action("模拟重复 F", "events.duplicate") { store.receiveDuplicate() }
                action("模拟独立已发生重试", "events.retry") { store.receiveRetry() }
            }
            action("切当前配置示例，不重标历史", "identity.change") { store.changeCurrentConfiguration() }
        }
    }

    private var sources: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(store.sourceRows.enumerated()), id: \.offset) { index, row in note(row, "source.\(index)") }
            ForEach(store.costs.amounts) { money in
                valueRow("参考 · \(money.usageID)", money.display + " · " + money.basis.rawValue + " · " + money.source)
            }
            ForEach(store.costs.reported) { money in
                valueRow("来源报告 · \(money.usageID)", money.display + " · " + money.basis.rawValue + " · 未核对 · " + money.source)
            }
            note("F 示例单价 USD / 1M：普通输入 \(store.price.freshPerMillion)、缓存 \(store.price.cachePerMillion)、输出 \(store.price.outputPerMillion)；\(store.price.version)", "price.basis")
            Toggle("显示有据部分的 USD 折算参考", isOn: $store.showConversion)
                .accessibilityIdentifier(id("currency"))
            if let value = store.costs.conversionUSD {
                valueRow("折算参考 · 非账单", "USD \(USReviewFormat.money(value)) · 仅有据部分，原币保留")
            }
            note(store.costs.subscription, "subscription")
            action("预览 F 自定义价格", "price.begin") { store.beginPriceDraft() }
            if let draft = store.priceDraft {
                note("\(draft.context.title) · F / API-A / Model-A / \(draft.usageRevision) · \(draft.price.version)", "price.scope")
                valueRow("草稿单价 · 虚构 USD / 1M",
                         "普通输入 \(draft.price.freshPerMillion) · 缓存 \(draft.price.cachePerMillion) · 输出 \(draft.price.outputPerMillion)")
                valueRow("草稿估算", draft.preview.map { "USD " + USReviewFormat.money($0) } ?? "未知，缺必要分项")
                HStack {
                    action("仅应用本范围参考", "price.apply") { store.applyPriceDraft() }
                    action("取消", "price.cancel") { store.cancelPriceDraft() }
                }
            }
            action("模拟冲突，不覆盖原依据", "events.conflict") { store.receiveConflict() }
        }
    }

    private var timeDetails: some View {
        VStack(alignment: .leading, spacing: 12) {
            note(store.freshness, "freshness")
            valueRow("并行累计 / 实际经过",
                     "\(store.timeProjection.cumulativeSeconds) 秒 / \(store.timeProjection.elapsedSeconds) 秒")
            note("未分配到当前期间的记录：\(store.timeProjection.unallocatedRecordIDs.joined(separator: "、"))；\(store.timeProjection.limitations.joined(separator: "；"))", "time.coverage")
            ForEach(Array(store.timeRows.enumerated()), id: \.offset) { index, row in note(row, "time.\(index)") }
            HStack {
                action("模拟迟到费用报告", "events.late") { store.receiveLateCost() }
                action("模拟刷新失败", "events.refreshFailure") { store.receiveRefreshFailure() }
            }
            HStack {
                action("模拟有据上调", "events.up") { store.reviseOutput(up: true) }
                action("模拟有据下调", "events.down") { store.reviseOutput(up: false) }
            }
            ForEach(Array(store.revisionHistory.enumerated()), id: \.offset) { index, revision in
                note(revision, "revision.\(index)")
            }
            action("创建新内存快照", "snapshot.create") { store.createSnapshot() }
            ForEach(store.snapshots) { snapshot in
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(snapshot.id) · \(snapshot.context.title)").font(technicalFont)
                    Text("F \(USReviewFormat.number(snapshot.fTokens)) Token · \(snapshot.fRevision)")
                    ForEach(snapshot.costs.amounts) { money in
                        Text("估算参考：\(money.display) · \(money.source)")
                    }
                    ForEach(snapshot.costs.reported) { money in
                        Text("来源报告：\(money.display) · 已报告，未核对 · \(money.source)")
                    }
                    if snapshot.costs.amounts.isEmpty && snapshot.costs.reported.isEmpty {
                        Text("费用尚未取得；不是零").foregroundStyle(Color.dsTextSecondary)
                    }
                    note("截至 \(snapshot.asOf ?? "尚无成功值") · \(snapshot.price.version)；旧快照值拷贝不暗改", "snapshot.\(snapshot.id)")
                }
            }
        }
    }

    private var visibility: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("角色上下文（不随用户筛选扩大）", selection: Binding(get: { store.role }, set: { store.selectRole($0) })) {
                ForEach(USReviewRole.allCases) { Text($0.title).tag($0) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("role"))
            ForEach(Array(store.roleSummary.rows.enumerated()), id: \.offset) { index, row in note(row, "roleSummary.\(index)") }
            note(store.roleSummary.coverage, "role.coverage")
            note(store.roleSummary.authorization, "role.authorization")
            Toggle("载入固定已有授权覆盖（不是授新权限）", isOn: Binding(
                get: { store.preauthorized }, set: { store.loadPreauthorizedScope($0) }
            )).accessibilityIdentifier(id("scope.fixture"))
            action("记录范围请求意图", "scope.request") { store.requestBroaderScope() }
            if store.scopeRequest { note("范围缺口仍在；仅请求，未授新权，不回显受限名称或可推算差额。", "scope.gap") }
            Toggle("预览包含安全角色标签", isOn: $store.includeSafeLabel).accessibilityIdentifier(id("export.fields"))
            action("创建最小内存预览", "export.preview") { store.createPreview() }
            if let preview = store.preview {
                Text("字段：" + preview.fields.joined(separator: "、")).font(.caption)
                ForEach(Array(preview.lines.enumerated()), id: \.offset) { index, line in note(line, "preview.\(index)") }
                HStack {
                    action("文件导出意图", "export.fileIntent") { store.expressExportIntent(.file) }
                    action("外部发送意图", "export.externalIntent") { store.expressExportIntent(.external) }
                }
                ForEach(store.exportIntents, id: \.rawValue) { note($0.rawValue, "export.intent.\($0.rawValue)") }
                action("关闭 / 取消预览", "export.close") { store.closePreview() }
            }
            note("本地查看、文件导出、外部发送是不同动作。此处没有文件/剪贴板/网络；不含秘密、账户、路径、全文，不承诺绝对匿名或召回外部副本。", "export.boundary")
        }
    }

    private var retention: some View {
        VStack(alignment: .leading, spacing: 12) {
            note("汇总与明细分层。未设默认天数、永久保留或自动到期策略；仅演示当前选择的内存清理。", "retention.policy")
            ForEach(USReviewCleanupLayer.allCases) { layer in
                action("预览：\(layer.title)", "cleanup.begin.\(layer.id)") { store.beginCleanup(layer) }
            }
            if let draft = store.cleanupDraft {
                note(draft.impact, "cleanup.impact")
                HStack {
                    action(draft.layer == .protectedRecords ? "核对不可清范围" : "确认仅清理上述内存范围", "cleanup.apply") { store.applyCleanup() }
                    action("取消", "cleanup.cancel") { store.cancelCleanup() }
                }
            }
            note(store.cleanupResult, "cleanup.result")
            note("可见明细 \(store.visibleDetails.count) 条；历史汇总\(store.summary.hasSummaryGap ? "存在断点，不是零" : "仍按已知覆盖保留")。", "details.state")
            HStack {
                action("普通刷新，不重建旧明细", "cleanup.refresh") { store.refresh() }
                action("模拟独立新报告", "events.newReport") { store.receiveNewReport() }
            }
            note("成果/任务/原会话引用保留 · \(store.protectedFacts.supervision) · \(store.protectedFacts.acceptance)", "protected.artifacts")
            note("控制消耗 \(store.protectedFacts.consumedControlTokens) · 重试 \(store.protectedFacts.retries)/\(store.protectedFacts.retryLimit) · 用户 Stop \(store.protectedFacts.userStop ? "仍有效" : "未设置")；统计清理不连带它们。", "protected.controls")
        }
    }

    private var resources: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("独立资源预设", selection: Binding(get: { store.resourcePreset }, set: { store.selectResourcePreset($0) })) {
                ForEach(USReviewResourcePreset.allCases) { Text($0.title).tag($0) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("resource.preset"))
            valueRow("任务控制计量 / 上限", "\(USReviewFormat.number(store.resourceProjection.taskKnown)) / \(store.resourceProjection.taskLimit) Token")
            valueRow("项目共享控制计量 / 上限", "\(USReviewFormat.number(store.resourceProjection.projectKnown)) / \(store.resourceProjection.projectLimit) Token")
            note(store.resourceProjection.basis, "resource.basis")
            note(store.resourcePreset.title, "resource.status")
            note(store.resourceProjection.independentWork, "independentWork")
            ForEach(Array(store.resourceProjection.reasons.enumerated()), id: \.offset) { index, reason in
                Label(reason, systemImage: "exclamationmark.triangle").foregroundStyle(Color.dsWarning)
                    .accessibilityIdentifier(id("resource.reason.\(index)"))
            }
            valueRow("原执行", store.resourceProjection.execution.rawValue)
            valueRow("监督 / 验收", store.resourceProjection.verification)
            HStack {
                action("模拟在途到限请求", "events.limit") { store.simulateLimitRequest() }
                action("模拟收束确认", "events.winding") { store.simulateWinding() }
            }
            HStack {
                action("模拟核实已停止", "events.verifyStop") { store.simulateStopVerification(known: true) }
                action("模拟核实仍未知", "events.unknownStop") { store.simulateStopVerification(known: false) }
            }
            action("预览项目 P-A 调额", "quota.begin") { store.beginQuotaDraft() }
            if let draft = store.quotaDraft {
                valueRow("仅 \(draft.project) 策略草稿", "\(draft.previousLimit) → \(draft.proposedLimit) Token；不重置计量")
                HStack {
                    action("仅应用策略", "quota.apply") { store.applyQuotaDraft() }
                    action("取消调额", "quota.cancel") { store.cancelQuotaDraft() }
                }
            }
            HStack {
                action("继续意图", "continue.intent") { store.expressContinue() }
                action("补额并继续意图", "topUpContinue.intent") { store.expressContinue(topUp: true) }
            }
            note("继续意图：\(store.continueIntent ? "已记录，未执行" : "无")；补额并继续：\(store.topUpContinueIntent ? "已记录，未执行" : "无")", "continue.state")
            Divider()
            Picker("载入虚构续行条件", selection: Binding(get: { store.continuePolicy }, set: {
                store.loadContinuationFixture(policy: $0, block: nil)
            })) {
                ForEach(USReviewContinuePolicy.allCases) { Text($0.title).tag($0) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("continue.fixture"))
            Picker("独立负例（每次重载成功基准再替换一项）", selection: Binding<USReviewContinuationBlock?>(
                get: { store.continuationBlocks.first }, set: { store.loadContinuationFixture(policy: store.continuePolicy, block: $0) }
            )) {
                Text("无负例，完整条件成功基准").tag(Optional<USReviewContinuationBlock>.none)
                ForEach(USReviewContinuationBlock.allCases) { Text($0.title).tag(Optional($0)) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("continue.block"))
            action("显式模拟完整重核", "events.recheck") { store.recheckAdmission() }
            ForEach(Array(store.continuationReasons.enumerated()), id: \.offset) { index, reason in
                note("仍受阻：\(reason)", "continue.reason.\(index)")
            }
            valueRow("新工作准入", store.admissionCanContinue ? "样例尚需工作可继续（未执行）" : "尚未完整重核或条件不满足")
            note("完整重核：有效交办、目标/输入/依赖、授权、全部资源、原执行/占用/副作用安全、独立重试、必要监督/验证。已完成不重放、未知不重试；资源恢复不等于验收或真实启动。", "continue.boundary")
        }
    }

    private var reviewControls: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("固定虚构故事板", selection: Binding(get: { store.presetID }, set: { value in
                store.loadPreset(value); expandPreset(value)
            })) {
                ForEach(NativeUsageStatisticsReviewFixtures.presets) { Text("\($0.id) · \($0.title)").tag($0.id) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("scenario"))
            note("载入任何预设会丢弃局部草稿、事件、快照和预览，不是生产刷新。六组编号仅用于审阅，不作为常驻导航。", "scenario.boundary")
            Toggle("经典不透明外观", isOn: $store.classic)
            Toggle("演示减少透明度", isOn: $store.demoTransparency)
            Toggle("演示提高对比度", isOn: $store.demoContrast)
            Toggle("演示减少动态", isOn: $store.demoMotion)
            note("演示开关只能加强系统辅助偏好；外观与 interactionMode / 执行事实无关。", "appearance.boundary")
            action("重置全部虚构内存", "reset") { store.reset(); expandPreset("US1/baseline") }
            note(store.actionMessage, "actionMessage")
        }
    }

    private func expandPreset(_ value: String) {
        detailsExpanded = value.hasPrefix("US1/") && value != "US1/baseline"
        sourcesExpanded = value.hasPrefix("US2/")
        timeExpanded = value.hasPrefix("US3/")
        visibilityExpanded = value.hasPrefix("US4/")
        retentionExpanded = value.hasPrefix("US5/")
        resourcesExpanded = value.hasPrefix("US6/")
    }
    private func id(_ key: String) -> String { "usReview." + key }
    private func note(_ value: String, _ key: String) -> some View {
        Text(value).font(.caption)
            .foregroundStyle(presentation.strongContrast ? Color.dsText : Color.dsTextSecondary)
            .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            .accessibilityIdentifier(id(key))
    }
    private func valueRow(_ title: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption).foregroundStyle(Color.dsTextSecondary)
            Text(value).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        }
    }
    private func panel<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        GlassPanel(preference: surface, reduceTransparency: presentation.opaque,
                   increaseContrast: presentation.strongContrast) {
            VStack(alignment: .leading, spacing: 12, content: content).padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private func disclosure<Content: View>(_ title: String, id key: String,
                                          expanded: Binding<Bool>, @ViewBuilder content: @escaping () -> Content) -> some View {
        panel {
            DisclosureGroup(isExpanded: expanded) {
                VStack(alignment: .leading, spacing: 12, content: content).padding(.top, 12)
            } label: { Text(title).fontWeight(.medium) }
            .accessibilityIdentifier(id(key + ".disclosure"))
        }
    }
    @ViewBuilder private func action(_ title: String, _ key: String, perform: @escaping () -> Void) -> some View {
        if #available(macOS 26.0, *), !presentation.opaque {
            Button(title, action: perform).buttonStyle(.glass).buttonBorderShape(.capsule)
                .accessibilityIdentifier(id(key))
        } else {
            Button(title, action: perform).buttonStyle(.bordered).buttonBorderShape(.capsule)
                .accessibilityIdentifier(id(key))
        }
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
#endif
