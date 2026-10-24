#if DEBUG
import Foundation
import SwiftUI

/// Window-owned, offline specimens only. Constructing this root never supplies a
/// production client, preference store, scheduler or window.
@MainActor
public struct NativeWorkArrangementsReviewView: View {
    @State private var store: WorkArrangementsReviewStore
    @State private var sheet: WAReviewSheet?
    @State private var selectedRuleID: String? = "rule"
    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() {
        _store = State(initialValue: WorkArrangementsReviewStore())
    }

    private var presentation: WAReviewPresentation {
        WAReviewPresentation(
            surface: store.classicAppearance ? .classic : .glass,
            reduceTransparency: systemTransparency || store.demonstrateReducedTransparency,
            increaseContrast: systemContrast == .increased || store.demonstrateIncreasedContrast,
            reduceMotion: systemMotion || store.demonstrateReducedMotion
        )
    }

    public var body: some View {
        GeometryReader { geometry in
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                header
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                        appearanceControls
                        if geometry.size.width >= 920 {
                            HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                                workList.frame(width: 280)
                                mainContent.frame(maxWidth: .infinity, alignment: .leading)
                            }
                        } else {
                            workList
                            mainContent
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.bottom, NativePanelGeometry.panelInset)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            .background {
                NativePanelFoundation(
                    preference: presentation.surface,
                    reduceTransparency: presentation.reduceTransparency
                )
            }
            .animation(presentation.animation, value: store.section)
            .transaction { transaction in
                if presentation.reduceMotion {
                    transaction.animation = nil
                    transaction.disablesAnimations = true
                }
            }
            .sheet(item: $sheet) { target in
                sheetContent(target)
                    .frame(
                        width: max(1, min(580, geometry.size.width - 2 * NativePanelGeometry.panelInset)),
                        height: max(1, min(720, geometry.size.height - 2 * NativePanelGeometry.panelInset))
                    )
            }
            .onChange(of: store.generation) { _, _ in
                sheet = nil
                selectedRuleID = "rule"
            }
        }
    }

    private var header: some View {
        WAReviewPanel(title: "星图项目 · 虚构工作安排", id: "header", presentation: presentation) {
            WAReviewText(
                WorkArrangementsReviewFixtures.disclaimer, id: "disclaimer",
                color: AeroricColor.accent
            )
            HStack {
                Menu {
                    Picker("载入初始示例场景", selection: Binding(
                        get: { store.scenario },
                        set: { scenario in
                            sheet = nil
                            store.reset(to: scenario)
                        }
                    )) {
                        ForEach(WAReviewScenario.allCases) { scenario in
                            Text("载入 \(scenario.rawValue) · \(scenario.title) 示例记录")
                                .tag(scenario)
                                .accessibilityIdentifier("workArrangementsReview.scenario.\(scenario.id)")
                        }
                    }
                } label: {
                    Text("\(store.scenario.rawValue) 场景")
                        .accessibilityIdentifier("workArrangementsReview.scenario.menu")
                }
                Spacer()
                NativeIconButton(
                    systemImage: "arrow.counterclockwise",
                    label: "重置：载入本场景初始示例",
                    identifier: "workArrangementsReview.reset",
                    help: "丢弃本窗口草稿、同意及演示记录；不恢复真实进程"
                ) {
                    sheet = nil
                    store.reset()
                }
            }
            WAReviewText(store.scenario.title, id: "scenario.title")
        }
    }

    private var appearanceControls: some View {
        @Bindable var state = store
        return WAReviewPanel(title: "本窗口外观演示", id: "appearance", presentation: presentation) {
            DisclosureGroup("外观与辅助偏好（不改变授权）") {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    NativeSegmentedPicker(
                        selection: $state.classicAppearance, groupTitle: "材质外观",
                        segments: [.init(value: false, title: "glass"), .init(value: true, title: "classic")]
                    )
                    NativeSegmentedPicker(
                        selection: $state.advancedPresentation, groupTitle: "交互呈现，仅改变说明",
                        segments: [.init(value: false, title: "native"), .init(value: true, title: "advanced")]
                    )
                    Toggle("演示：减少透明度", isOn: $state.demonstrateReducedTransparency)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("workArrangementsReview.appearance.transparency")
                    Toggle("演示：增强对比", isOn: $state.demonstrateIncreasedContrast)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("workArrangementsReview.appearance.contrast")
                    Toggle("演示：减少动效", isOn: $state.demonstrateReducedMotion)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("workArrangementsReview.appearance.motion")
                    WAReviewReading(presentation: presentation) {
                        WAReviewText(
                            "系统 / 演示 / 有效值\n"
                            + "减少透明度：\(flag(systemTransparency)) / \(flag(state.demonstrateReducedTransparency)) / \(flag(presentation.reduceTransparency))\n"
                            + "增强对比：\(flag(systemContrast == .increased)) / \(flag(state.demonstrateIncreasedContrast)) / \(flag(presentation.increaseContrast))\n"
                            + "减少动效：\(flag(systemMotion)) / \(flag(state.demonstrateReducedMotion)) / \(flag(presentation.reduceMotion))",
                            id: "appearance.effective"
                        )
                        WAReviewText(
                            "演示只加强系统偏好；减少透明度时采用 classic。材质与 native/advanced 仅属呈现，不改变交办、权限、配置或执行。",
                            id: "appearance.boundary"
                        )
                    }
                }
                .padding(.top, NativePanelGeometry.panelInset)
            }
        }
    }

    private var workList: some View {
        @Bindable var state = store
        return WAReviewPanel(title: "工作列表", id: "list", presentation: presentation) {
            NativeSegmentedPicker(
                selection: $state.section, groupTitle: "工作安排分区",
                segments: [
                    .init(value: .todos, title: "待办"),
                    .init(value: .executions, title: "关联执行"),
                    .init(value: .rule, title: "规则"),
                ]
            )
            selectionList
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
                .background(AeroricColor.layer1)
                .frame(height: store.section == .rule ? 120 : 300)
            WAReviewActions {
                WAReviewAction("新建目标", id: "todo.new", presentation: presentation) {
                    store.section = .todos
                    sheet = WAReviewSheet(draft: .todo(store.makeNewTodoDraft()))
                }
                WAReviewAction("批量交办", id: "batch.assignment.open", presentation: presentation) {
                    sheet = WAReviewSheet(draft: .batch(store.makeBatchDraft(kind: .assignment)))
                }
                WAReviewAction("纳入旧项", id: "batch.backlog.open", presentation: presentation) {
                    sheet = WAReviewSheet(draft: .batch(store.makeBatchDraft(kind: .backlog)))
                }
            }
        }
    }

    @ViewBuilder private var selectionList: some View {
        switch store.section {
        case .todos:
            NativeSelectionList(selection: Binding(
                get: { store.selectedTodoID }, set: { store.selectedTodoID = $0 }
            )) {
                ForEach(store.todos) { todo in
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        Text(todo.title)
                            .accessibilityIdentifier("workArrangementsReview.todo.\(todo.id).title")
                        Text("\(todo.source.rawValue) · \(todo.intent.rawValue)")
                            .font(.caption)
                            .accessibilityIdentifier("workArrangementsReview.todo.\(todo.id).summary")
                    }
                    .padding(.vertical, AeroricSpace.xs)
                    .tag(todo.id)
                }
            }
        case .executions:
            NativeSelectionList(selection: Binding(
                get: { store.selectedRunID }, set: { store.selectedRunID = $0 }
            )) {
                ForEach(store.executions) { run in
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        Text(run.basis.goal)
                            .accessibilityIdentifier("workArrangementsReview.run.\(run.id).title")
                        Text("\(run.id) · \(run.phase.rawValue)")
                            .font(.caption)
                            .accessibilityIdentifier("workArrangementsReview.run.\(run.id).summary")
                    }
                    .padding(.vertical, AeroricSpace.xs)
                    .tag(run.id)
                }
            }
        case .rule:
            NativeSelectionList(selection: $selectedRuleID) {
                VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                    Text("星图项目范围规则 · v\(store.rule.revision)")
                        .accessibilityIdentifier("workArrangementsReview.rule.row.title")
                    Text(store.rule.enabled ? "已启用（示例）" : "尚未启用")
                        .accessibilityIdentifier("workArrangementsReview.rule.row.state")
                }
                .tag("rule")
            }
        }
    }

    private var mainContent: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            detail
            WAReviewContext(store: store, presentation: presentation)
        }
    }

    @ViewBuilder private var detail: some View {
        switch store.section {
        case .todos:
            if let todo = store.todo(store.selectedTodoID) {
                WAReviewTodoDetail(store: store, todo: todo, presentation: presentation)
            } else {
                WAReviewText("请选择一条待办；新建目标只增加记录。", id: "todo.empty")
            }
        case .executions:
            if let run = store.execution(store.selectedRunID) {
                WAReviewRunDetail(store: store, run: run, presentation: presentation)
            } else {
                WAReviewText("请选择一次执行；历史不会被后续尝试覆盖。", id: "run.empty")
            }
        case .rule:
            WAReviewRuleDetail(store: store, presentation: presentation) {
                sheet = WAReviewSheet(draft: .rule(store.makeRuleDraft()))
            }
        }
    }

    @ViewBuilder private func sheetContent(_ target: WAReviewSheet) -> some View {
        switch target.draft {
        case let .todo(draft):
            WAReviewTodoSheet(store: store, draft: draft, presentation: presentation)
        case let .rule(draft):
            WAReviewRuleSheet(store: store, draft: draft, presentation: presentation)
        case let .batch(draft):
            WAReviewBatchSheet(store: store, draft: draft, presentation: presentation)
        }
    }

    private func flag(_ value: Bool) -> String { value ? "开" : "关" }
}

