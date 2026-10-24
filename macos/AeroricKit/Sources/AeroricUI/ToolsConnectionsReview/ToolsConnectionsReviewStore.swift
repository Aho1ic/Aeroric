#if DEBUG
import Observation

/// One review root owns one instance. No production owner, callbacks or I/O.
@MainActor @Observable
final class ToolsConnectionsReviewStore {
    private(set) var scenario: ToolsConnectionsReviewScenario = .TC1
    private(set) var variant: TConnReviewVariant = .standard
    private(set) var context: ToolsConnectionsReviewContext = .alphaOne
    var category: TConnReviewCategory = .capability
    private(set) var selectedID = ToolsConnectionsReviewFixtures.queryID
    private(set) var configuredIDs: Set<String> = []
    private(set) var projectEnabled: [String: Set<String>] = [:]
    private(set) var taskEnabled: [ToolsConnectionsReviewContext: Set<String>] = [:]
    private(set) var taskReferences: [ToolsConnectionsReviewContext: Set<String>] = [:]
    private(set) var temporaryReferences: [ToolsConnectionsReviewContext: Set<String>] = [:]
    private(set) var authorizedTasks: Set<ToolsConnectionsReviewContext> = []
    var scopeDraft: TConnReviewScopeDraft?
    private(set) var disableDraft: TConnReviewScopeDraft?
    private(set) var reviewDraft: TConnReviewIntroductionDraft?
    private(set) var approvedIntents: [TConnReviewApprovedIntent] = []
    // Current selection is independent of the deduplicated approval history.
    // Returning to an already-approved operation must still select its version.
    private var activeIntroductions: [ToolsConnectionsReviewContext: [String: TConnReviewIntroductionDraft]] = [:]
    private var skillConsents: Set<TConnReviewSkillConsent> = []
    private var constraints: [ToolsConnectionsReviewContext: TConnReviewUserConstraint] = [:]
    private var constrainedItems: [ToolsConnectionsReviewContext: String] = [:]
    private(set) var proposedID: String?
    private(set) var message = "仅查看，不探测、不执行。"
    private(set) var disabledScopes: Set<String> = []
    private(set) var stopStates: [String: TConnReviewStopState] = [:]
    private(set) var continuation = TConnReviewContinuation()
    private(set) var credentialState: TConnReviewCredentialState = .referenced
    private var credentialMessages: [ToolsConnectionsReviewContext: [String: String]] = [:]
    private(set) var results: [TConnReviewResult] = []
    private(set) var expandedResultIDs: Set<String> = []
    private(set) var loadedEvents: Set<String> = []
    private(set) var eventLog: [String] = []
    private(set) var installedFact: TConnReviewFact = .blocked("外部候选未安装")
    private(set) var candidateVersionFact: TConnReviewFact = .unknown("v2 结果尚无依据")
    private(set) var updateHasFailed = false
    private var resumeAfterStopIntent = false
    let identity = TConnReviewIdentity()
    let inFlightVersion = "demo-v1 · 固定虚构在途版本"
    let businessHistory = ToolsConnectionsReviewFixtures.history
    // Illustrative invariants, not a security or isolation mechanism.
    let externalExecutionCount = 0
    let replayCount = 0
    let independentWork = "独立文档工作可继续；不关闭其它项目共享服务。"

    init(scenario: ToolsConnectionsReviewScenario = .TC1, variant: TConnReviewVariant = .standard) {
        loadScenario(scenario, variant: variant)
    }

