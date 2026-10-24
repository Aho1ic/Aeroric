#if DEBUG
import Foundation

// Window-local specimens, NOT production DTOs, permissions or scheduler states.
enum WAReviewScenario: String, CaseIterable, Identifiable {
    case w1 = "W1", w2 = "W2", w3 = "W3", w4 = "W4"
    case w5 = "W5", w6 = "W6", w7 = "W7", w8 = "W8"
    var id: String { rawValue }
    var title: String {
        switch self {
        case .w1: "目标、来源与完成依据"
        case .w2: "明确交办、范围与存量"
        case .w3: "暂停与请求停止"
        case .w4: "规则差异与在途依据"
        case .w5: "漏跑与恢复补一次"
        case .w6: "繁忙与结束后补一次"
        case .w7: "局部冲突与等待重核"
        case .w8: "共享护栏与补额续行"
        }
    }
}

enum WAReviewSection: String, CaseIterable {
    case todos = "待办", executions = "关联执行", rule = "自动化规则"
}

enum WAReviewTruth: String, CaseIterable {
    case satisfied = "已核实满足（示例）"
    case failed = "不满足（示例）"
    case unknown = "未知（示例）"
    case unanswered = "尚未确认（示例）"
}

enum WAReviewSource: String, CaseIterable {
    case user = "用户（示例）"
    case agent = "获准 Agent · 来源任务 A（示例）"
}

enum WAReviewCategory: String, CaseIterable {
    case implementation = "实现清单 · 标签：可交办"
    case documentation = "文档清单 · 标签：可交办"
}

enum WAReviewIntent: String, CaseIterable {
    case recorded = "仅记录", assigned = "明确交办", cancelled = "已取消"
    case resolvedElsewhere = "其他成果已解决 · 待核验", unknown = "意图待核实"
}

enum WAReviewWait: String {
    case none = "无等待"
    case sharedPoint = "等待共享修改点：示例依赖配置"
    case budget = "已交办 · 等待适用额度"
    case alignment = "输入已变化 · 仅相关适配步骤待对齐"
    case recheck = "现行条件已变化 · 等待重核"
    case ready = "覆盖内已重核 · 待载入已开始记录"
}

struct WAReviewGoalProof: Equatable {
    var runID: String?
    var integration: WAReviewTruth = .unanswered
    var checks: WAReviewTruth = .unanswered
    var supervision: WAReviewTruth = .unanswered
}

struct WAReviewTodo: Identifiable, Equatable {
    let id: String
    var title: String
    var source: WAReviewSource
    var category: WAReviewCategory = .implementation
    var intent: WAReviewIntent = .recorded
    var assignmentPermission: WAReviewPermission = .unanswered
    var assignedByRule = false
    var recordOnly = false
    var clarificationNeeded = false
    var excluded = false
    var allowWaiting = false
    var wait: WAReviewWait = .none
    var inputAligned = true
    var attemptPermission: WAReviewTruth = .satisfied
    var proof = WAReviewGoalProof()
    var userAccepted = false
}

enum WAReviewPermission: String, CaseIterable {
    case unanswered = "未授予"
    case limited = "有限授权（示例）"
    case fullAccess = "本规则 Full Access（示例）"
    case unknown = "授权未知"
    case revoked = "已撤权（示例）"
    var isGranted: Bool { self == .limited || self == .fullAccess }
}

enum WAReviewScope: String, CaseIterable {
    case implementation = "星图项目 · 实现清单 · 可交办"
    case wholeProject = "星图项目 · 实现及文档清单 · 可交办"
    func includes(_ todo: WAReviewTodo) -> Bool {
        self == .wholeProject || todo.category == .implementation
    }
}

enum WAReviewConfiguration: String, CaseIterable {
    case a = "示例配置 A", b = "示例配置 B"
    var apiAlias: String { self == .a ? "示例 API A" : "示例 API B" }
}

enum WAReviewTrigger: String, CaseIterable {
    case arrival = "范围内新增", schedule = "定时到点（仅载入事件）"
}

struct WAReviewRulePolicy: Equatable {
    var scope: WAReviewScope = .implementation
    var configuration: WAReviewConfiguration = .a
    var trigger: WAReviewTrigger = .arrival
    var permission: WAReviewPermission = .unanswered
    var recoveryOnce = false
    var afterBusyOnce = false
    var autoContinueAfterLimit = false
}

struct WAReviewRule: Equatable {
    var revision = 1
    var policy = WAReviewRulePolicy()
    var policyConfirmed = false
    var enabled = false
    var paused = false
    var stopped = false
}

struct WAReviewBasis: Equatable {
    let ruleRevision: Int
    let goal: String
    let configuration: WAReviewConfiguration
    var summary: String {
        "规则 v\(ruleRevision) · \(configuration.rawValue) / \(configuration.apiAlias) · 原目标：\(goal)"
    }
}

enum WAReviewRunPhase: String {
    case running = "在途（示例记录）"
    case ended = "执行结束 · 不等于目标完成"
    case failed = "执行失败（示例记录）"
    case stopRequested = "已请求停止 · 尚未核实"
    case unknown = "执行/停止事实未知"
    case safelyStopped = "已核实安全收束（示例记录）"
}

enum WAReviewStopCause: String {
    case user = "用户 Stop"
    case permission = "撤权/安全禁令"
    case resource = "在途资源到限"
}

struct WAReviewExecution: Identifiable, Equatable {
    let id: String
    let todoID: String?
    let primary: Bool
    var usesRuleAuthority = true
    let basis: WAReviewBasis
    var phase: WAReviewRunPhase
    var safety: WAReviewTruth
    var sideEffects: WAReviewTruth
    var hasFailureRecord = false
    var stopCause: WAReviewStopCause?
    var autoContinueGrantedBeforeLimit = false
    var continued = false
    var artifacts: String