// MARK: - Actual detail projections, never substitute execution facts

@MainActor
private struct WAReviewTodoDetail: View {
    let store: WorkArrangementsReviewStore
    let todo: WAReviewTodo
    let presentation: WAReviewPresentation

    var body: some View {
        WAReviewPanel(title: "目标详情", id: "todo.detail", presentation: presentation) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(todo.title, id: "todo.detail.title")
                WAReviewText(
                    "\(todo.source.rawValue)\n\(todo.category.rawValue)\n"
                    + "意图：\(todo.intent.rawValue)；独立授权：\(todo.assignmentPermission.rawValue)\n"
                    + "规则交办：\(yes(todo.assignedByRule))；仅记录：\(yes(todo.recordOnly))；待澄清：\(yes(todo.clarificationNeeded))；排除：\(yes(todo.excluded))\n"
                    + "等待：\(todo.wait.rawValue)；允许等待：\(yes(todo.allowWaiting))\n"
                    + "输入已对齐：\(yes(todo.inputAligned))；本项尝试边界：\(todo.attemptPermission.rawValue)",
                    id: "todo.detail.facts"
                )
                WAReviewText(
                    "规则范围预览：\(store.previewReason(for: todo) ?? "条件匹配；匹配本身不授予运行权")",
                    id: "todo.detail.preview"
                )
                WAReviewText(itemBlockers, id: "todo.detail.blockers", color: AeroricColor.warning)
            }
            WAReviewActions {
                WAReviewAction("交给 Agent（示例）", id: "todo.assign", presentation: presentation) {
                    _ = store.assignTodo(todo.id)
                }
                WAReviewAction("手动请求启动（仅意图）", id: "todo.request", presentation: presentation) {
                    _ = store.requestLaunch(todoID: todo.id)
                }
            }
            verification
            if todo.allowWaiting { waitingActions }
            WAReviewReading(presentation: presentation) {
                WAReviewText("关联执行 · 每次结果独立保留", id: "todo.related.heading")
                ForEach(store.executions.filter { $0.todoID == todo.id }) { run in
                    WAReviewText(
                        "\(run.id)：\(run.phase.rawValue)\n\(run.basis.summary)",
                        id: "todo.related.\(run.id)"
                    )
                    WAReviewAction("查看该次执行", id: "todo.related.\(run.id).open", presentation: presentation) {
                        store.selectedRunID = run.id
                        store.section = .executions
                    }
                }
                if !store.executions.contains(where: { $0.todoID == todo.id }) {
                    WAReviewText("尚无关联执行；创建和交办均不制造执行记录。", id: "todo.related.empty")
                }
            }
        }
    }

    private var itemBlockers: String {
        let origins: [WAReviewLaunchOrigin] = todo.allowWaiting ? [.manual, .waiting] : [.manual]
        return origins.map { origin in
            let intent = WAReviewLaunchIntent(todoID: todo.id, origin: origin, continuingRunID: nil)
            let reasons = store.blockers(for: intent)
            let label = origin == .manual ? "手动请求启动" : "重核获准等待"
            return reasons.isEmpty
                ? "\(label)：当前无阻塞；仅形成意图，仍须另载入已开始记录。"
                : "\(label)当前阻塞：\n" + reasons.joined(separator: "\n")
        }.joined(separator: "\n\n")
    }

    private var verification: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(
                    store.isTechnicallyComplete(todo.id) ? "技术完成（示例）" : "技术核验未齐 · 执行结束不等于完成",
                    id: "todo.proof.technical",
                    color: store.isTechnicallyComplete(todo.id) ? AeroricColor.success : AeroricColor.warning
                )
                WAReviewText(
                    "核验关联：\(todo.proof.runID ?? "尚无")\n整合：\(todo.proof.integration.rawValue)\n"
                    + "必需检查：\(todo.proof.checks.rawValue)\n适用监督：\(todo.proof.supervision.rawValue)\n"
                    + "用户验收：\(todo.userAccepted ? "已明确记录（示例）" : "尚未记录")",
                    id: "todo.proof.facts"
                )
            }
            WAReviewActions {
                Menu {
                    ForEach(Array(WAReviewVerificationRecord.allCases.enumerated()), id: \.offset) { index, record in
                        Button("载入\(record.rawValue)示例记录") {
                            store.loadVerificationRecord(record, todoID: todo.id)
                        }
                        .accessibilityIdentifier("workArrangementsReview.todo.proof.load.\(index)")
                    }
                } label: {
                    Text("载入核验记录")
                        .accessibilityIdentifier("workArrangementsReview.todo.proof.menu")
                }
                WAReviewAction(
                    "记录用户验收（示例）", id: "todo.accept", presentation: presentation,
                    enabled: store.isTechnicallyComplete(todo.id) && !todo.userAccepted
                ) { _ = store.recordUserAcceptance(todo.id) }
            }
            WAReviewText(
                "先在关联执行中载入结束及安全收束，再载入目标核验。失败、停止、未知及缺监督均不能替代成功依据。",
                id: "todo.proof.help"
            )
            .background(AeroricColor.layer1)
        }
    }

    private var waitingActions: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            WAReviewActions {
                WAReviewAction("重核获准等待（意图）", id: "todo.wait.request", presentation: presentation) {
                    _ = store.requestWaitingLaunch(todo.id)
                }
                WAReviewAction("载入局部对齐示例记录", id: "todo.wait.align", presentation: presentation) {
                    store.loadLocalAlignmentRecord(todo.id)
                }
            }
            WAReviewActions {
                Menu {
                    ForEach(Array(WAReviewIntent.allCases.enumerated()), id: \.offset) { index, intent in
                        Button("载入\(intent.rawValue)示例记录") {
                            store.loadIntentRecord(intent, todoID: todo.id)
                        }
                        .accessibilityIdentifier("workArrangementsReview.todo.wait.intent.\(index)")
                    }
                } label: {
                    Text("载入当前意图记录")
                        .accessibilityIdentifier("workArrangementsReview.todo.wait.intent.menu")
                }
                WAReviewTruthMenu(
                    record: "本项尝试边界", id: "todo.wait.attempt"
                ) { store.loadAttemptPermissionRecord($0, todoID: todo.id) }
            }
        }
    }

    private func yes(_ value: Bool) -> String { value ? "是" : "否" }
}