    var catalog: [ToolsConnectionsReviewCatalogItem] { ToolsConnectionsReviewFixtures.catalog }
    var visibleItems: [ToolsConnectionsReviewCatalogItem] { catalog.filter { $0.category == category } }
    var selectedItem: ToolsConnectionsReviewCatalogItem {
        catalog.first { $0.id == selectedID } ?? catalog[1]
    }
    var userConstraint: TConnReviewUserConstraint { constraints[context] ?? .automatic }
    var constrainedID: String? { constrainedItems[context] }
    var skillManuallyConsented: Bool {
        guard let draft = currentIntroductionDraft else { return false }
        return skillConsents.contains(skillAnchor(draft))
    }
    var credentialMessage: String {
        credentialMessages[context]?[selectedID]
            ?? "本任务/条目只有虚构引用，未记录使用覆盖；没有可读取或导出的秘密值。"
    }
    var scopeSummary: String {
        let reference = taskReferences[context, default: []].contains(selectedID)
        let temporary = temporaryReferences[context, default: []].contains(selectedID)
        return "保存：\(configuredIDs.contains(selectedID) ? "有" : "无")；"
            + "项目启用：\(isProjectEnabled(selectedID, in: context) ? "是" : "否")；"
            + "任务启用：\(taskEnabled[context, default: []].contains(selectedID) ? "是" : "否")；"
            + "任务引用：\(reference ? (temporary ? "仅任务临时引用" : "已有引用") : "无")；"
            + "操作授权：\(authorizedTasks.contains(context) ? "示例覆盖" : "未覆盖")"
    }
    var detailLayers: [TConnReviewLayer] {
        let item = selectedItem
        return [
            .init(id: "installation", title: "配置/安装", fact:
                item.id == ToolsConnectionsReviewFixtures.externalID ? installedFact : item.installation),
            .init(id: "configuration", title: "配置保存", fact:
                configuredIDs.contains(item.id) ? .ready("已保存示例配置，不自动启用") : .blocked("未保存示例配置")),
            .init(id: "scope", title: "启用范围", fact:
                isEnabled(item.id, in: context) ? .ready(scopeSummary) : .blocked("当前范围未启用；其它项目不能借用")),
            .init(id: "support", title: "执行器支持", fact: item.support),
            .init(id: "service", title: "服务/连接", fact: serviceFact(for: item)),
            .init(id: "authorization", title: "任务操作授权", fact:
                authorizedTasks.contains(context) ? .ready("仅覆盖示例开发目标读取，不含安装/导出")
                    : .blocked("本任务未获开发目标访问；服务运行不代替授权")),
        ]
    }

    func isProjectEnabled(_ itemID: String, in target: ToolsConnectionsReviewContext) -> Bool {
        guard let project = target.project else { return false }
        return projectEnabled[project, default: []].contains(itemID)
    }

    func isEnabled(_ itemID: String, in target: ToolsConnectionsReviewContext) -> Bool {
        (isProjectEnabled(itemID, in: target) || taskEnabled[target, default: []].contains(itemID))
            && !isDisabled(itemID, in: target)
    }

    func isDisabled(_ itemID: String, in target: ToolsConnectionsReviewContext) -> Bool {
        disabledScopes.contains(scopeKey(.task, context: target, itemID: itemID))
            || (target.project != nil
                && disabledScopes.contains(scopeKey(.project, context: target, itemID: itemID)))
    }

    func eligibility(for itemID: String) -> TConnReviewProjection {
        guard let item = catalog.first(where: { $0.id == itemID }) else {
            return .init(eligible: false, title: "未知条目", reasons: ["没有当前条目依据"])
        }
        var reasons: [String] = []
        if !configuredIDs.contains(itemID) { reasons.append("未保存配置") }
        let installation = itemID == ToolsConnectionsReviewFixtures.externalID ? installedFact : item.installation
        if installation.state != .available && installation.state != .notApplicable {
            reasons.append(installation.text)
        }
        if !isEnabled(itemID, in: context) { reasons.append("未启用或已停用当前范围") }
        // A connection's back-reference is catalog navigation, not a cyclic
        // prerequisite. Methods/capabilities use the same referenced objects.
        for reference in item.references where item.category != .connection {
            guard let dependency = catalog.first(where: { $0.id == reference }) else {
                reasons.append("依赖 \(reference) 未知")
                continue
            }
            if !configuredIDs.contains(reference) { reasons.append("依赖 \(reference) 未配置") }
            if !isEnabled(reference, in: context) {
                reasons.append("依赖 \(reference) 未启用或已停用，不能绕过")
            }
            if dependency.support.state != .available { reasons.append(dependency.support.text) }
            let dependencyService = serviceFact(for: dependency)
            if dependencyService.state != .available { reasons.append(dependencyService.text) }
            if userConstraint == .disabled && constrainedID == reference {
                reasons.append("依赖 \(reference) 被用户禁用，不能绕过")
            }
        }
        if item.support.state != .available { reasons.append(item.support.text) }
        let service = serviceFact(for: item)
        if service.state != .available { reasons.append(service.text) }
        if !authorizedTasks.contains(context) { reasons.append("本任务操作授权不足") }
        if item.target != identity.target { reasons.append("目标不匹配，不暗换目标") }
        if itemID == constrainedID && userConstraint == .disabled { reasons.append("用户禁用优先") }
        if userConstraint == .specified && itemID != constrainedID { reasons.append("不在用户指定集合") }
        if userConstraint == .ambiguousTarget { reasons.append("目标有实质歧义，待澄清") }
        return .init(eligible: reasons.isEmpty, title: reasons.isEmpty ? "可拟选 · 未调用" : "局部受阻", reasons: reasons)
    }

