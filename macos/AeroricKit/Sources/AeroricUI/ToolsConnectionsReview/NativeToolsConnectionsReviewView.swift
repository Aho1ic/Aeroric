#if DEBUG
import SwiftUI

/// DEBUG-only fictional memory. Construction does not construct product services.
@MainActor
public struct NativeToolsConnectionsReviewView: View {
    @State private var store = ToolsConnectionsReviewStore()
    @State private var surface: SurfacePreference = .glass
    @State private var appearance: TConnReviewAppearance = .system
    @State private var demonstrateTransparency = false
    @State private var demonstrateContrast = false
    @State private var demonstrateMotion = false
    @State private var disableScope: TConnReviewScope = .task
    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() {}

    private var reduceTransparency: Bool { systemTransparency || demonstrateTransparency }
    private var increaseContrast: Bool { systemContrast == .increased || demonstrateContrast }
    private var reduceMotion: Bool { systemMotion || demonstrateMotion }
    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: reduceTransparency)
    }

    public var body: some View {
        GeometryReader { geometry in
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    header
                    scenarioControls
                    if geometry.size.width >= 940 {
                        HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                            catalog.frame(width: min(320, geometry.size.width * 0.28))
                            details.frame(maxWidth: .infinity)
                        }
                    } else {
                        catalog
                        details
                    }
                    panel("本窗口意图与示例事件", "activity") {
                        text(store.message, "activity.message")
                        text("外部执行 \(store.externalExecutionCount) · 重放 \(store.replayCount) · 只是样例计数，不是安全隔离认证。", "activity.counts")
                        text(store.independentWork, "activity.independent")
                        ForEach(Array(store.eventLog.enumerated()), id: \.offset) { index, event in
                            text(event, "activity.event.\(index)")
                        }
                    }
                    appearanceControls
                }
                .padding(NativePanelGeometry.panelInset)
            }
            .foregroundStyle(Color.dsText)
            .background {
                NativePanelFoundation(preference: surface, reduceTransparency: reduceTransparency)
            }
            .animation(reduceMotion ? nil : .easeInOut(duration: DSMotion.fast), value: store.selectedID)
            .transaction {
                if reduceMotion {
                    $0.animation = nil
                    $0.disablesAnimations = true
                }
            }
        }
        .preferredColorScheme(appearance.colorScheme)
    }

    private var header: some View {
        panel("工具库与连接 · 离线审阅", "header") {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    text(ToolsConnectionsReviewFixtures.offlineNotice, "offlineNotice")
                        .fontWeight(.semibold).foregroundStyle(Color.dsWarning)
                    text(ToolsConnectionsReviewFixtures.acceptanceBoundary, "acceptanceBoundary")
                }
                Spacer(minLength: 0)
                NativeIconButton(
                    systemImage: "arrow.counterclockwise", label: "重置当前离线场景",
                    identifier: id("reset"), help: "丢弃本窗口局部变化、草稿、事件和结果展开"
                ) { store.reset(); disableScope = .task }
            }
        }
    }

    private var scenarioControls: some View {
        panel("场景与上下文", "scenario") {
            Picker("六组离线关系", selection: Binding(
                get: { store.scenario },
                set: { store.loadScenario($0); disableScope = .task }
            )) {
                ForEach(ToolsConnectionsReviewScenario.allCases) { scenario in
                    Text("\(scenario.rawValue) · \(scenario.title)").tag(scenario)
                }
            }
            .pickerStyle(.menu).accessibilityIdentifier(id("scenario"))
            Picker("正向与受阻示例", selection: Binding(
                get: { store.variant }, set: { store.loadScenario(store.scenario, variant: $0) }
            )) {
                ForEach(store.scenario.variants) { variant in Text(variant.title).tag(variant) }
            }
            .pickerStyle(.menu).accessibilityIdentifier(id("scenario.variant"))
            Picker("项目与任务范围", selection: Binding(get: { store.context }, set: { store.selectContext($0) })) {
                ForEach(ToolsConnectionsReviewContext.allCases) { context in Text(context.title).tag(context) }
            }
            .pickerStyle(.menu).accessibilityIdentifier(id("context"))
            text("每组有可切换的正向/受阻分支；载入预设会清空本地草稿与事件，不代表真实探测或任务执行。", "scenario.boundary")
        }
    }

    private var catalog: some View {
        panel("统一目录", "catalog") {
            NativeSegmentedPicker(
                selection: Binding(get: { store.category }, set: { store.selectCategory($0) }),
                groupTitle: "工具库类别",
                segments: TConnReviewCategory.allCases.map {
                    .init(value: $0, title: LocalizedStringKey($0.title))
                }
            ).accessibilityIdentifier(id("catalog.category"))
            NativeSelectionList(selection: Binding<String?>(
                get: { store.selectedID },
                set: { if let next = $0 { store.selectItem(next) } }
            )) {
                ForEach(store.visibleItems) { item in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(item.title).fontWeight(.medium)
                        Text(item.purpose).font(.caption).foregroundStyle(Color.dsTextSecondary)
                        Text(store.eligibility(for: item.id).title)
                            .font(.caption).foregroundStyle(Color.dsTextSecondary)
                    }
                    .tag(item.id)
                    .accessibilityLabel("\(item.title)，\(item.purpose)，\(store.eligibility(for: item.id).title)")
                    .accessibilityIdentifier(id("catalog.item.\(item.id)"))
                }
            }.frame(height: 290)
            text("目录不是安装/调用清单；不默认加载整库到 Agent。方法与连接按条目 ID 复用，不复制配置。", "catalog.boundary")
        }
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            panel(store.selectedItem.title, "detail") {
                text(store.selectedItem.purpose, "detail.purpose")
                text(store.selectedItem.provenance, "detail.source")
                text("关联条目：\(store.selectedItem.references.isEmpty ? "无" : store.selectedItem.references.joined(separator: "、"))", "detail.references")
                text("目标：\(store.selectedItem.target)；查看不会连接、安装或探测。", "detail.target")
                ForEach(store.detailLayers) { layer in
                    fact(layer.title, layer.fact, "detail.layers.\(layer.id)")
                }
                text(ToolsConnectionsReviewFixtures.executorNotice, "detail.executors")
                text("执行身份：\(store.identity.executor) / \(store.identity.api) / \(store.identity.model)；会话/权限/cwd/PTY 均不随外观改变。", "detail.identity")
            }
            switch store.scenario {
            case .TC1: scopePanel
            case .TC2: selectionPanel
            case .TC3: introductionPanel
            case .TC4: continuationPanel
            case .TC5: credentialPanel
            case .TC6: resultsPanel
            }
        }
    }

    private var scopePanel: some View {
        panel("TC1 · 配置、启用、引用与操作授权分开", "scope") {
            text(store.scopeSummary, "scope.summary")
            action("仅保存示例配置", "scope.configuration") { store.saveConfiguration() }
            action("编辑范围草稿", "scope.editor") { store.beginScopeDraft() }
            if store.scopeDraft != nil {
                Picker("应用范围", selection: Binding(
                    get: { store.scopeDraft?.scope ?? .task },
                    set: { store.scopeDraft?.scope = $0 }
                )) {
                    ForEach(TConnReviewScope.allCases) { Text($0.title).tag($0) }
                }.accessibilityIdentifier(id("scope.draft.scope"))
                Toggle("启用所选范围", isOn: Binding(
                    get: { store.scopeDraft?.enabled ?? false },
                    set: { store.scopeDraft?.enabled = $0 }
                )).accessibilityIdentifier(id("scope.draft.enabled"))
                Toggle("任务临时引用（不写项目默认）", isOn: Binding(
                    get: { store.scopeDraft?.temporary ?? false },
                    set: { store.scopeDraft?.temporary = $0 }
                )).disabled(store.scopeDraft?.scope == .project)
                    .accessibilityIdentifier(id("scope.draft.temporary"))
                action("保存示例范围", "scope.save") { store.commitScopeDraft() }
                action("取消范围草稿", "scope.cancel", cancel: true) { store.cancelScopeDraft() }
            }
            action("项目明确启用当前条目", "scope.projectEnabled") { store.setProjectEnabled(true) }
                .disabled(store.context.project == nil)
            action("任务引用已启用项", "scope.reference") { store.referenceForTask() }
            action("仅本任务临时引用", "scope.temporaryReference") { store.referenceForTask(temporary: true) }
            text("以上均不授操作权；无项目任务直接使用任务范围，无需建项目。", "scope.boundary")
            eventButtons
        }
    }

    private var selectionPanel: some View {
        panel("TC2 · 分层状态与范围内拟选", "selection") {
            Picker("用户约束", selection: Binding(
                get: { store.userConstraint }, set: { store.setUserConstraint($0) }
            )) {
                ForEach(TConnReviewUserConstraint.allCases) { Text($0.title).tag($0) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("selection.constraint"))
            text("约束锚点：\(store.constrainedID ?? "无，范围内自主拟选")；浏览其它条目不迁移指定/禁用，任务之间不串用。", "selection.constraintAnchor")
            action("示例拟选工具 · 不调用", "selection.propose") { store.proposeSelection() }
            text("拟选：\(store.proposedID ?? "无")", "selection.proposed")
            let projection = store.eligibility(for: store.selectedID)
            text(projection.title, "selection.reason")
            ForEach(Array(projection.reasons.enumerated()), id: \.offset) { index, reason in
                text("受阻：\(reason)", "selection.block.\(index)").foregroundStyle(Color.dsWarning)
            }
            text("未知/未检查/过时不等于可用；不暗换目标、执行器、API 或模型；独立安全工作可继续。", "selection.boundary")
            eventButtons
        }
    }

    private var introductionPanel: some View {
        panel("TC3 · 引入、预授权、新技能与更新", "review") {
            text("demo-publisher · 版本依据：虚构固定清单；官方/只读自称、目录收录、模型推荐均非可信或安全认证。", "review.source")
            text("位置：\(store.variant == .remoteUnpinned ? "demo-remote-service · 接入不是本机安装" : "demo-execution-location")", "review.location")
            text("依赖/脚本/服务/联网/文件/网络/凭据/目标范围：集中列示，仅虚构开发目标；未知不默许扩权。", "review.dependencies")
            text(store.inFlightVersion, "review.inFlightVersion")
            text(store.variant == .remoteUnpinned
                 ? "远端服务无法固定版本；实质变化须重核契约/目标/授权，未知只阻塞相关新增调用。"
                 : "demo-v2 候选：能力/依赖/权限及数据去向有差异，普通更新先审阅；一次安装不授未来更新。", "review.version")
            ForEach(TConnReviewOperation.allCases) { operation in
                action(operation.title, "review.begin.\(operation.id)") { store.beginIntroductionReview(operation) }
            }
            if let draft = store.reviewDraft {
                text("\(draft.operation.title) · \(draft.version) · \(draft.source)", "review.draft")
                text("范围：\(draft.context.title) / \(draft.target)；\(draft.permissions)", "review.scope")
                text(draft.location + "；" + draft.dependencies, "review.draft.dependencies")
                text(draft.differences, "review.differences")
                action("同意示例意图 · 尚未执行", "review.confirm") { store.confirmReviewIntent() }
                action("取消审阅", "review.cancel", cancel: true) { store.cancelReview() }
            }
            text("获准意图 \(store.approvedIntents.count) 项（含预授权覆盖，均未执行）", "review.intentCount")
            ForEach(Array(store.approvedIntents.enumerated()), id: \.offset) { index, intent in
                text("\(intent.draft.operation.title) · \(intent.draft.context.title) · \(intent.draft.version) · \(intent.preauthorized ? "预授权覆盖" : "集中审阅同意") · 尚未执行", "review.intent.\(index)")
            }
            text(store.skillBoundary, "review.skillBoundary")
            if store.variant == .bundledSkill {
                action("人工同意附带新技能示例 · 不安装", "review.skillConsent") { store.consentToBundledSkill() }
            }
            fact("安装结果", store.installedFact, "review.installation")
            fact("新版候选结果", store.candidateVersionFact, "review.candidate")
            text(store.rollbackProjection.title + "；" + store.rollbackProjection.reasons.joined(separator: "；"), "review.rollback")
            text("回退工具不撤销业务写入、不重放任务、不恢复 Stop；失败/残留/未知保留，不强杀共享服务或删锁。", "review.boundary")
            eventButtons
        }
    }

    private var continuationPanel: some View {
        panel("TC4 · 范围停用、请求停止与条件恢复", "continuation") {
            Picker("明确停用/重新启用范围", selection: $disableScope) {
                ForEach(TConnReviewScope.allCases) { Text($0.title).tag($0) }
            }.accessibilityIdentifier(id("disable.scope"))
            action("审阅停用范围", "disable.begin") { store.beginDisableDraft(scope: disableScope) }
            if let draft = store.disableDraft {
                text("拟停用：\(draft.context.title) · \(draft.scope.title) · \(draft.itemID)；不是卸载或删配置/秘密。", "disable.draft")
                action("确认停用指定范围", "disable.confirm") { store.confirmDisableIntent() }
                action("取消停用", "disable.cancel", cancel: true) { store.cancelDisableDraft() }
            }
            action("重新启用指定范围 · 不复活 Stop", "disable.reenable") { store.reenableScope(disableScope) }
            text("当前范围：\(store.isDisabled(store.selectedID, in: store.context) ? "已停用：新增/排队/重试受阻" : "未停用")", "disable.state")
            text(store.stopState.rawValue, "continuation.stopState")
            action("记录明确 Stop", "continuation.stop") { store.recordExplicitStop() }
            action("记录明确恢复意图 · 仍须重核", "continuation.resume") { store.recordExplicitResume() }
            text(store.continuation.disposition.rawValue, "continuation.disposition")
            text(store.continuationProjection.title, "continuation.projection")
            ForEach(Array(store.continuationProjection.reasons.enumerated()), id: \.offset) { index, reason in
                text(reason, "continuation.reasons.\(index)").foregroundStyle(Color.dsWarning)
            }
            text("条件恢复只重建条件；有效等待覆盖内可续行投影不再审批。暂停/Stop/取消须明确恢复；到限沿 Q67。未知原执行先核实，不重放或重叠替代。", "continuation.boundary")
            ForEach(Array(store.businessHistory.enumerated()), id: \.offset) { index, history in
                text(history, "continuation.history.\(index)")
            }
            eventButtons
        }
    }

    private var credentialPanel: some View {
        panel("TC5 · 仅凭据引用，使用许可不等于导出许可", "credential") {
            text("虚构连接：演示开发库连接；目标：\(store.identity.target)", "credential.target")
            text(ToolsConnectionsReviewFixtures.credentialReference, "credential.reference")
            text(ToolsConnectionsReviewFixtures.credentialLocation, "credential.location")
            text(store.credentialState.rawValue, "credential.status")
            ForEach(TConnReviewCredentialIntent.allCases) { intent in
                action(intent.title + " · 不执行", "credential.intent.\(intent.id)") { store.recordCredentialIntent(intent) }
            }
            text(store.credentialMessage, "credential.intent")
            text(store.sharePreview, "credential.sharePreview")
            text("不向模型/全队复制秘密；监督角色不默认获业务凭据，不扫描个人配置找替代账号，不静默换 API/模型/资源。", "credential.boundary")
        }
    }

    private var resultsPanel: some View {
        panel("TC6 · 固定内存结果与独立证据层", "result") {
            text("以下固定结果属于项目甲任务一的 demo-query 调用；浏览其它目录/任务不迁移事件归属，不读取当前任务数据。", "result.anchor")
            ForEach(store.results) { result in
                VStack(alignment: .leading, spacing: 8) {
                    text(result.title, "result.\(result.id).title").font(.headline)
                    text(result.source, "result.\(result.id).source")
                    text(result.preview, "result.\(result.id).preview")
                    ForEach(Array(result.errors.enumerated()), id: \.offset) { index, error in
                        text(error, "result.\(result.id).errors.\(index)").foregroundStyle(Color.dsDanger)
                    }
                    text(result.limits.joined(separator: "；"), "result.\(result.id).limits")
                    action(store.expandedResultIDs.contains(result.id) ? "收起内存文本" : "展开内存文本 · 不代表已读",
                           "result.\(result.id).expand") { store.toggleResultExpansion(result.id) }
                    if store.expandedResultIDs.contains(result.id) {
                        text(result.body, "result.\(result.id).body")
                    }
                    action("补全结果意图 · 不重放", "result.\(result.id).complete") { store.requestResultCompletionIntent(result.id) }
                    action("分享意图 · 不外发", "result.\(result.id).share") { store.recordResultShareIntent(result.id) }
                    fact("调用返回", result.evidence.returned, "result.\(result.id).returned")
                    fact("业务结果", result.evidence.business, "result.\(result.id).business")
                    fact("指定执行者读取", result.evidence.read, "result.\(result.id).read")
                    fact("指定执行者采用", result.evidence.adopted, "result.\(result.id).adopted")
                    fact("技术核验", result.evidence.verified, "result.\(result.id).verified")
                    fact("用户验收", result.evidence.accepted, "result.\(result.id).accepted")
                    text(result.evidence.modelReport, "result.\(result.id).modelReport")
                    ForEach(store.availableEvents) { event in
                        action("载入示例事件：\(event.title)", "result.\(result.id).event.\(event.id)") {
                            store.loadExampleEvent(event, resultID: result.id)
                        }
                    }
                    text("这些按钮只载入带来源的固定事实，不执行远端读取/核验/验收。关联不证明正确性。", "result.\(result.id).eventBoundary")
                }
                .padding(NativePanelGeometry.panelInset)
                .background(Color.dsTextBackground, in: RoundedRectangle(cornerRadius: DSRadius.sm))
            }
        }
    }

    private var eventButtons: some View {
        VStack(alignment: .leading, spacing: 8) {
            text("载入示例事件不是普通操作确认：只改变列明的示例事实，未实际执行。", "events.boundary")
            ForEach(store.availableEvents) { event in
                action("载入示例事件：\(event.title)", "events.\(event.id)") { store.loadExampleEvent(event) }
                text("虚构来源：\(event.rawValue)", "events.\(event.id).source")
            }
        }
    }

    private var appearanceControls: some View {
        panel("样例外观与辅助偏好 · 不改执行身份", "appearance") {
            NativeSegmentedPicker(
                selection: $surface, groupTitle: "面板外观",
                segments: [.init(value: .glass, title: "玻璃"), .init(value: .classic, title: "经典")]
            ).accessibilityIdentifier(id("appearance.surface"))
            Picker("浅深示例", selection: $appearance) {
                ForEach(TConnReviewAppearance.allCases) { Text($0.title).tag($0) }
            }.pickerStyle(.menu).accessibilityIdentifier(id("appearance.scheme"))
            Toggle("示例加强：减少透明度", isOn: $demonstrateTransparency).accessibilityIdentifier(id("appearance.transparency"))
            Toggle("示例加强：增强对比", isOn: $demonstrateContrast).accessibilityIdentifier(id("appearance.contrast"))
            Toggle("示例加强：减少动效", isOn: $demonstrateMotion).accessibilityIdentifier(id("appearance.motion"))
            text("系统/示例/有效减少透明度：\(systemTransparency)/\(demonstrateTransparency)/\(reduceTransparency)；"
                 + "增强对比：\(systemContrast == .increased)/\(demonstrateContrast)/\(increaseContrast)；"
                 + "减少动效：\(systemMotion)/\(demonstrateMotion)/\(reduceMotion)。", "appearance.effective")
            text("只改局部呈现；不更改全局偏好、interactionMode、任务/会话、模型/API、权限、cwd 或 PTY。真实材质/AX/键盘/布局尚未运行验收。", "appearance.boundary")
        }
    }

    private func id(_ leaf: String) -> String { "toolsConnectionsReview." + leaf }
    private func text(_ value: String, _ leaf: String) -> some View {
        Text(value).fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled).accessibilityIdentifier(id(leaf))
    }

    private func fact(_ title: String, _ value: TConnReviewFact, _ leaf: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            text("\(title)：\(value.text)", leaf)
                .foregroundStyle(value.state == .blocked ? Color.dsWarning : Color.dsText)
            text("依据：\(value.source)", leaf + ".source")
                .font(.caption).foregroundStyle(Color.dsTextSecondary)
        }
    }

    private func panel<Content: View>(
        _ title: String, _ leaf: String, @ViewBuilder content: () -> Content
    ) -> some View {
        GlassPanel(preference: surface, reduceTransparency: reduceTransparency, increaseContrast: increaseContrast) {
            VStack(alignment: .leading, spacing: 10) {
                text(title, leaf + ".heading").font(.headline)
                content()
            }
            .padding(NativePanelGeometry.panelInset)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func action(
        _ title: String, _ leaf: String, cancel: Bool = false, perform: @escaping () -> Void
    ) -> some View {
        Button(title, role: cancel ? .cancel : nil, action: perform)
            .modifier(TConnReviewShortControl(
                glass: effectiveSurface == .glass && !reduceTransparency, increasedContrast: increaseContrast
            ))
            .accessibilityLabel(title).accessibilityIdentifier(id(leaf))
    }
}

private enum TConnReviewAppearance: String, CaseIterable, Identifiable {
    case system, light, dark
    var id: String { rawValue }
    var title: String {
        switch self {
        case .system: "跟随系统"
        case .light: "浅色示例"
        case .dark: "深色示例"
        }
    }
    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

private struct TConnReviewShortControl: ViewModifier {
    let glass: Bool
    let increasedContrast: Bool
    @ViewBuilder func body(content: Content) -> some View {
        if #available(macOS 26, *), glass {
            content.buttonStyle(.glass).buttonBorderShape(.capsule)
                .fontWeight(increasedContrast ? .semibold : .regular).controlSize(.small)
        } else {
            content.buttonStyle(.bordered).buttonBorderShape(.capsule)
                .fontWeight(increasedContrast ? .semibold : .regular).controlSize(.small)
        }
    }
}
#endif