@MainActor
private struct WAReviewRunDetail: View {
    let store: WorkArrangementsReviewStore
    let run: WAReviewExecution
    let presentation: WAReviewPresentation

    var body: some View {
        WAReviewPanel(title: "执行详情 · \(run.id)", id: "run.detail", presentation: presentation) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(run.phase.rawValue, id: "run.detail.phase", color: AeroricColor.accent)
                WAReviewText(
                    "关联目标：\(store.todo(run.todoID)?.title ?? "直接执行 · 无需待办")\n\(run.basis.summary)",
                    id: "run.detail.basis"
                )
                WAReviewText(
                    "原执行及占用安全：\(run.safety.rawValue)\n副作用核实：\(run.sideEffects.rawValue)\n"
                    + "仍持有相关占用：\(run.holdsOccupancy ? "是，不能判空闲" : "否（示例已核实）")\n"
                    + "收束原因：\(run.stopCause?.rawValue ?? "无停止请求")\n"
                    + "该次执行已载入失败记录：\(run.hasFailureRecord ? "是；结束不覆盖失败" : "否")\n"
                    + "权限依据：\(run.usesRuleAuthority ? "本规则" : "独立交办/独立工作")\n"
                    + "到限前自动续行同意：\(run.autoContinueGrantedBeforeLimit ? "有" : "无")；已形成后续执行：\(run.continued ? "是" : "否")",
                    id: "run.detail.safety"
                )
                WAReviewText(run.artifacts, id: "run.detail.artifacts")
            }
            Menu {
                ForEach(Array(WAReviewRunRecord.allCases.enumerated()), id: \.offset) { index, record in
                    Button("载入\(record.rawValue)示例记录") {
                        store.loadRunRecord(record, runID: run.id)
                    }
                    .accessibilityIdentifier("workArrangementsReview.run.record.\(index)")
                }
            } label: {
                Text("载入执行记录")
                    .accessibilityIdentifier("workArrangementsReview.run.record.menu")
            }
            WAReviewText(
                "退出、请求停止或未知回执均不证明安全释放；原执行、副作用与相关占用须载入安全收束记录。历史与成果不回滚，在途原目标及配置不被规则编辑静默替换。",
                id: "run.detail.boundary"
            )
            .background(AeroricColor.layer1)
            if let todoID = run.todoID {
                WAReviewAction("查看关联目标", id: "run.todo.open", presentation: presentation) {
                    store.selectedTodoID = todoID
                    store.section = .todos
                }
            }
        }
    }
}