    func selectItem(_ id: String) {
        guard let item = catalog.first(where: { $0.id == id }) else { return }
        selectedID = id
        category = item.category
        discardDrafts()
        proposedID = nil
        message = "查看详情不会安装、连接、探测或授权。"
    }

    func selectContext(_ next: ToolsConnectionsReviewContext) {
        context = next
        discardDrafts()
        proposedID = nil
        message = "已切换示例上下文；未复制项目/任务授权。"
    }

    func selectCategory(_ next: TConnReviewCategory) {
        guard let first = catalog.first(where: { $0.category == next }) else { return }
        selectItem(first.id)
    }

    func setUserConstraint(_ constraint: TConnReviewUserConstraint) {
        constraints[context] = constraint
        constrainedItems[context] = constraint == .automatic ? nil : selectedID
        proposedID = nil
    }

    func proposeSelection() {
        let candidates = userConstraint == .specified
            ? [constrainedID ?? selectedID] : catalog.filter { $0.category == .capability }.map(\.id)
        proposedID = candidates.first { eligibility(for: $0).eligible }
        message = proposedID.map { "示例拟选 \($0)；尚未调用、读取或采用。" }
            ?? "没有可拟选项；\(eligibility(for: selectedID).reasons.joined(separator: "；"))。\(independentWork)"
    }

    func beginScopeDraft() {
        scopeDraft = .init(itemID: selectedID, context: context, scope: .task, enabled: true, temporary: true)
    }

    func cancelScopeDraft() {
        scopeDraft = nil
        message = "范围草稿已取消，没有应用。"
    }

    func commitScopeDraft() {
        guard let draft = scopeDraft, draft.itemID == selectedID, draft.context == context else { return }
        if draft.scope == .project {
            guard context.project != nil else {
                message = "无项目任务只能使用任务范围，不会先创建项目。"
                return
            }
            setProjectEnabled(draft.enabled)
        } else {
            invalidateIntroductionIntents(scope: .task)
            if draft.enabled { taskEnabled[context, default: []].insert(selectedID) }
            else { taskEnabled[context, default: []].remove(selectedID) }
            if draft.enabled { referenceForTask(temporary: draft.temporary) }
        }
        scopeDraft = nil
        reviewDraft = nil
        message = "示例范围已保存；未改变操作授权或外部事实。"
    }

    func saveConfiguration() {
        configuredIDs.insert(selectedID)
        message = "仅保存示例配置；没有全局/项目启用或操作授权。"
    }

    func setProjectEnabled(_ enabled: Bool) {
        guard let project = context.project else {
            message = "无项目任务不写项目默认，请明确使用任务范围。"
            return
        }
        if enabled { projectEnabled[project, default: []].insert(selectedID) }
        else { projectEnabled[project, default: []].remove(selectedID) }
        invalidateIntroductionIntents(scope: .project)
        reviewDraft = nil
        proposedID = nil
    }

