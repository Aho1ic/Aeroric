#if DEBUG
import Foundation
import Observation

/// Only synchronous specimen transitions. No business client, storage, clock,
/// background work or callbacks. An intent never manufactures an execution fact.
@MainActor @Observable
final class WorkArrangementsReviewStore {
    private let ownerID = UUID()
    private(set) var generation = 0
    private(set) var scenario: WAReviewScenario
    private(set) var todos: [WAReviewTodo]
    private(set) var executions: [WAReviewExecution]
    private(set) var rule: WAReviewRule
    private(set) var gates: [WAReviewGate: WAReviewTruth]
    private(set) var limits: [WAReviewLimit: WAReviewTruth]
    private(set) var sharedPoint: WAReviewTruth
    private(set) var events: [WAReviewEvent] = []
    private(set) var opportunities: [WAReviewOpportunity] = []
    private(set) var usage: [WAReviewUsage] = []
    private(set) var launchIntent: WAReviewLaunchIntent?
    private(set) var busyFollowupPending = false
    private(set) var recoveryEligible = false
    private(set) var message = "已载入初始虚构示例；没有启动任何工作。"
    private var loadedRecords: Set<String> = []
    private var serial = 0

    var section: WAReviewSection = .todos
    var selectedTodoID: String? = WorkArrangementsReviewFixtures.primaryTodoID
    var selectedRunID: String?
    // Presentation only. Never aliases a permission or execution setting.
    var classicAppearance = false
    var advancedPresentation = false
    var demonstrateReducedTransparency = false
    var demonstrateIncreasedContrast = false
    var demonstrateReducedMotion = false

    init(scenario: WAReviewScenario = .w1) {
        self.scenario = scenario
        todos = WorkArrangementsReviewFixtures.todos(for: scenario)
        executions = WorkArrangementsReviewFixtures.executions(for: scenario)
        rule = WorkArrangementsReviewFixtures.rule(for: scenario)
        gates = Dictionary(uniqueKeysWithValues: WAReviewGate.allCases.map { ($0, .satisfied) })
        limits = Dictionary(uniqueKeysWithValues: WAReviewLimit.allCases.map { ($0, .satisfied) })
        sharedPoint = scenario == .w7 ? .failed : .satisfied
        selectedRunID = executions.first?.id
    }

    func reset(to scenario: WAReviewScenario? = nil) {
        if let scenario { self.scenario = scenario }
        generation += 1
        todos = WorkArrangementsReviewFixtures.todos(for: self.scenario)
        executions = WorkArrangementsReviewFixtures.executions(for: self.scenario)
        rule = WorkArrangementsReviewFixtures.rule(for: self.scenario)
        gates = Dictionary(uniqueKeysWithValues: WAReviewGate.allCases.map { ($0, .satisfied) })
        limits = Dictionary(uniqueKeysWithValues: WAReviewLimit.allCases.map { ($0, .satisfied) })
        sharedPoint = self.scenario == .w7 ? .failed : .satisfied
        events = []; opportunities = []; usage = []; loadedRecords = []
        launchIntent = nil; busyFollowupPending = false; recoveryEligible = false; serial = 0
        section = .todos; selectedTodoID = todos.first?.id; selectedRunID = executions.first?.id
        classicAppearance = false; advancedPresentation = false
        demonstrateReducedTransparency = false
        demonstrateIncreasedContrast = false
        demonstrateReducedMotion = false
        message = "已重新载入初始示例；不是恢复真实进程。旧草稿已失效。"
    }

    func todo(_ id: String?) -> WAReviewTodo? { todos.first { $0.id == id } }
    func execution(_ id: String?) -> WAReviewExecution? { executions.first { $0.id == id } }
    var hasPrimaryOccupancy: Bool { executions.contains { $0.primary && $0.holdsOccupancy } }
    var latestLimitedRun: WAReviewExecution? {
        executions.last { $0.primary && $0.stopCause == .resource && !$0.continued }
    }

    func makeNewTodoDraft() -> WAReviewNewTodoDraft {
        WAReviewNewTodoDraft(ownerID: ownerID, generation: generation)
    }