@MainActor
private struct WAReviewRuleDetail: View {
    let store: WorkArrangementsReviewStore
    let presentation: WAReviewPresentation
    let edit: () -> Void

    var body: some View {
        WAReviewPanel(title: "规则与授权 · v\(store.rule.revision)", id: "rule.detail", presentation: presentation) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(
                    "启用：\(yes(store.rule.enabled))；策略已确认：\(yes(store.rule.policyConfirmed))\n"
                    + "用户暂停：\(yes(store.rule.paused))；用户 Stop：\(yes(store.rule.stopped))",
                    id: "rule.state"
                )
                WAReviewText(
                    "范围：\(store.rule.policy.scope.rawValue)\n"
                    + "配置：\(store.rule.policy.configuration.rawValue) / \(store.rule.policy.configuration.apiAlias)\n"
                    + "授权：\(store.rule.policy.permission.rawValue)\n触发：\(store.rule.policy.trigger.rawValue)\n"
                    + "恢复后补一次：\(yes(store.rule.policy.recoveryOnce))\n"
                    + "结束后补一次：\(yes(store.rule.policy.afterBusyOnce))\n"
                    + "到限补额后自动续行：\(yes(store.rule.policy.autoContinueAfterLimit))",
                    id: "rule.policy"
                )
                WAReviewText(
                    "允许操作（示例）：在明确范围内适配接口、补充说明及必需核验；不含新技能安装。资源仍受局部、项目和共同边界约束。Full Access 不免新技能人工同意、监督、Stop、撤权或硬限制。",
                    id: "rule.authority"
                )
            }
            WAReviewActions {
                WAReviewAction("编辑策略草稿", id: "rule.edit", presentation: presentation, action: edit)
                WAReviewAction(
                    "明确启用（示例）", id: "rule.enable", presentation: presentation, enabled: !store.rule.enabled
                ) { _ = store.enableRule() }
                WAReviewAction(
                    "停用规则（示例）", id: "rule.disable", presentation: presentation, enabled: store.rule.enabled
                ) { store.disableRule() }
            }
            WAReviewActions {
                WAReviewAction("暂停接单（示例）", id: "rule.pause", presentation: presentation) { store.pauseRule() }
                WAReviewAction("请求停止（示例）", id: "rule.stop", presentation: presentation, destructive: true) {
                    store.stopRule()
                }
                WAReviewAction("明确恢复范围", id: "rule.resume", presentation: presentation) {
                    _ = store.explicitlyResumeRule()
                }
            }
            WAReviewReading(presentation: presentation) {
                WAReviewText(
                    "首次启用不接存量；保存、确认策略、启用、到点、已开始是不同记录。未来新增只在当前有效覆盖内接单，不代表已开始。",
                    id: "rule.future"
                )
                ForEach(store.todos) { todo in
                    WAReviewText(
                        "\(todo.title)\n范围预览：\(store.previewReason(for: todo) ?? "条件匹配；不等于授权或运行")",
                        id: "rule.preview.\(todo.id)"
                    )
                }
            }
        }
    }

    private func yes(_ value: Bool) -> String { value ? "是" : "否" }
}

// MARK: - Visible context, explicit specimen records and resource boundaries

@MainActor
private struct WAReviewContext: View {
    let store: WorkArrangementsReviewStore
    let presentation: WAReviewPresentation