    func referenceForTask(temporary: Bool = false) {
        guard configuredIDs.contains(selectedID) else {
            message = "尚未保存示例配置；引用不自动安装或补配置。"
            return
        }
        guard temporary || context.project == nil || isEnabled(selectedID, in: context) else {
            message = "项目尚未启用；可明确选择仅本任务临时用途。"
            return
        }
        taskReferences[context, default: []].insert(selectedID)
        if temporary || context.project == nil {
            invalidateIntroductionIntents(scope: .task)
            temporaryReferences[context, default: []].insert(selectedID)
            taskEnabled[context, default: []].insert(selectedID)
        }
        message = "已记录示例任务引用；不授操作权限，不执行，不写回项目默认。"
    }

    func beginIntroductionReview(_ operation: TConnReviewOperation = .install) {
        guard scenario == .TC3, selectedID == ToolsConnectionsReviewFixtures.externalID else {
            message = "本引入/更新示例只属于明确的外部候选，不复用到其它条目。"
            return
        }
        let draft = TConnReviewIntroductionDraft(
            operation: operation, itemID: selectedID, context: context, target: identity.target,
            version: operation == .install ? "demo-v1" : (operation == .update ? "demo-v2" : "demo-v1"),
            source: "demo-publisher · 版本依据为虚构固定清单，未认证来源可信",
            location: variant == .remoteUnpinned ? "demo-remote-service · 接入不是本机安装" : "demo-execution-location",
            dependencies: "虚构依赖一项；示例安装脚本/服务启动/联网均未执行",
            differences: operation == .update ? "v2 新增查询字段/依赖，网络去向及权限需重核；不暗换在途 v1"
                : "仅覆盖列示范围；不涵盖所有项目、资源或未来版本",
            permissions: "仅 demo-development-target；文件/网络/凭据范围均须列示，不自动供给",
            introducesSkill: variant == .bundledSkill
        )
        let covered = context == .alphaOne && isEnabled(selectedID, in: context)
            && ((operation == .install && (variant == .installCovered || variant == .bundledSkill))
                || (operation == .update && variant == .updateCovered))
        if operation == .rollback && !rollbackProjection.eligible {
            message = rollbackProjection.reasons.joined(separator: "；")
            return
        }
        if covered {
            approve(draft, preauthorized: true)
            reviewDraft = nil
            message = "明确示例预授权覆盖，无需重复安装/更新审批；仅记录未执行意图。"
        } else {
            reviewDraft = draft
            message = "集中审阅示例范围；确认只记录未执行意图。"
        }
    }

    func cancelReview() {
        reviewDraft = nil
        message = "审阅已取消，未批准或执行。"
    }

    func confirmReviewIntent() {
        guard let draft = reviewDraft, draft.itemID == selectedID, draft.context == context else { return }
        if draft.operation == .rollback && !rollbackProjection.eligible {
            message = "回退条件不满足，不批准示例意图。"
            return
        }
        approve(draft, preauthorized: false)
        reviewDraft = nil
        message = "示例意图已获准 · 尚未执行；安装/服务/连接可用性仍需独立依据。"
    }

    func consentToBundledSkill() {
        guard scenario == .TC3, variant == .bundledSkill,
            let draft = currentIntroductionDraft
        else {
            message = "请先明确本条目、版本与任务的引入/更新意图，再人工同意新技能。"
            return
        }
        skillConsents.insert(skillAnchor(draft))
        message = "仅记录用户人工同意新技能的示例意图；未安装、加载或使用技能。"
    }

    var skillBoundary: String {
        variant == .bundledSkill
            ? (skillManuallyConsented ? "新技能人工同意已记录 · 仍未安装/使用"
                : "附带新技能仍需人工同意；Full Access 不能代替")
            : "本分支不引入新技能；不重新实现技能 Q49/U01 治理。"
    }

    var rollbackProjection: TConnReviewProjection {
        var reasons: [String] = []
        if !updateHasFailed { reasons.append("没有更新失败示例依据") }
        if variant != .rollbackEligible {
            reasons.append("旧版许可、安全兼容性或回退操作授权缺项/未知")
        }
        return .init(eligible: reasons.isEmpty, title: reasons.isEmpty ? "可审阅回退意图 · 未回退" : "回退受阻",
                     reasons: reasons)
    }

    func beginDisableDraft(scope: TConnReviewScope = .task) {
        disableDraft = .init(itemID: selectedID, context: context, scope: scope, enabled: false, temporary: false)
    }