    var holdsOccupancy: Bool {
        phase == .running || phase == .stopRequested || phase == .unknown
            || safety != .satisfied || sideEffects != .satisfied
    }
}

enum WAReviewLaunchOrigin: String {
    case manual = "手动明确启动意图"
    case scheduled = "新的定时触发"
    case recovery = "恢复后一次当前检查"
    case afterBusy = "繁忙结束后一次当前检查"
    case waiting = "原获准等待覆盖内继续"
    case resourceContinuation = "到限旧执行明确续行"
}

struct WAReviewLaunchIntent: Equatable {
    let todoID: String?
    let origin: WAReviewLaunchOrigin
    let continuingRunID: String?
}

enum WAReviewGate: String, CaseIterable {
    case currentGoal = "当前目标/意图有效"
    case dependency = "当前依赖"
    case supervision = "适用监督"
    case skillConsent = "新技能人工同意"
    case noProhibition = "无显式禁令"
    case hostSafety = "宿主硬安全条件"
    case hardLimitCapability = "必需硬限可可靠执行"
}

enum WAReviewBudgetScope: String, CaseIterable {
    case local = "本规则/任务", project = "星图项目", shared = "共同资源组 A"
}

enum WAReviewLimit: String, CaseIterable {
    case concurrency = "新准入并发名额"
    case attempts = "新尝试机会"
    case localBudget = "本规则/任务消耗"
    case projectBudget = "星图项目消耗"
    case sharedBudget = "共同资源组 A 消耗"
    case duration = "本执行独立时长"

    var affectsInFlight: Bool {
        switch self {
        case .concurrency, .attempts: false
        case .localBudget, .projectBudget, .sharedBudget, .duration: true
        }
    }
}

enum WAReviewUsageKind: Equatable {
    case estimate(Int), reported(Int), unknown
    var label: String {
        switch self {
        case let .estimate(value): "示例预估 \(value) 单位"
        case let .reported(value): "示例已报告 \(value) 单位"
        case .unknown: "未知 · 缺值不报零"
        }
    }
}

struct WAReviewUsage: Identifiable, Equatable {
    let id: String
    let role: String
    let scopes: [WAReviewBudgetScope]
    let kind: WAReviewUsageKind
}

struct WAReviewUsageSummary: Equatable {
    let reported: Int?
    let estimated: Int?
    let unknownCount: Int
    var text: String {
        let report = reported.map { String($0) } ?? "无报告"
        let estimate = estimated.map { String($0) } ?? "无预估"
        return "已报告：\(report) · 预估：\(estimate) · 未知记录：\(unknownCount)；非账单"
    }
}

struct WAReviewOpportunity: Identifiable, Equatable {
    let id: String
    let description: String
    // A skipped opportunity is never converted into a run or a wait queue.
}

struct WAReviewEvent: Identifiable, Equatable {
    let id: String
    let title: String
    let detail: String
}

struct WAReviewRuleDraft: Equatable {
    let ownerID: UUID
    let generation: Int
    let baseRevision: Int
    let original: WAReviewRulePolicy
    var edited: WAReviewRulePolicy
    var difference: String {
        var changes: [String] = []
        if original.scope != edited.scope {
            changes.append("范围：\(original.scope.rawValue) → \(edited.scope.rawValue)（可能扩围）")
        }
        if original.configuration != edited.configuration {
            changes.append("配置：\(original.configuration.rawValue) → \(edited.configuration.rawValue)；在途不换绑")
        }
        if original.permission != edited.permission {
            changes.append("权限：\(original.permission.rawValue) → \(edited.permission.rawValue)（提权须明确确认）")
        }
        if original.trigger != edited.trigger { changes.append("触发类别已改变；保存不是到点或运行") }
        if original.recoveryOnce != edited.recoveryOnce { changes.append("恢复后补一次已改变") }
        if original.afterBusyOnce != edited.afterBusyOnce { changes.append("结束后补一次已改变（不继承恢复设置）") }
        if original.autoContinueAfterLimit != edited.autoContinueAfterLimit {
            changes.append("到限资源恢复后自动续行策略已改变（独立同意）")
        }
        return changes.isEmpty ? "策略无差异；确认授权也不等于启用。" : changes.joined(separator: "\n")
    }
}

enum WAReviewBatchKind: String {
    case assignment = "批量明确交办"
    case backlog = "主动纳入旧待办"
}

struct WAReviewBatchDraft {
    let ownerID: UUID
    let generation: Int
    let ruleRevision: Int
    let kind: WAReviewBatchKind
    let candidates: [String]
    var selected: Set<String>
    var excluded: Set<String>
}

struct WAReviewNewTodoDraft {
    let ownerID: UUID
    let generation: Int
    var title = ""
    var source: WAReviewSource = .user
    var category: WAReviewCategory = .implementation
    var recordOnly = false
}

enum WAReviewRunRecord: String, CaseIterable {
    case ended = "执行结束"
    case failed = "执行失败"
    case unknown = "未知回执"
    case processExited = "主进程退出但占用未核实"
    case safeClosure = "原执行、副作用及相关占用安全收束"
}

enum WAReviewVerificationRecord: String, CaseIterable {
    case complete = "整合、必需检查与适用监督全部满足"
    case missingIntegration = "缺整合"
    case missingChecks = "缺必需检查"
    case missingSupervision = "缺适用监督"
    case unknown = "核验未知"
}
#endif