    var body: some View {
        WAReviewPanel(title: "当前依据、阻塞与事件", id: "context", presentation: presentation) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(WorkArrangementsReviewFixtures.explanation(for: store.scenario), id: "context.explanation")
                WAReviewText(store.message, id: "context.message", color: AeroricColor.accent)
                WAReviewText(
                    store.currentBlockers.isEmpty
                        ? "当前规则准入无阻塞；这不是已运行或目标已完成。"
                        : "当前规则阻塞：\n" + store.currentBlockers.joined(separator: "\n"),
                    id: "context.blockers", color: AeroricColor.warning
                )
                WAReviewText(launchDescription, id: "context.launchIntent")
                WAReviewText(
                    "繁忙后补一次待核对：\(store.busyFollowupPending ? "有一个当前意图" : "无")\n"
                    + "恢复后补一次资格：\(store.recoveryEligible ? "具备，仍须现行重核" : "无")",
                    id: "context.followups"
                )
                if store.advancedPresentation {
                    WAReviewText(
                        "advanced 只展示内存记录标识：generation \(store.generation)，"
                        + "待办 \(store.todos.count)，执行 \(store.executions.count)，规则 v\(store.rule.revision)。没有额外执行权限。",
                        id: "context.advanced"
                    )
                }
            }
            WAReviewActions {
                WAReviewAction("直接检查（仅意图）", id: "launch.direct", presentation: presentation) {
                    _ = store.requestLaunch()
                }
                WAReviewAction("载入已开始示例记录", id: "launch.loadStarted", presentation: presentation) {
                    _ = store.loadStartedRecord()
                }
            }
            scenarioRecords
            gates
            if store.scenario == .w8 { resources }
            opportunities
            eventHistory
            WAReviewText(
                "仅本窗口内存。App、Preview、真实交互/材质、AX/键盘、窄窗与关闭重开生命周期均 NOT RUN / DEFERRED。",
                id: "context.deferred"
            )
            .background(AeroricColor.layer1)
        }
    }

    private var launchDescription: String {
        guard let intent = store.launchIntent else { return "启动意图：尚无；交办、保存与补额都不是已开始事实。" }
        return "当前启动意图：\(intent.origin.rawValue)\n"
            + "目标：\(store.todo(intent.todoID)?.title ?? "无需待办的当前状态检查")\n"
            + "到限旧执行：\(intent.continuingRunID ?? "不适用")；仍须载入已开始记录。"
    }

    @ViewBuilder private var scenarioRecords: some View {
        switch store.scenario {
        case .w1:
            WAReviewText("目标中审阅两次不同结果及独立验收；执行列表保留无需待办的直接执行。", id: "records.w1")
                .background(AeroricColor.layer1)
        case .w2:
            WAReviewAction("载入新增示例记录", id: "records.arrival", presentation: presentation) {
                store.loadArrivalRecord()
            }
        case .w3:
            WAReviewText("在规则详情分别暂停、请求停止及明确恢复；在执行详情载入未知或安全收束回执。", id: "records.w3")
                .background(AeroricColor.layer1)
        case .w4:
            WAReviewActions {
                WAReviewAction("载入撤权示例记录", id: "records.revocation", presentation: presentation) {
                    store.loadRevocationRecord()
                }
                scheduledAction
            }
        case .w5:
            WAReviewActions {
                WAReviewAction("载入三次漏跑示例记录", id: "records.missed", presentation: presentation) {
                    store.loadMissedRecords()
                }
                WAReviewAction("载入恢复可用示例记录", id: "records.recovery", presentation: presentation) {
                    store.loadRecoveryRecord()
                }
                scheduledAction
            }
        case .w6:
            WAReviewActions {
                WAReviewAction("载入三次繁忙示例记录", id: "records.busy", presentation: presentation) {
                    store.loadBusyRecords()
                }
                scheduledAction
            }
        case .w7:
            WAReviewReading(presentation: presentation) {
                WAReviewText("共享修改点：\(store.sharedPoint.rawValue)", id: "records.sharedPoint")
                WAReviewTruthMenu(record: "共享修改点及原占用安全", id: "records.sharedPoint.load") {
                    store.loadSharedPointRecord($0)
                }
                WAReviewAction("载入输入变化示例记录", id: "records.inputChanged", presentation: presentation) {
                    store.loadInputChangedRecord()
                }
                WAReviewText(
                    "安全释放/局部对齐只重核既有获准等待，至多呈现一个启动意图；不会造执行或把跳过变队列。请选择等待项审阅当前意图和尝试边界；独立文档在执行列表中保留。",
                    id: "records.waiting.help"
                )
            }
        case .w8:
            scheduledAction
        }
    }

    private var scheduledAction: some View {
        WAReviewAction("载入新到点示例记录", id: "records.schedule", presentation: presentation) {
            store.loadScheduledRecord()
        }
    }

    private var gates: some View {
        DisclosureGroup("当前条件与失败 / 未知 / 未答示例") {
            WAReviewReading(presentation: presentation) {
                ForEach(Array(WAReviewGate.allCases.enumerated()), id: \.offset) { index, gate in
                    WAReviewText(
                        "\(gate.rawValue)：\((store.gates[gate] ?? .unknown).rawValue)",
                        id: "gate.\(index).value"
                    )
                    WAReviewTruthMenu(record: gate.rawValue, id: "gate.\(index).load") {
                        store.loadGateRecord(gate, value: $0)
                    }
                }
            }
            .padding(.top, NativePanelGeometry.panelInset)
        }
    }

    private var resources: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            WAReviewReading(presentation: presentation) {
                WAReviewText(
                    "W8 · 护栏与用量（固定虚构单位，非账单）\n"
                    + "并发/尝试只挡新准入；在途消耗/时长到限请求安全收束。独立时长只约束到限旧执行续行，不冻结规则新的合法触发。",
                    id: "resource.boundary"
                )
                ForEach(Array(WAReviewLimit.allCases.enumerated()), id: \.offset) { index, limit in
                    WAReviewText(
                        "\(limit.rawValue)：\((store.limits[limit] ?? .unknown).rawValue)\n"
                        + (limit.affectsInFlight ? "对受影响在途生效；未核实不能报已停。" : "仅新准入/下一次尝试；不终止最后一次仍合规尝试。"),
                        id: "limit.\(index).value"
                    )
                    WAReviewTruthMenu(record: limit.rawValue, id: "limit.\(index).load") {
                        store.loadLimitRecord(limit, value: $0)
                    }
                }
            }
            WAReviewAction("载入用量示例记录", id: "usage.load", presentation: presentation) {
                store.loadUsageRecords()
            }
            ForEach(Array(WAReviewBudgetScope.allCases.enumerated()), id: \.offset) { index, scope in
                WAReviewReading(presentation: presentation) {
                    WAReviewText(scope.rawValue, id: "usage.scope.\(index).title")
                    WAReviewText(store.usageSummary(scope: scope).text, id: "usage.scope.\(index).summary")
                    WAReviewActions {
                        WAReviewAction("补额（示例）", id: "usage.scope.\(index).topUp", presentation: presentation) {
                            store.topUp(scope)
                        }
                        WAReviewAction(
                            "补额并继续（示例）", id: "usage.scope.\(index).continue", presentation: presentation,
                            enabled: store.latestLimitedRun != nil
                        ) {
                            if let run = store.latestLimitedRun {
                                store.topUp(scope, continuingRunID: run.id)
                            }
                        }
                    }
                }
            }
            WAReviewReading(presentation: presentation) {
                WAReviewText(
                    "到限旧执行：\(store.latestLimitedRun?.id ?? "无")。补额并继续仅提交该执行续行意图；未安全收束时 Store 拒绝。补额不清独立时长、尝试、Stop、暂停或缺监督，不重放未知副作用。",
                    id: "resource.continuation"
                )
                ForEach(store.usage) { usage in
                    WAReviewText(
                        "\(usage.role)：\(usage.kind.label)\n关联：\(usage.scopes.map(\.rawValue).joined(separator: " / "))",
                        id: "usage.record.\(usage.id)"
                    )
                }
                if store.usage.isEmpty {
                    WAReviewText("尚未载入用量；无报告不等于消耗为零。", id: "usage.empty")
                }
                WAReviewText(
                    "同笔记录不重复累计；三作用域是关联视图，不能相加当总账。Full Access 不免新技能人工同意、适用监督或必需硬限能力。",
                    id: "usage.completeness"
                )
            }
        }
    }

    private var opportunities: some View {
        WAReviewReading(presentation: presentation) {
            WAReviewText("机会记录 · 跳过不等于成功或等待队列", id: "opportunity.heading")
            ForEach(store.opportunities) { opportunity in
                WAReviewText(opportunity.description, id: "opportunity.\(opportunity.id)")
            }
            if store.opportunities.isEmpty {
                WAReviewText("尚未载入漏跑或繁忙机会记录。", id: "opportunity.empty")
            }
        }
    }

    private var eventHistory: some View {
        WAReviewReading(presentation: presentation) {
            WAReviewText("事件记录（\(store.events.count)）", id: "event.heading")
            ForEach(store.events) { event in
                WAReviewText("\(event.title)\n\(event.detail)", id: "event.\(event.id)")
            }
            if store.events.isEmpty {
                WAReviewText("只有初始虚构数据；尚无后续动作记录。", id: "event.empty")
            }
        }
    }
}