    func cancelDisableDraft() {
        disableDraft = nil
        message = "停用草稿已取消，没有应用。"
    }

    func confirmDisableIntent() {
        guard let draft = disableDraft, draft.itemID == selectedID, draft.context == context else { return }
        guard draft.scope != .project || context.project != nil else {
            message = "无项目任务不能停用一个不存在的项目。"
            return
        }
        disabledScopes.insert(scopeKey(draft.scope, context: context, itemID: selectedID))
        invalidateIntroductionIntents(scope: draft.scope)
        // Only this finite fixture context has an in-flight example.
        if selectedID == ToolsConnectionsReviewFixtures.connectionID
            && (context == .alphaOne || (draft.scope == .project && context.project == ToolsConnectionsReviewContext.alphaOne.project)) {
            let key = scopeKey(.task, context: .alphaOne, itemID: selectedID)
            // There is only one fixed in-flight execution in this fixture.
            // A repeated intent cannot create a second execution to stop.
            if stopStates[key] != .confirmed { stopStates[key] = .requested }
            resumeAfterStopIntent = false
        }
        proposedID = nil
        disableDraft = nil
        reviewDraft = nil
        message = "已停用指定范围：阻止新增、排队和重试；在途只记请求停止，未确认停止/回滚。"
    }

    func reenableScope(_ scope: TConnReviewScope) {
        disabledScopes.remove(scopeKey(scope, context: context, itemID: selectedID))
        invalidateIntroductionIntents(scope: scope)
        message = "仅重新启用指定范围；不撤销 Stop/暂停/取消，不恢复权限，不重放历史。"
    }

    var stopState: TConnReviewStopState {
        stopStates[scopeKey(.task, context: context, itemID: selectedID)] ?? .notRequested
    }

    func recordExplicitStop(_ disposition: TConnReviewDisposition = .stopped) {
        guard isContinuationAnchor else { return }
        continuation.disposition = disposition
        if stopState != .confirmed {
            stopStates[scopeKey(.task, context: context, itemID: selectedID)] = .requested
        }
        resumeAfterStopIntent = false
        message = "已记录\(disposition.rawValue)；资源恢复不能自动复活。"
    }

    func recordExplicitResume() {
        guard isContinuationAnchor else { return }
        guard continuation.disposition != .expired else {
            message = "等待到限，沿 Q67 另行明确安排；不在本样例重造策略。"
            return
        }
        guard stopState != .requested else {
            message = "停止结果仍待核实；明确恢复意图也不能启动重叠替代。"
            return
        }
        continuation.disposition = .waiting
        resumeAfterStopIntent = true
        message = "已记录明确恢复意图；仍须重核所有条件，不代表实际执行。"
    }

    var continuationProjection: TConnReviewProjection {
        var reasons: [String] = []
        if context != .alphaOne { reasons.append("本上下文没有该示例交办，不创建新任务") }
        if selectedID != ToolsConnectionsReviewFixtures.connectionID {
            reasons.append("该续行依据只关联固定示例连接步骤，不迁移到所浏览条目")
        }
        if continuation.disposition != .waiting { reasons.append(continuation.disposition.rawValue + "须明确恢复") }
        if !continuation.assignmentValid || !continuation.allowsWaiting { reasons.append("交办无效或不允许等待") }
        if !continuation.targetMatches { reasons.append("目标已变化，不能静默替代") }
        if !continuation.permissionCovered || !authorizedTasks.contains(context) { reasons.append("有效授权不足") }
        if !continuation.resourceReady { reasons.append("资源条件尚未恢复") }
        if !continuation.dependenciesReady { reasons.append("依赖尚未恢复") }
        if !continuation.originalResultKnown { reasons.append("原写入结果未知，须先核实，不盲重放") }
        if isDisabled(selectedID, in: context) { reasons.append("用户停用范围仍有效") }
        if stopState != .notRequested && !resumeAfterStopIntent {
            reasons.append(stopState == .requested ? "停止结果待核实，不启动重叠替代" : "已确认停止，仍需明确恢复意图")
        }
        return .init(eligible: reasons.isEmpty, title: reasons.isEmpty ? "可续行投影 · 未实际执行" : "相关工作仍受阻",
                     reasons: reasons)
    }