    @discardableResult
    func saveNewTodo(_ draft: WAReviewNewTodoDraft) -> Bool {
        guard owns(draft.ownerID, draft.generation) else { return reject("新建草稿已失效。") }
        let title = draft.title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return reject("目标不能为空。") }
        let id = nextID("todo")
        todos.append(WAReviewTodo(
            id: id, title: title, source: draft.source, category: draft.category, recordOnly: draft.recordOnly
        ))
        selectedTodoID = id
        note("保存记录（示例）", "只增加待办；未交办、未启用、未运行。")
        return true
    }

    func makeRuleDraft() -> WAReviewRuleDraft {
        WAReviewRuleDraft(
            ownerID: ownerID, generation: generation, baseRevision: rule.revision,
            original: rule.policy, edited: rule.policy
        )
    }

    @discardableResult
    func applyRuleDraft(_ draft: WAReviewRuleDraft, confirmingPolicy: Bool) -> Bool {
        guard owns(draft.ownerID, draft.generation), draft.baseRevision == rule.revision,
              draft.original == rule.policy else { return reject("策略草稿已失效；重新审阅当前差异。") }
        let old = rule.policy
        rule.policy = draft.edited
        rule.revision += 1
        rule.policyConfirmed = confirmingPolicy
        // Saving/confirming does not enable, schedule, restart or rebind a run.
        launchIntent = nil
        invalidateWaitingReadiness()
        if old.trigger != draft.edited.trigger {
            busyFollowupPending = false
            recoveryEligible = false
        }
        if !draft.edited.afterBusyOnce { busyFollowupPending = false }
        if !draft.edited.recoveryOnce { recoveryEligible = false }
        if old.permission.isGranted && !draft.edited.permission.isGranted {
            busyFollowupPending = false
            recoveryEligible = false
            requestPrimaryClosure(cause: .permission, ruleAuthorityOnly: true)
        }
        let detail = draft.difference + "\n只用于后续启动重核；在途保留原依据。"
        note(confirmingPolicy ? "保存并确认策略（示例）" : "仅保存策略（示例）", detail)
        if old.permission.isGranted && !draft.edited.permission.isGranted {
            message += " 撤回权限优先；已请求受影响在途收束，事实仍待核实。"
        }
        return true
    }

    @discardableResult
    func enableRule() -> Bool {
        guard rule.policyConfirmed, rule.policy.permission.isGranted else {
            return reject("启用前须明确确认本规则策略及授权；Full Access 不是启用。")
        }
        guard !rule.stopped, !rule.paused else { return reject("先明确恢复用户暂停/Stop，再另行启用。") }
        rule.enabled = true
        note("确认并启用规则（示例）", "未回填任何已有待办，未载入到点或开始记录。")
        return true
    }

    func disableRule() {
        rule.enabled = false; launchIntent = nil
        invalidateWaitingReadiness()
        busyFollowupPending = false; recoveryEligible = false
        note("停用规则（示例）", "只挡该规则新准入；保留在途与历史。")
    }

    func previewReason(for todo: WAReviewTodo, policy: WAReviewRulePolicy? = nil) -> String? {
        let policy = policy ?? rule.policy
        if todo.excluded { return "单项已排除" }
        if !policy.scope.includes(todo) { return "不在当前清单/标签范围" }
        if todo.recordOnly { return "仅记一下；没有执行意图" }
        if todo.clarificationNeeded { return "目标待澄清" }
        if todo.intent == .cancelled || todo.intent == .resolvedElsewhere || todo.intent == .unknown {
            return todo.intent.rawValue
        }
        if todo.userAccepted || isTechnicallyComplete(todo.id) { return "已完成/待验收；不重开" }
        if executions.contains(where: { $0.todoID == todo.id && $0.holdsOccupancy }) {
            return "已有在途/结果或副作用未知"
        }
        return nil
    }

    func makeBatchDraft(kind: WAReviewBatchKind) -> WAReviewBatchDraft {
        let candidates = todos.map(\.id)
        return WAReviewBatchDraft(
            ownerID: ownerID, generation: generation, ruleRevision: rule.revision, kind: kind,
            candidates: candidates, selected: [], excluded: Set(todos.filter(\.excluded).map(\.id))
        )
    }

    @discardableResult
    func confirmBatch(_ draft: WAReviewBatchDraft) -> Bool {
        guard owns(draft.ownerID, draft.generation), draft.ruleRevision == rule.revision,
              Set(draft.candidates) == Set(todos.map(\.id)),
              draft.selected.isSubset(of: Set(draft.candidates)),
              draft.excluded.isSubset(of: Set(draft.candidates)) else { return reject("批量草稿已失效。") }
        if draft.kind == .backlog && (!rule.enabled || !rule.policyConfirmed || !rule.policy.permission.isGranted) {
            return reject("主动纳入存量仍需本规则已启用且策略/授权有效。")
        }
        let chosen = draft.selected.subtracting(draft.excluded)
        for id in chosen {
            guard var item = todo(id) else { return reject("所选记录已变化。") }
            item.excluded = false // Proposed exclusion, not the old saved checkbox.
            if let reason = assignmentReason(item, useScope: draft.kind == .backlog) {
                return reject("未应用批量草稿：\(item.title) — \(reason)。")
            }
        }
        for index in todos.indices {
            todos[index].excluded = draft.excluded.contains(todos[index].id)
            guard chosen.contains(todos[index].id) else { continue }
            todos[index].intent = .assigned
            todos[index].assignedByRule = draft.kind == .backlog
            todos[index].assignmentPermission = draft.kind == .assignment ? .limited : .unanswered
        }
        note("\(draft.kind.rawValue)（示例）", "仅确认 \(chosen.count) 项，应用排除；未制造已开始事实。")
        return true
    }

    @discardableResult
    func assignTodo(_ id: String) -> Bool {
        guard let index = todos.firstIndex(where: { $0.id == id }) else { return reject("待办不存在。") }
        if let reason = assignmentReason(todos[index], useScope: false) { return reject(reason) }
        todos[index].intent = .assigned
        todos[index].assignmentPermission = .limited
        todos[index].assignedByRule = false
        note("单项明确交办（示例）", "独立有限交办，不启用规则、不继承 Full Access、不伪造运行。")
        return true
    }

    func loadArrivalRecord() {
        guard loadOnce("arrival") else { return }
        let arrivals = [
            WAReviewTodo(id: "arrival-user", title: "新增：用户接口适配", source: .user),
            WAReviewTodo(id: "arrival-agent", title: "新增：来源任务的接口说明", source: .agent),
            WAReviewTodo(id: "arrival-note", title: "新增：仅记一下", source: .user, recordOnly: true),
            WAReviewTodo(id: "arrival-unclear", title: "新增：待澄清目标", source: .agent, clarificationNeeded: true),
            WAReviewTodo(id: "arrival-excluded", title: "新增：明确排除项", source: .agent, excluded: true),
        ]
        var adopted = 0
        for var item in arrivals {
            if rule.policy.trigger == .arrival && ruleBlockers().isEmpty && previewReason(for: item) == nil {
                item.intent = .assigned; item.assignedByRule = true; adopted += 1
            }
            todos.append(item)
        }
        note("载入新增示例事件", "新增 \(arrivals.count) 项，其中 \(adopted) 项在有效覆盖内接单；没有启动执行，旧项保持原样。")
    }

    func pauseRule() {
        rule.paused = true; launchIntent = nil; busyFollowupPending = false; recoveryEligible = false
        invalidateWaitingReadiness()
        for index in executions.indices { executions[index].autoContinueGrantedBeforeLimit = false }
        note("暂停接单（示例）", "只挡相关新启动；仍合规在途不被暂停按钮中断。")
    }

    func stopRule() {
        rule.stopped = true; launchIntent = nil; busyFollowupPending = false; recoveryEligible = false
        invalidateWaitingReadiness()
        for index in executions.indices { executions[index].autoContinueGrantedBeforeLimit = false }
        requestPrimaryClosure(cause: .user)
        note("请求停止本规则工作（示例）", "先挡新启动；仅已发请求，不等于已停止，不回滚历史或外部成果。")
    }

    @discardableResult
    func explicitlyResumeRule() -> Bool {
        // Clearing a user's control is never an effect of a passive record/top-up.
        rule.paused = false; rule.stopped = false
        launchIntent = nil; busyFollowupPending = false; recoveryEligible = false
        let reasons = ruleBlockers() + commonBlockers(includeOccupancy: true)
        note("明确恢复范围意图（示例）", reasons.isEmpty
             ? "当前准入可重核；不自动复活任何旧跳过、停止执行或未知操作。"
             : "仍受阻：\(reasons.joined(separator: "；"))。没有启动。")
        return reasons.isEmpty
    }

    func loadGateRecord(_ gate: WAReviewGate, value: WAReviewTruth) {
        gates[gate] = value
        if value != .satisfied { invalidateWaitingReadiness() }
        if value != .satisfied && [.noProhibition, .hostSafety, .hardLimitCapability].contains(gate) {
            requestPrimaryClosure(cause: .permission)
            launchIntent = nil
        }
        note("载入\(gate.rawValue)示例记录", value.rawValue)
    }

    func loadRevocationRecord() {
        rule.policy.permission = .revoked; rule.policyConfirmed = false; rule.revision += 1
        launchIntent = nil
        busyFollowupPending = false; recoveryEligible = false
        invalidateWaitingReadiness()
        // This scenario's explicit revocation also withdraws its related task grants.
        for index in todos.indices where todos[index].intent == .assigned {
            todos[index].assignmentPermission = .revoked
        }
        requestPrimaryClosure(cause: .permission)
        note("载入撤权示例记录", "旧依据只留作追溯；撤权优先，收束事实须另载入核实。")
    }

    @discardableResult
    func requestLaunch(todoID: String? = nil) -> Bool {
        let intent = WAReviewLaunchIntent(todoID: todoID, origin: .manual, continuingRunID: nil)
        return prepare(intent)
    }

    /// Explicitly loading this record is the ONLY path adding an in-flight run.
    @discardableResult
    func loadStartedRecord() -> Bool {
        guard let intent = launchIntent else { return reject("没有有效启动意图；交办/保存本身不是已开始。") }
        let reasons = blockers(for: intent)
        guard reasons.isEmpty else { launchIntent = nil; return reject(reasons.joined(separator: "；")) }
        let old = execution(intent.continuingRunID)
        let basis = old?.basis ?? WAReviewBasis(
            ruleRevision: rule.revision,
            goal: todo(intent.todoID)?.title ?? "当前状态检查（无需先创建待办）",
            configuration: rule.policy.configuration
        )
        if let oldIndex = executions.firstIndex(where: { $0.id == intent.continuingRunID }) {
            executions[oldIndex].continued = true
        }
        let id = nextID("run")
        executions.append(WAReviewExecution(
            id: id, todoID: intent.todoID, primary: true,
            usesRuleAuthority: !usesIndependentAuthority(intent),
            basis: basis,
            phase: .running, safety: .failed, sideEffects: .unknown,
            artifacts: "由明确载入已开始示例记录产生；\(intent.origin.rawValue)，不是外部执行"
        ))
        if let index = todos.firstIndex(where: { $0.id == intent.todoID }) { todos[index].wait = .none }
        launchIntent = nil; selectedRunID = id
        note("载入已开始示例记录", "\(intent.origin.rawValue) · \(basis.summary)")
        return true
    }

    func loadRunRecord(_ record: WAReviewRunRecord, runID: String) {
        guard let index = executions.firstIndex(where: { $0.id == runID }),
              executions[index].holdsOccupancy else { _ = reject("该历史执行已收束或不存在；不改写旧结果。"); return }
        if record == .ended && executions[index].hasFailureRecord {
            _ = reject("该次执行已有失败记录；结束不覆盖失败，重试须保留为另一次执行。")
            return
        }
        let key = "run-\(runID)-\(record.rawValue)"
        guard loadOnce(key) else { return }
        switch record {
        case .ended:
            executions[index].phase = .ended
            executions[index].safety = .unknown
            executions[index].sideEffects = .unknown
        case .failed:
            executions[index].phase = .failed
            executions[index].hasFailureRecord = true
            executions[index].safety = .unknown
        case .unknown, .processExited:
            executions[index].phase = .unknown
            executions[index].safety = .unknown
            executions[index].sideEffects = .unknown
        case .safeClosure:
            // Safe handoff is not a successful outcome or a goal proof.
            if executions[index].hasFailureRecord {
                executions[index].phase = .failed
            } else if executions[index].phase != .ended && executions[index].phase != .failed {
                executions[index].phase = .safelyStopped
            }
            executions[index].safety = .satisfied; executions[index].sideEffects = .satisfied
        }
        note("载入\(record.rawValue)示例记录", "保留原目标、依据及产物；不推断技术完成/用户验收，不恢复暂停/Stop。")
        if record == .safeClosure && executions[index].primary {
            let hadFollowup = busyFollowupPending
            busyFollowupPending = false
            if hadFollowup && rule.policy.afterBusyOnce {
                _ = prepare(WAReviewLaunchIntent(todoID: nil, origin: .afterBusy, continuingRunID: nil))
            }
            considerAutomaticLimitContinuation()
            reevaluateWaitingItems()
        }
    }

    func loadVerificationRecord(_ record: WAReviewVerificationRecord, todoID: String) {
        guard let index = todos.firstIndex(where: { $0.id == todoID }),
              let run = executions.last(where: { $0.todoID == todoID }), run.phase == .ended,
              run.stopCause == nil, !run.hasFailureRecord, !run.holdsOccupancy else {
            _ = reject("先载入该目标最近一次执行结束及安全核实；失败/停止/未知不算成功。"); return
        }
        var proof = WAReviewGoalProof(
            runID: run.id, integration: .satisfied, checks: .satisfied, supervision: .satisfied
        )
        switch record {
        case .complete: break
        case .missingIntegration: proof.integration = .unanswered
        case .missingChecks: proof.checks = .failed
        case .missingSupervision: proof.supervision = .unanswered
        case .unknown:
            proof.integration = .unknown; proof.checks = .unknown; proof.supervision = .unknown
        }
        todos[index].proof = proof
        note("载入\(record.rawValue)示例核验记录", "核验关联 \(run.id)；技术完成与用户验收仍分开。")
    }

    func isTechnicallyComplete(_ todoID: String) -> Bool {
        guard let item = todo(todoID),
              let last = executions.last(where: { $0.todoID == todoID }),
              last.id == item.proof.runID, last.phase == .ended,
              last.stopCause == nil, !last.hasFailureRecord, !last.holdsOccupancy else { return false }
        return item.proof.integration == .satisfied && item.proof.checks == .satisfied
            && item.proof.supervision == .satisfied && gates[.supervision] == .satisfied
    }

    @discardableResult
    func recordUserAcceptance(_ id: String) -> Bool {
        guard isTechnicallyComplete(id), let index = todos.firstIndex(where: { $0.id == id }) else {
            return reject("目标仍缺技术核验；不推断用户已验收。")
        }
        todos[index].userAccepted = true
        note("记录用户验收（示例）", "明确的独立动作；不是执行退出或默认签字。")
        return true
    }

    func loadMissedRecords() {
        guard rule.policy.trigger == .schedule else { _ = reject("这不是定时规则。"); return }
        guard !hasPrimaryOccupancy else { _ = reject("原执行/副作用未核实，不能当作确定未启动的漏跑。"); return }
        guard loadOnce("missed-segment") else { return }
        for number in 1...3 {
            opportunities.append(WAReviewOpportunity(id: "missed-\(number)", description: "已核实漏跑 \(number) · 未启动 · 默认跳过"))
        }
        // The missed period may itself be caused by unavailable dependencies
        // or resources. Capture prior policy/control, recheck resources NOW
        // only when the recovery record is loaded.
        recoveryEligible = rule.policy.recoveryOnce && ruleBlockers().isEmpty
        note("载入三次确定未启动示例记录", "历史三次均未运行；只允许事先开启且仍合规的一次当前检查，不追赶三次。")
    }

    func loadRecoveryRecord() {
        guard loadedRecords.contains("missed-segment"), loadOnce("recovery-segment") else {
            if !loadedRecords.contains("missed-segment") { _ = reject("先核实确实未启动的漏跑；不重放未知执行。") }
            return
        }
        let eligible = recoveryEligible
        recoveryEligible = false
        if eligible && rule.policy.recoveryOnce {
            _ = prepare(WAReviewLaunchIntent(todoID: nil, origin: .recovery, continuingRunID: nil))
        } else {
            note("载入恢复可用示例事件", "未事先获准或现行条件不满足；历史跳过保持未运行，没有生成积压队列。")
        }
    }

    func loadScheduledRecord() {
        guard rule.policy.trigger == .schedule else { _ = reject("范围新增与定时到点是不同触发类别。"); return }
        guard loadOnce("new-schedule") else { return }
        _ = prepare(WAReviewLaunchIntent(todoID: nil, origin: .scheduled, continuingRunID: nil))
    }

    func loadBusyRecords() {
        guard rule.policy.trigger == .schedule, hasPrimaryOccupancy else { _ = reject("没有已核实的同规则繁忙/未知占用；不伪造繁忙历史。"); return }
        guard loadOnce("busy-segment") else { return }
        for number in 1...3 {
            opportunities.append(WAReviewOpportunity(id: "busy-\(number)", description: "繁忙到点 \(number) · 跳过 · 不排队"))
        }
        busyFollowupPending = rule.policy.afterBusyOnce && ruleBlockers().isEmpty
        note("载入三次繁忙到点示例记录", busyFollowupPending
             ? "历史均跳过；仅保留一个面向当前状态的后续意图，等待原执行及占用核实收束。"
             : "默认繁忙跳过；没有后续队列，不继承恢复补一次设置。")
    }

    func loadSharedPointRecord(_ value: WAReviewTruth) {
        sharedPoint = value
        note("载入共享修改点示例记录", value == .satisfied
             ? "原执行、副作用及修改点均核实安全释放；覆盖内等待项按当前依据重核。"
             : "\(value.rawValue)；超时、退出或 Stop 请求不等于安全释放。")
        if value == .satisfied { reevaluateWaitingItems() } else { invalidateWaitingReadiness() }
    }

    func loadInputChangedRecord() {
        guard loadOnce("input-change") else { return }
        for index in todos.indices where todos[index].allowWaiting {
            todos[index].inputAligned = false; todos[index].wait = .alignment
        }
        note("载入接口输入变化示例记录", "仅相关适配/验证步骤待对齐；不重做全项目，不影响独立文档。")
    }

    func loadLocalAlignmentRecord(_ todoID: String) {
        guard let index = todos.firstIndex(where: { $0.id == todoID }), todos[index].allowWaiting else { return }
        todos[index].inputAligned = true
        note("载入局部对齐示例记录", "只对齐该等待项的受影响步骤；原目标、API 别名与其他成果保留。")
        reevaluateWaitingItems()
    }

    func loadIntentRecord(_ intent: WAReviewIntent, todoID: String) {
        guard let index = todos.firstIndex(where: { $0.id == todoID }) else { return }
        todos[index].intent = intent
        if intent == .cancelled || intent == .resolvedElsewhere { todos[index].wait = .none }
        if intent == .unknown && todos[index].wait == .ready { todos[index].wait = .recheck }
        if intent != .assigned && launchIntent?.todoID == todoID { launchIntent = nil }
        note("载入当前执行意图示例记录", "\(intent.rawValue)；释放资源不产生新的交办/重试权。")
    }

    func loadAttemptPermissionRecord(_ value: WAReviewTruth, todoID: String) {
        guard let index = todos.firstIndex(where: { $0.id == todoID }) else { return }
        todos[index].attemptPermission = value
        note("载入该项尝试边界示例记录", value.rawValue)
    }

    @discardableResult
    func requestWaitingLaunch(_ todoID: String) -> Bool {
        prepare(WAReviewLaunchIntent(todoID: todoID, origin: .waiting, continuingRunID: nil))
    }

    func loadUsageRecords() {
        var existing = Set(usage.map(\.id))
        for record in WorkArrangementsReviewFixtures.usage where existing.insert(record.id).inserted {
            usage.append(record)
        }
        note("载入用量示例记录", "按固定记录 ID 去重；各作用域是关联视图，不把局部/项目/共同组再次相加。")
    }

    func usageSummary(scope: WAReviewBudgetScope) -> WAReviewUsageSummary {
        let records = usage.filter { $0.scopes.contains(scope) }
        let reported = records.compactMap { record -> Int? in
            if case let .reported(value) = record.kind { return value }; return nil
        }
        let estimated = records.compactMap { record -> Int? in
            if case let .estimate(value) = record.kind { return value }; return nil
        }
        let unknown = records.filter { $0.kind == .unknown }.count
        return WAReviewUsageSummary(
            reported: reported.isEmpty ? nil : reported.reduce(0, +),
            estimated: estimated.isEmpty ? nil : estimated.reduce(0, +), unknownCount: unknown
        )
    }

    func loadLimitRecord(_ limit: WAReviewLimit, value: WAReviewTruth) {
        limits[limit] = value
        if value != .satisfied { invalidateWaitingReadiness() }
        if limit.affectsInFlight && value != .satisfied { requestPrimaryClosure(cause: .resource) }
        if value != .satisfied { launchIntent = nil }
        note("载入\(limit.rawValue)边界示例记录", limit.affectsInFlight && value != .satisfied
             ? "\(value.rawValue)；阻止扩张并请求受影响在途收束，不借收尾新增付费工作，监督仍待核对。"
             : "\(value.rawValue)；准入上限只挡新启动/下一次重试，不终止最后一次仍合规尝试。")
    }

    func topUp(_ scope: WAReviewBudgetScope, continuingRunID: String? = nil) {
        let limit: WAReviewLimit
        switch scope {
        case .local: limit = .localBudget
        case .project: limit = .projectBudget
        case .shared: limit = .sharedBudget
        }
        limits[limit] = .satisfied
        note(continuingRunID == nil ? "补额（示例）" : "补额并继续（示例）",
             "只恢复\(scope.rawValue)消耗条件；不清时长/尝试/Stop/暂停，不伪造监督通过。")
        if let id = continuingRunID {
            _ = prepare(WAReviewLaunchIntent(todoID: execution(id)?.todoID, origin: .resourceContinuation, continuingRunID: id))
        } else {
            considerAutomaticLimitContinuation()
        }
        reevaluateWaitingItems()
    }

    func blockers(for intent: WAReviewLaunchIntent) -> [String] {
        var reasons: [String] = []
        let item = todo(intent.todoID)
        let independentGrant = usesIndependentAuthority(intent)
        if independentGrant {
            if rule.paused { reasons.append("用户已暂停相关新启动") }
            if rule.stopped { reasons.append("用户 Stop 尚未明确恢复") }
            if item?.assignmentPermission.isGranted != true { reasons.append("本项独立交办授权缺失/未知或已撤回") }
        } else {
            reasons += ruleBlockers()
        }
        if [.scheduled, .recovery, .afterBusy].contains(intent.origin), rule.policy.trigger != .schedule {
            reasons.append("当前规则已不是定时触发；不兑现旧定时补一次")
        }
        reasons += commonBlockers(
            includeOccupancy: true, continuingLimitedRun: intent.origin == .resourceContinuation
        )
        if let id = intent.todoID {
            guard let item else { return reasons + ["待办已不存在：\(id)"] }
            if let reason = assignmentReason(item, useScope: !independentGrant) { reasons.append(reason) }
            if item.intent != .assigned { reasons.append("没有仍有效的明确交办意图") }
            if !item.assignedByRule && !item.assignmentPermission.isGranted {
                reasons.append("本项独立授权未获准/未知或已撤回；不借规则授权继承")
            }
            if item.attemptPermission != .satisfied { reasons.append("本项没有获准的新尝试机会：\(item.attemptPermission.rawValue)") }
            if !item.inputAligned { reasons.append("输入变化；仅相关步骤待对齐") }
            if item.allowWaiting && sharedPoint != .satisfied { reasons.append("共享修改点尚未核实安全释放") }
            if intent.origin == .waiting && (!item.allowWaiting || item.wait == .none) {
                reasons.append("不是已有获准、允许保留的等待工作")
            }
            if intent.origin != .resourceContinuation,
               let old = executions.last(where: { $0.todoID == id }),
               old.stopCause == .resource, !old.continued {
                reasons.append("到限旧工作须独立续行意图；不能借普通启动/等待重开")
            }
        }
        if intent.origin == .resourceContinuation {
            guard let run = execution(intent.continuingRunID), run.stopCause == .resource, !run.continued else {
                return reasons + ["不是可明确续行的到限旧执行"]
            }
            if run.phase != .safelyStopped || run.holdsOccupancy {
                reasons.append("原执行、副作用与相关占用尚未核实安全收束")
            }
            if run.usesRuleAuthority && run.basis.configuration != rule.policy.configuration {
                reasons.append("当前规则配置/API 依据已变化；旧执行续行须先明确对齐，不静默换绑或借旧配置绕过新策略")
            }
        }
        return reasons
    }

    var currentBlockers: [String] {
        if let launchIntent { return blockers(for: launchIntent) }
        return ruleBlockers() + commonBlockers(includeOccupancy: true)
    }

    private func ruleBlockers() -> [String] {
        var reasons: [String] = []
        if !rule.enabled { reasons.append("规则尚未启用") }
        if !rule.policyConfirmed { reasons.append("当前规则策略尚未明确确认") }
        if !rule.policy.permission.isGranted { reasons.append("当前规则授权缺失/未知或已撤回") }
        if rule.paused { reasons.append("用户已暂停新启动") }
        if rule.stopped { reasons.append("用户 Stop 尚未明确恢复") }
        return reasons
    }

    private func usesIndependentAuthority(_ intent: WAReviewLaunchIntent) -> Bool {
        if intent.origin == .resourceContinuation {
            return execution(intent.continuingRunID)?.usesRuleAuthority == false
        }
        return [.manual, .waiting].contains(intent.origin) && todo(intent.todoID)?.assignedByRule == false
    }

    private func commonBlockers(includeOccupancy: Bool, continuingLimitedRun: Bool = false) -> [String] {
        var reasons = WAReviewGate.allCases.compactMap { gate -> String? in
            let value = gates[gate] ?? .unknown
            return value == .satisfied ? nil : "\(gate.rawValue)：\(value.rawValue)"
        }
        reasons += WAReviewLimit.allCases.compactMap { limit -> String? in
            // The old execution's independent duration is NOT a permanent
            // rule-wide ban on a genuinely new current-state trigger.
            if limit == .duration && !continuingLimitedRun { return nil }
            let value = limits[limit] ?? .unknown
            return value == .satisfied ? nil : "\(limit.rawValue)：\(value.rawValue)"
        }
        if includeOccupancy && hasPrimaryOccupancy { reasons.append("同规则原执行/相关占用未安全收束；手动和补一次也不得重叠") }
        if sharedPoint != .satisfied { reasons.append("相关共享修改点尚未核实安全释放") }
        return reasons
    }

    private func assignmentReason(_ item: WAReviewTodo, useScope: Bool) -> String? {
        if item.excluded { return "单项已排除" }
        if item.recordOnly { return "仅记一下，不可偷接" }
        if item.clarificationNeeded { return "目标待澄清" }
        if useScope && !rule.policy.scope.includes(item) { return "不在当前规则范围" }
        if [.cancelled, .resolvedElsewhere, .unknown].contains(item.intent) { return item.intent.rawValue }
        if item.userAccepted || isTechnicallyComplete(item.id) { return "不重开已完成/待验收目标" }
        if executions.contains(where: { $0.todoID == item.id && $0.holdsOccupancy }) {
            return "该目标已有在途/未知执行，须先核实"
        }
        return nil
    }

    @discardableResult
    private func prepare(_ intent: WAReviewLaunchIntent) -> Bool {
        let reasons = blockers(for: intent)
        guard reasons.isEmpty else { launchIntent = nil; return reject(reasons.joined(separator: "；")) }
        launchIntent = intent
        note("记录下一步意图（示例）", "\(intent.origin.rawValue)；须另载入已开始记录才显示在途。")
        return true
    }

    private func reevaluateWaitingItems() {
        for index in todos.indices where todos[index].allowWaiting {
            // Do not transform a skipped opportunity, cancelled goal or old limit
            // closure into an authorized wait. These are pre-existing assignments.
            let item = todos[index]
            guard item.wait != .none, item.intent == .assigned else { continue }
            let intent = WAReviewLaunchIntent(todoID: item.id, origin: .waiting, continuingRunID: nil)
            if blockers(for: intent).isEmpty {
                todos[index].wait = .ready
                // Only fixed, already assigned fixture waiters are considered.
                // Readiness/intent is not an in-flight execution or a queue.
                if launchIntent == nil { _ = prepare(intent) }
            }
        }
    }

    private func invalidateWaitingReadiness() {
        for index in todos.indices where todos[index].wait == .ready { todos[index].wait = .recheck }
    }

    private func considerAutomaticLimitContinuation() {
        guard let old = latestLimitedRun, old.autoContinueGrantedBeforeLimit,
              rule.policy.autoContinueAfterLimit, launchIntent == nil else { return }
        let intent = WAReviewLaunchIntent(todoID: old.todoID, origin: .resourceContinuation, continuingRunID: old.id)
        if blockers(for: intent).isEmpty { _ = prepare(intent) }
    }

    private func requestPrimaryClosure(cause: WAReviewStopCause, ruleAuthorityOnly: Bool = false) {
        for index in executions.indices where executions[index].primary && executions[index].holdsOccupancy
            && (!ruleAuthorityOnly || executions[index].usesRuleAuthority) {
            if cause == .resource && executions[index].stopCause == nil {
                executions[index].autoContinueGrantedBeforeLimit = rule.policy.autoContinueAfterLimit
                    && !rule.paused && !rule.stopped
            }
            // A later budget event must not turn Stop/revocation into a
            // resource-only closure eligible for automatic continuation.
            if cause == .user {
                executions[index].stopCause = .user
                executions[index].autoContinueGrantedBeforeLimit = false
            } else if cause == .permission && executions[index].stopCause != .user {
                executions[index].stopCause = .permission
                executions[index].autoContinueGrantedBeforeLimit = false
            } else if executions[index].stopCause == nil {
                executions[index].stopCause = cause
            }
            if executions[index].phase == .running || executions[index].phase == .unknown {
                executions[index].phase = .stopRequested
            }
            executions[index].safety = .unknown
        }
    }

    private func owns(_ owner: UUID, _ generation: Int) -> Bool {
        owner == ownerID && generation == self.generation
    }
    private func nextID(_ prefix: String) -> String { serial += 1; return "\(prefix)-sample-\(serial)" }
    private func loadOnce(_ id: String) -> Bool {
        guard loadedRecords.insert(id).inserted else { _ = reject("该固定示例记录已载入；未重复处理。"); return false }
        return true
    }
    private func note(_ title: String, _ detail: String) {
        events.append(WAReviewEvent(id: nextID("event"), title: title, detail: detail))
        message = "\(title)：\(detail)"
    }
    @discardableResult
    private func reject(_ reason: String) -> Bool {
        message = "未应用/未启动（示例）：\(reason)"
        return false
    }
}
#endif