// MARK: - Local drafts; dismissal never applies changes

private struct WAReviewSheet: Identifiable {
    enum Draft {
        case todo(WAReviewNewTodoDraft)
        case rule(WAReviewRuleDraft)
        case batch(WAReviewBatchDraft)
    }
    let id = UUID()
    let draft: Draft
}

@MainActor
private struct WAReviewTodoSheet: View {
    let store: WorkArrangementsReviewStore
    let presentation: WAReviewPresentation
    @State private var draft: WAReviewNewTodoDraft
    @State private var feedback = ""
    @Environment(\.dismiss) private var dismiss

    init(store: WorkArrangementsReviewStore, draft: WAReviewNewTodoDraft, presentation: WAReviewPresentation) {
        self.store = store
        self.presentation = presentation
        _draft = State(initialValue: draft)
    }

    var body: some View {
        WAReviewSheetLayout(title: "新建目标 · 只保存示例记录", id: "draft.todo", presentation: presentation) {
            TextField("目标（不会自动交办或运行）", text: $draft.title, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("workArrangementsReview.draft.todo.title")
            Picker("虚构来源", selection: $draft.source) {
                ForEach(WAReviewSource.allCases, id: \.self) { source in
                    Text(source.rawValue).tag(source)
                }
            }
            .pickerStyle(.menu)
            Picker("清单与标签", selection: $draft.category) {
                ForEach(WAReviewCategory.allCases, id: \.self) { category in
                    Text(category.rawValue).tag(category)
                }
            }
            .pickerStyle(.menu)
            Toggle("仅记一下，不交办", isOn: $draft.recordOnly)
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("workArrangementsReview.draft.todo.recordOnly")
            WAReviewText(
                "来源：\(draft.source.rawValue)\n\(draft.category.rawValue)\n创建不授予任何执行权；取消直接丢弃本地草稿。",
                id: "draft.todo.boundary"
            )
            WAReviewText(feedback, id: "draft.todo.feedback", color: AeroricColor.warning)
        } actions: {
            Button("取消", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .accessibilityIdentifier("workArrangementsReview.draft.todo.cancel")
            Button("保存记录（示例）") {
                if store.saveNewTodo(draft) { dismiss() } else { feedback = store.message }
            }
            .modifier(WAReviewShortButtonStyle(presentation: presentation))
            .keyboardShortcut(.defaultAction)
            .accessibilityIdentifier("workArrangementsReview.draft.todo.save")
        }
    }
}

@MainActor
private struct WAReviewRuleSheet: View {
    let store: WorkArrangementsReviewStore
    let presentation: WAReviewPresentation
    @State private var draft: WAReviewRuleDraft
    @State private var feedback = ""
    @Environment(\.dismiss) private var dismiss

    init(store: WorkArrangementsReviewStore, draft: WAReviewRuleDraft, presentation: WAReviewPresentation) {
        self.store = store
        self.presentation = presentation
        _draft = State(initialValue: draft)
    }

    var body: some View {
        WAReviewSheetLayout(title: "规则策略草稿 · v\(draft.baseRevision)", id: "draft.rule", presentation: presentation) {
            policyFields
            Toggle("恢复后补一次当前检查", isOn: $draft.edited.recoveryOnce)
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("workArrangementsReview.draft.rule.recoveryOnce")
            Toggle("繁忙结束后补一次当前检查（独立设置）", isOn: $draft.edited.afterBusyOnce)
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("workArrangementsReview.draft.rule.afterBusyOnce")
            Toggle("事先同意到限补额后自动续行（仍须安全核实）", isOn: $draft.edited.autoContinueAfterLimit)
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("workArrangementsReview.draft.rule.autoContinue")
            WAReviewReading(presentation: presentation) {
                WAReviewText("变更差异与影响\n\(draft.difference)", id: "draft.rule.difference")
                WAReviewText(
                    "新启动重核新策略；在途保留原目标与配置/API 别名。扩围和提权须明确确认。保存或确认策略都不调用启用，也不回填存量；已有暂停/Stop 不被清除。",
                    id: "draft.rule.impact"
                )
                WAReviewText(
                    "允许操作：范围内适配、说明及必需核验；资源仍须满足全部适用边界。Full Access 不免新技能人工同意或监督。",
                    id: "draft.rule.authority"
                )
                ForEach(store.todos) { todo in
                    WAReviewText(
                        "\(todo.title)\n草稿预览：\(store.previewReason(for: todo, policy: draft.edited) ?? "条件匹配；预览不授运行权")",
                        id: "draft.rule.preview.\(todo.id)"
                    )
                }
            }
            WAReviewText(feedback, id: "draft.rule.feedback", color: AeroricColor.warning)
        } actions: {
            Button("取消", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .accessibilityIdentifier("workArrangementsReview.draft.rule.cancel")
            Button("仅保存草稿策略") { apply(confirming: false) }
                .modifier(WAReviewShortButtonStyle(presentation: presentation))
                .accessibilityIdentifier("workArrangementsReview.draft.rule.save")
            Button("保存并确认策略") { apply(confirming: true) }
                .modifier(WAReviewShortButtonStyle(presentation: presentation))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("workArrangementsReview.draft.rule.confirm")
        }
    }

    private var policyFields: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Picker("有限范围", selection: $draft.edited.scope) {
                ForEach(WAReviewScope.allCases, id: \.self) { scope in Text(scope.rawValue).tag(scope) }
            }
            Picker("配置别名", selection: $draft.edited.configuration) {
                ForEach(WAReviewConfiguration.allCases, id: \.self) { configuration in
                    Text("\(configuration.rawValue) / \(configuration.apiAlias)").tag(configuration)
                }
            }
            Picker("本规则授权", selection: $draft.edited.permission) {
                ForEach(WAReviewPermission.allCases, id: \.self) { permission in Text(permission.rawValue).tag(permission) }
            }
            Picker("触发类别", selection: $draft.edited.trigger) {
                ForEach(WAReviewTrigger.allCases, id: \.self) { trigger in Text(trigger.rawValue).tag(trigger) }
            }
        }
        .pickerStyle(.menu)
    }

    private func apply(confirming: Bool) {
        if store.applyRuleDraft(draft, confirmingPolicy: confirming) {
            dismiss()
        } else {
            feedback = store.message
        }
    }
}

@MainActor
private struct WAReviewBatchSheet: View {
    let store: WorkArrangementsReviewStore
    let presentation: WAReviewPresentation
    @State private var draft: WAReviewBatchDraft
    @State private var feedback = ""
    @Environment(\.dismiss) private var dismiss