    func recordCredentialIntent(_ intent: TConnReviewCredentialIntent) {
        guard scenario == .TC5 else { return }
        guard selectedID == ToolsConnectionsReviewFixtures.connectionID else {
            credentialMessages[context, default: [:]][selectedID] = "此凭据引用仅属 demo-connection，不向所浏览条目迁移使用许可。"
            return
        }
        if credentialState != .referenced {
            credentialMessages[context, default: [:]][selectedID] = "\(credentialState.rawValue)；不扫描个人配置、换账号或自动补密钥。"
        } else if intent == .use && authorizedTasks.contains(context) && isEnabled(selectedID, in: context) {
            credentialMessages[context, default: [:]][selectedID] = "已覆盖最少使用示意：仅 demo-execution-location 的具体进程/目标；未供给秘密。"
        } else {
            credentialMessages[context, default: [:]][selectedID] = "\(intent.title)：缺独立适用授权，仅记录未执行意图；使用许可不自动推导。"
        }
    }

    var sharePreview: String {
        "虚构连接：演示开发库连接；目标：\(identity.target)；"
            + "位置：\(ToolsConnectionsReviewFixtures.credentialLocation)；范围：本任务；[秘密省略]。"
            + "这只是呈现示意，不实现脱敏/安全存储/跨 Host 隔离，不承诺外部零回显或下游清除。"
    }

    func toggleResultExpansion(_ resultID: String) {
        guard results.contains(where: { $0.id == resultID }) else { return }
        if expandedResultIDs.contains(resultID) { expandedResultIDs.remove(resultID) }
        else { expandedResultIDs.insert(resultID) }
        message = "仅展开固定内存文本；不代表执行者读取、采用、技术核验或用户验收。"
    }

    func requestResultCompletionIntent(_ resultID: String) {
        guard results.contains(where: { $0.id == resultID }) else { return }
        message = "补全结果仅为未执行意图：先核实已有结果/原执行；远端重读是新操作，不能重放写库、上传或发布。"
    }

    func recordResultShareIntent(_ resultID: String) {
        guard results.contains(where: { $0.id == resultID }) else { return }
        message = "仅记录未执行分享意图；另一 API/模型/协作者须有适用授权，不整份外发或自动收录成果/知识/技能。"
    }

    var availableEvents: [TConnReviewEvent] {
        switch scenario {
        case .TC1, .TC2: [.taskAuthorized]
        case .TC3: [.installationSucceeded, .updateSucceeded, .updateFailed]
        case .TC4: [.stopped, .conditionsRestored, .originalExecutionVerified]
        case .TC5: []
        case .TC6: [.resultRead, .resultAdopted, .resultVerified, .resultAccepted]
        }
    }