    init(store: WorkArrangementsReviewStore, draft: WAReviewBatchDraft, presentation: WAReviewPresentation) {
        self.store = store
        self.presentation = presentation
        _draft = State(initialValue: draft)
    }

    var body: some View {
        WAReviewSheetLayout(title: "\(draft.kind.rawValue)（示例）", id: "draft.batch", presentation: presentation) {
            WAReviewText(
                "逐项选择并独立排除；排除优先。批量明确交办仅授本项有限意图；纳入旧项另需本规则已启用且策略有效。取消不保存选择或排除。",
                id: "draft.batch.boundary"
            )
            ForEach(draft.candidates, id: \.self) { id in
                if let todo = store.todo(id) {
                    WAReviewReading(presentation: presentation) {
                        WAReviewText("\(todo.title)\n\(todo.source.rawValue)", id: "draft.batch.\(id).title")
                        Toggle("选择此项", isOn: membership(id, excluded: false))
                            .toggleStyle(.checkbox)
                            .accessibilityIdentifier("workArrangementsReview.draft.batch.\(id).selected")
                        Toggle("排除此项", isOn: membership(id, excluded: true))
                            .toggleStyle(.checkbox)
                            .accessibilityIdentifier("workArrangementsReview.draft.batch.\(id).excluded")
                        WAReviewText(reason(for: todo), id: "draft.batch.\(id).reason")
                    }
                } else {
                    WAReviewText("记录已变化；旧草稿不能应用。", id: "draft.batch.\(id).missing")
                }
            }
            WAReviewText(
                "已选 \(draft.selected.count) 项，排除 \(draft.excluded.count) 项；确认时 Store 重核本窗口 generation、对象集合与规则版本。",
                id: "draft.batch.count"
            )
            WAReviewText(feedback, id: "draft.batch.feedback", color: AeroricColor.warning)
        } actions: {
            Button("取消", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .accessibilityIdentifier("workArrangementsReview.draft.batch.cancel")
            Button("确认所选（仅示例意图）") {
                if store.confirmBatch(draft) { dismiss() } else { feedback = store.message }
            }
            .modifier(WAReviewShortButtonStyle(presentation: presentation))
            .keyboardShortcut(.defaultAction)
            .accessibilityIdentifier("workArrangementsReview.draft.batch.confirm")
        }
    }

    private func membership(_ id: String, excluded: Bool) -> Binding<Bool> {
        Binding(
            get: { excluded ? draft.excluded.contains(id) : draft.selected.contains(id) },
            set: { value in
                if excluded {
                    if value { draft.excluded.insert(id) } else { draft.excluded.remove(id) }
                } else {
                    if value { draft.selected.insert(id) } else { draft.selected.remove(id) }
                }
            }
        )
    }

    private func reason(for todo: WAReviewTodo) -> String {
        var proposed = todo
        proposed.excluded = draft.excluded.contains(todo.id)
        let reason = store.previewReason(for: proposed)
        if draft.kind == .assignment {
            return "当前规则范围预览：\(reason ?? "匹配")；独立交办不继承规则授权，确认仍须重核。"
        }
        return "纳入范围预览：\(reason ?? "条件匹配；尚未纳入或运行")"
    }
}

// MARK: - Small local assemblies of existing native primitives

private struct WAReviewPresentation {
    let surface: SurfacePreference
    let reduceTransparency: Bool
    let increaseContrast: Bool
    let reduceMotion: Bool