    func loadExampleEvent(_ event: TConnReviewEvent, resultID: String? = nil) {
        guard availableEvents.contains(event) else { return }
        if scenario == .TC4 && !isContinuationAnchor {
            message = "此事件仅属项目甲任务一的固定连接步骤，不对其它项目/任务/条目复用。"
            return
        }
        if scenario == .TC3 && (context != .alphaOne || selectedID != ToolsConnectionsReviewFixtures.externalID) {
            message = "此结果预设仅属项目甲任务一的外部候选，不迁移示例事实。"
            return
        }
        if scenario == .TC6 && (context != .alphaOne || selectedID != ToolsConnectionsReviewFixtures.queryID) {
            message = "此结果事件仅属项目甲任务一的固定查询结果，不随目录浏览改变归属。"
            return
        }
        let targetResult = resultID ?? results.first?.id ?? "demo-no-result"
        let target = scenario == .TC6 ? targetResult : context.id
        let key = scenario == .TC6 || event == .taskAuthorized
            ? "\(event.rawValue):\(target)" : "\(event.rawValue):\(target):\(selectedID)"
        guard !loadedEvents.contains(key) else {
            message = "该示例事件已载入，不重复生成调用/写入。"
            return
        }
        switch event {
        case .taskAuthorized:
            authorizedTasks.insert(context)
        case .installationSucceeded:
            guard let approval = approvedIntents.last(where: {
                    $0.draft.operation == .install && $0.draft.context == context && $0.draft.itemID == selectedID
                })
            else { message = "尚无安装示例意图；不能从结果按钮补审批。"; return }
            guard !approval.draft.introducesSkill || skillConsents.contains(skillAnchor(approval.draft)) else {
                message = "附带新技能尚无本条目/版本/任务的人工同意，不能载入安装成功。"
                return
            }
            installedFact = .event("示例安装结果已载入 · 可用性仍分层检查", event)
        case .updateSucceeded:
            guard let approval = approvedIntents.last(where: {
                    $0.draft.operation == .update && $0.draft.context == context && $0.draft.itemID == selectedID
                })
            else { message = "尚无更新示例意图；一次安装不授权更新。"; return }
            guard !approval.draft.introducesSkill || skillConsents.contains(skillAnchor(approval.draft)) else {
                message = "新版本附带技能仍缺独立人工同意，旧版同意不覆盖新版。"
                return
            }
            candidateVersionFact = .event("v2 候选结果已载入；在途仍固定 v1", event)
        case .updateFailed:
            updateHasFailed = true
            candidateVersionFact = .blocked("demo-event-update-failure · 示例新版失败，残留/未知保留")
        case .stopped:
            guard stopState == .requested else { message = "没有此范围的停止请求，不编造已停。"; return }
            stopStates[scopeKey(.task, context: context, itemID: selectedID)] = .confirmed
        case .conditionsRestored:
            continuation.resourceReady = true
            if variant != .blocked { continuation.dependenciesReady = true }
        case .originalExecutionVerified:
            continuation.originalResultKnown = true
        case .resultRead, .resultAdopted, .resultVerified, .resultAccepted:
            guard let index = results.firstIndex(where: { $0.id == targetResult }) else { return }
            let fact = TConnReviewFact.event("指定层的固定示例事实；关联不证明业务正确", event)
            switch event {
            case .resultRead: results[index].evidence.read = fact
            case .resultAdopted: results[index].evidence.adopted = fact
            case .resultVerified: results[index].evidence.verified = fact
            case .resultAccepted: results[index].evidence.accepted = fact
            default: break
            }
        }
        loadedEvents.insert(key)
        eventLog.append("\(event.title) · \(event.rawValue) · 示例事件，未实际执行 · \(target)")
        message = "已载入明确示例事件，未进行真实操作。"
    }

    func loadScenario(_ next: ToolsConnectionsReviewScenario, variant nextVariant: TConnReviewVariant = .standard) {
        scenario = next
        variant = next.variants.contains(nextVariant) ? nextVariant : .standard
        context = .alphaOne
        category = .capability
        selectedID = next == .TC3 ? ToolsConnectionsReviewFixtures.externalID :
            (next == .TC4 || next == .TC5 ? ToolsConnectionsReviewFixtures.connectionID : ToolsConnectionsReviewFixtures.queryID)
        category = selectedItem.category
        let all = Set(catalog.map(\.id))
        configuredIDs = next == .TC1
            ? [] : all.subtracting([ToolsConnectionsReviewFixtures.externalID])
        projectEnabled = next == .TC1 ? [:] : ["demo-project-alpha": all]
        if variant == .notEnabled { projectEnabled = [:] }
        taskEnabled = [:]
        taskReferences = [:]
        temporaryReferences = [:]
        authorizedTasks = next == .TC1 || (next == .TC2 && variant == .blocked) ? [] : [.alphaOne]
        discardDrafts()
        approvedIntents = []
        activeIntroductions = [:]
        skillConsents = []
        constraints = [:]
        constrainedItems = [:]
        proposedID = nil
        disabledScopes = []
        stopStates = [:]
        resumeAfterStopIntent = false
        continuation = .init()
        if variant == .blocked {
            continuation.dependenciesReady = false
            continuation.targetMatches = false
            continuation.permissionCovered = false
        }
        if variant == .paused { continuation.disposition = .paused }
        if variant == .stopped { continuation.disposition = .stopped }
        if variant == .cancelled { continuation.disposition = .cancelled }
        if variant == .expired { continuation.disposition = .expired }
        if variant == .unknownExecution { continuation.originalResultKnown = false }
        switch variant {
        case .credentialMissing: credentialState = .missing
        case .credentialRevoked: credentialState = .revoked
        case .credentialLocationMismatch: credentialState = .locationMismatch
        default: credentialState = .referenced
        }
        credentialMessages = [:]
        results = ToolsConnectionsReviewFixtures.results(referenceMissing: variant == .referenceMissing)
        expandedResultIDs = []
        loadedEvents = []
        eventLog = []
        installedFact = .blocked("外部候选未安装，目录不证明可用")
        if next == .TC3 && [.updateCovered, .remoteUnpinned, .rollbackEligible, .rollbackUnknown].contains(variant) {
            configuredIDs.insert(ToolsConnectionsReviewFixtures.externalID)
            installedFact = .ready("固定示例：v1 已接入；v2 仅候选，不暗换在途版本")
        }
        candidateVersionFact = .unknown("v2 结果尚无依据；不更换在途版本")
        updateHasFailed = variant == .rollbackEligible || variant == .rollbackUnknown
        if updateHasFailed { candidateVersionFact = .blocked("固定虚构预设：新版失败，业务历史不变") }
        message = "\(next.rawValue) · \(variant.title) · 固定虚构预设，可重置，不执行。"
    }