    var animation: Animation? {
        let duration = AeroricMotion.duration(AeroricMotion.standard, reduceMotion: reduceMotion)
        return reduceMotion ? nil : .easeInOut(duration: duration)
    }
}

private struct WAReviewText: View {
    let text: String
    let id: String
    var color: Color = AeroricColor.text

    init(_ text: String, id: String, color: Color = AeroricColor.text) {
        self.text = text
        self.id = id
        self.color = color
    }

    var body: some View {
        Text(text)
            .foregroundStyle(color)
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
            .accessibilityIdentifier("workArrangementsReview.\(id)")
    }
}

private struct WAReviewPanel<Content: View>: View {
    let title: String
    let id: String
    let presentation: WAReviewPresentation
    let content: Content

    init(title: String, id: String, presentation: WAReviewPresentation, @ViewBuilder content: () -> Content) {
        self.title = title
        self.id = id
        self.presentation = presentation
        self.content = content()
    }

    var body: some View {
        GlassPanel(
            preference: presentation.surface,
            reduceTransparency: presentation.reduceTransparency,
            increaseContrast: presentation.increaseContrast
        ) {
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                WAReviewText(title, id: "\(id).heading").font(.headline)
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NativePanelGeometry.panelInset)
        }
    }
}

private struct WAReviewReading<Content: View>: View {
    let presentation: WAReviewPresentation
    let content: Content

    init(presentation: WAReviewPresentation, @ViewBuilder content: () -> Content) {
        self.presentation = presentation
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NativePanelGeometry.panelInset)
            .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.s))
            .overlay {
                let outline = NativePanelGeometry.outline(increasedContrast: presentation.increaseContrast)
                RoundedRectangle(cornerRadius: AeroricRadius.s)
                    .strokeBorder(AeroricColor.separator2.opacity(outline.opacity), lineWidth: outline.width)
                    .allowsHitTesting(false)
            }
    }
}

private struct WAReviewActions<Content: View>: View {
    let content: Content
    init(@ViewBuilder content: () -> Content) { self.content = content() }

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: NativePanelGeometry.panelGap) { content }
                .fixedSize(horizontal: true, vertical: false)
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) { content }
        }
    }
}

private struct WAReviewShortButtonStyle: ViewModifier {
    let presentation: WAReviewPresentation

    @ViewBuilder func body(content: Content) -> some View {
        if #available(macOS 26.0, *), presentation.surface == .glass, !presentation.reduceTransparency {
            content
                .buttonStyle(.glass)
                .buttonBorderShape(.capsule)
                .fontWeight(presentation.increaseContrast ? .semibold : .regular)
                .controlSize(.small)
        } else {
            content
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .fontWeight(presentation.increaseContrast ? .semibold : .regular)
                .controlSize(.small)
        }
    }
}

private struct WAReviewAction: View {
    let title: String
    let id: String
    let presentation: WAReviewPresentation
    var enabled = true
    var destructive = false
    let action: () -> Void

    init(
        _ title: String, id: String, presentation: WAReviewPresentation,
        enabled: Bool = true, destructive: Bool = false, action: @escaping () -> Void
    ) {
        self.title = title
        self.id = id
        self.presentation = presentation
        self.enabled = enabled
        self.destructive = destructive
        self.action = action
    }

    var body: some View {
        Button(role: destructive ? .destructive : nil, action: action) {
            Text(title).fixedSize(horizontal: false, vertical: true)
        }
        .modifier(WAReviewShortButtonStyle(presentation: presentation))
        .tint(destructive ? AeroricColor.danger : AeroricColor.accent)
        .disabled(!enabled)
        .accessibilityIdentifier("workArrangementsReview.\(id)")
    }
}

private struct WAReviewTruthMenu: View {
    let record: String
    let id: String
    let action: (WAReviewTruth) -> Void

    var body: some View {
        Menu {
            ForEach(Array(WAReviewTruth.allCases.enumerated()), id: \.offset) { index, value in
                Button("载入\(record)示例记录：\(value.rawValue)") { action(value) }
                    .accessibilityIdentifier("workArrangementsReview.\(id).\(index)")
            }
        } label: {
            Text("载入\(record)记录")
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("workArrangementsReview.\(id).menu")
        }
        .controlSize(.small)
    }
}

private struct WAReviewSheetLayout<Content: View, Actions: View>: View {
    let title: String
    let id: String
    let presentation: WAReviewPresentation
    let content: Content
    let actions: Actions

    init(
        title: String, id: String, presentation: WAReviewPresentation,
        @ViewBuilder content: () -> Content, @ViewBuilder actions: () -> Actions
    ) {
        self.title = title
        self.id = id
        self.presentation = presentation
        self.content = content()
        self.actions = actions()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            WAReviewText(title, id: "\(id).heading").font(.headline)
            WAReviewText(WorkArrangementsReviewFixtures.disclaimer, id: "\(id).disclaimer", color: AeroricColor.accent)
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) { content }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, NativePanelGeometry.panelInset)
            }
            Divider()
            WAReviewActions { actions }
        }
        .padding(NativePanelGeometry.panelInset)
        .background(AeroricColor.layer1)
        .transaction { transaction in
            if presentation.reduceMotion {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
    }
}
#endif