    func reset() { loadScenario(scenario, variant: variant) }

    private func approve(_ draft: TConnReviewIntroductionDraft, preauthorized: Bool) {
        let value = TConnReviewApprovedIntent(draft: draft, preauthorized: preauthorized)
        if !approvedIntents.contains(value) { approvedIntents.append(value) }
        activeIntroductions[draft.context, default: [:]][draft.itemID] = draft
    }

    private var isContinuationAnchor: Bool {
        scenario == .TC4 && context == .alphaOne && selectedID == ToolsConnectionsReviewFixtures.connectionID
    }

    private func serviceFact(for item: ToolsConnectionsReviewCatalogItem) -> TConnReviewFact {
        if scenario == .TC1 && variant == .blocked
            && [ToolsConnectionsReviewFixtures.queryID, ToolsConnectionsReviewFixtures.connectionID].contains(item.id) {
            return .blocked("固定示例连接条件仍缺；保存/启用/引用与授权都不能代替连接依据")
        }
        if scenario == .TC3 && variant == .remoteUnpinned && item.id == ToolsConnectionsReviewFixtures.externalID {
            return .unknown("远端版本不可固定，变化后的契约/目标/授权未核实；阻塞相关新增调用")
        }
        return item.service
    }

    private var currentIntroductionDraft: TConnReviewIntroductionDraft? {
        if let draft = reviewDraft, draft.context == context, draft.itemID == selectedID { return draft }
        return activeIntroductions[context]?[selectedID]
    }

    private func skillAnchor(_ draft: TConnReviewIntroductionDraft) -> TConnReviewSkillConsent {
        .init(context: draft.context, itemID: draft.itemID, version: draft.version)
    }

    private func invalidateIntroductionIntents(scope: TConnReviewScope) {
        let affected = Set(ToolsConnectionsReviewContext.allCases.filter {
            scope == .task ? $0 == context : (context.project != nil && $0.project == context.project)
        })
        approvedIntents.removeAll {
            $0.draft.itemID == selectedID && affected.contains($0.draft.context)
        }
        skillConsents = skillConsents.filter { $0.itemID != selectedID || !affected.contains($0.context) }
        for target in affected {
            activeIntroductions[target]?[selectedID] = nil
            credentialMessages[target]?[selectedID] = nil
        }
        reviewDraft = nil
    }

    private func discardDrafts() {
        scopeDraft = nil
        disableDraft = nil
        reviewDraft = nil
    }

    private func scopeKey(
        _ scope: TConnReviewScope, context: ToolsConnectionsReviewContext, itemID: String
    ) -> String {
        "\(scope.rawValue):\(scope == .task ? context.id : (context.project ?? "demo-no-project")):\(itemID)"
    }
}
#endif
