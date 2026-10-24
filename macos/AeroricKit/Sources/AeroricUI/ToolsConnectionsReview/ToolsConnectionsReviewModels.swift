#if DEBUG
import Foundation

// Finite review-only values, not production DTOs, permissions or a scheduler.
enum ToolsConnectionsReviewScenario: String, CaseIterable, Identifiable {
    case TC1, TC2, TC3, TC4, TC5, TC6
    var id: String { rawValue }
    var title: String {
        switch self {
        case .TC1: "目录与范围"
        case .TC2: "状态与拟选"
        case .TC3: "引入与更新"
        case .TC4: "停用与恢复"
        case .TC5: "连接与秘密引用"
        case .TC6: "结果与证据"
        }
    }
    var variants: [TConnReviewVariant] {
        switch self {
        case .TC1: [.standard, .blocked]
        case .TC2: [.standard, .blocked, .notEnabled]
        case .TC3:
            [.standard, .installCovered, .updateCovered, .bundledSkill,
             .remoteUnpinned, .rollbackEligible, .rollbackUnknown]
        case .TC4:
            [.standard, .blocked, .paused, .stopped, .cancelled, .unknownExecution, .expired]
        case .TC5: [.standard, .credentialMissing, .credentialRevoked, .credentialLocationMismatch]
        case .TC6: [.standard, .referenceMissing]
        }
    }
}

enum TConnReviewVariant: String, Identifiable {
    case standard, blocked, notEnabled, installCovered, updateCovered, bundledSkill, remoteUnpinned
    case rollbackEligible, rollbackUnknown, paused, stopped, cancelled, unknownExecution, expired
    case credentialMissing, credentialRevoked, credentialLocationMismatch, referenceMissing
    var id: String { rawValue }
    var title: String {
        switch self {
        case .standard: "普通示例"
        case .blocked: "条件仍缺"
        case .notEnabled: "已保存但未启用"
        case .installCovered: "安装预授权覆盖"
        case .updateCovered: "更新预授权覆盖"
        case .bundledSkill: "Full Access 与附带新技能"
        case .remoteUnpinned: "远端版本不可固定"
        case .rollbackEligible: "更新失败 · 回退条件齐备"
        case .rollbackUnknown: "更新失败 · 回退条件未知"
        case .paused: "用户暂停"
        case .stopped: "用户 Stop"
        case .cancelled: "用户取消"
        case .unknownExecution: "原写入结果未知"
        case .expired: "等待已到限"
        case .credentialMissing: "凭据引用缺项"
        case .credentialRevoked: "使用许可已撤销"
        case .credentialLocationMismatch: "执行位置不匹配"
        case .referenceMissing: "原结果引用失效"
        }
    }
}

enum TConnReviewCategory: String, CaseIterable, Identifiable {
    case method, capability, connection
    var id: String { rawValue }
    var title: String {
        switch self {
        case .method: "工作方法"
        case .capability: "工具能力"
        case .connection: "资源连接"
        }
    }
}

enum TConnReviewFactState: String { case available, blocked, unknown, unchecked, stale, notApplicable }

struct TConnReviewFact: Equatable {
    let state: TConnReviewFactState
    let text: String
    let source: String
    static func ready(_ text: String) -> Self {
        .init(state: .available, text: text, source: "固定虚构预设 · 非本机探测")
    }
    static func blocked(_ text: String) -> Self {
        .init(state: .blocked, text: text, source: "固定虚构预设 · 不代表真实能力")
    }
    static func unknown(_ text: String) -> Self {
        .init(state: .unknown, text: text, source: "无可靠依据，保持未知")
    }
    static func event(_ text: String, _ event: TConnReviewEvent) -> Self {
        .init(state: .available, text: text, source: "\(event.rawValue) · 示例事件，未实际执行")
    }
}

struct ToolsConnectionsReviewCatalogItem: Identifiable, Equatable {
    let id: String
    let title: String
    let category: TConnReviewCategory
    let purpose: String
    let references: [String]
    let target: String
    let installation: TConnReviewFact
    let support: TConnReviewFact
    let service: TConnReviewFact
    let provenance: String
}

enum ToolsConnectionsReviewContext: String, CaseIterable, Identifiable {
    case alphaOne, alphaTwo, betaOne, standalone
    var id: String { rawValue }
    var project: String? {
        switch self {
        case .alphaOne, .alphaTwo: "demo-project-alpha"
        case .betaOne: "demo-project-beta"
        case .standalone: nil
        }
    }
    var title: String {
        switch self {
        case .alphaOne: "项目甲 · 任务一"
        case .alphaTwo: "项目甲 · 任务二"
        case .betaOne: "项目乙 · 任务一"
        case .standalone: "无项目任务"
        }
    }
}

enum TConnReviewScope: String, CaseIterable, Identifiable {
    case task, project
    var id: String { rawValue }
    var title: String { self == .task ? "仅本任务" : "本项目" }
}

struct TConnReviewScopeDraft: Equatable {
    let itemID: String
    let context: ToolsConnectionsReviewContext
    var scope: TConnReviewScope
    var enabled: Bool
    var temporary: Bool
}

struct TConnReviewLayer: Identifiable {
    let id: String
    let title: String
    let fact: TConnReviewFact
}

struct TConnReviewProjection: Equatable {
    let eligible: Bool
    let title: String
    let reasons: [String]
}

enum TConnReviewUserConstraint: String, CaseIterable, Identifiable {
    case automatic, specified, disabled, ambiguousTarget
    var id: String { rawValue }
    var title: String {
        switch self {
        case .automatic: "范围内自主拟选"
        case .specified: "仅指定当前条目"
        case .disabled: "用户禁用当前条目"
        case .ambiguousTarget: "目标待澄清"
        }
    }
}

struct TConnReviewIdentity: Equatable {
    let target = "demo-development-target"
    let executor = "Codex · 虚构支持预设"
    let model = "demo-model"
    let api = "demo-api"
    let session = "demo-session"
    let permissions = "demo-task-read-scope"
    let cwd = "demo-workspace"
    let pty = "demo-pty-generation"
}

enum TConnReviewOperation: String, CaseIterable, Identifiable {
    case install, update, rollback
    var id: String { rawValue }
    var title: String {
        switch self {
        case .install: "引入外部工具"
        case .update: "审阅 v2 更新"
        case .rollback: "审阅失败回退"
        }
    }
}

struct TConnReviewIntroductionDraft: Equatable {
    let operation: TConnReviewOperation
    let itemID: String
    let context: ToolsConnectionsReviewContext
    let target: String
    let version: String
    let source: String
    let location: String
    let dependencies: String
    let differences: String
    let permissions: String
    let introducesSkill: Bool
}

struct TConnReviewApprovedIntent: Equatable {
    let draft: TConnReviewIntroductionDraft
    let preauthorized: Bool
}

struct TConnReviewSkillConsent: Hashable {
    let context: ToolsConnectionsReviewContext
    let itemID: String
    let version: String
}

enum TConnReviewDisposition: String {
    case waiting = "允许等待"
    case paused = "用户暂停"
    case stopped = "用户 Stop"
    case cancelled = "用户取消"
    case expired = "等待到限"
}

enum TConnReviewStopState: String {
    case notRequested = "未请求停止"
    case requested = "示例停止请求已提出 · 结果待核实"
    case confirmed = "示例停止事实已载入 · 不证明回滚"
}

struct TConnReviewContinuation: Equatable {
    var disposition: TConnReviewDisposition = .waiting
    var assignmentValid = true
    var allowsWaiting = true
    var targetMatches = true
    var permissionCovered = true
    var resourceReady = false
    var dependenciesReady = true
    var originalResultKnown = true
}

enum TConnReviewCredentialState: String {
    case referenced = "仅有不可解析的示例引用"
    case missing = "凭据引用缺项"
    case revoked = "许可已撤销，缓存不能绕过"
    case locationMismatch = "供给位置与执行位置不匹配"
}

enum TConnReviewCredentialIntent: String, CaseIterable, Identifiable {
    case use, original, export, otherModel, crossHost
    var id: String { rawValue }
    var title: String {
        switch self {
        case .use: "最少必要使用示意"
        case .original: "原文查看意图"
        case .export: "原文导出意图"
        case .otherModel: "另一模型供给意图"
        case .crossHost: "跨 Host 分发意图"
        }
    }
}

struct TConnReviewResultEvidence: Equatable {
    var returned: TConnReviewFact
    var business: TConnReviewFact
    var read: TConnReviewFact = .unknown("指定执行者读取未知")
    var adopted: TConnReviewFact = .unknown("指定执行者采用未知")
    var verified: TConnReviewFact = .unknown("技术核验未知")
    var accepted: TConnReviewFact = .unknown("用户验收未知")
    var modelReport = "模型自报“已看过” · 非宿主消费或正确性证据"
}

struct TConnReviewResult: Identifiable, Equatable {
    let id: String
    let title: String
    let source: String
    let preview: String
    let body: String
    let limits: [String]
    let errors: [String]
    let referenceAvailable: Bool
    var evidence: TConnReviewResultEvidence
}

enum TConnReviewEvent: String, CaseIterable, Identifiable {
    case taskAuthorized = "demo-event-task-authorization"
    case installationSucceeded = "demo-event-installation-result"
    case updateSucceeded = "demo-event-update-result"
    case updateFailed = "demo-event-update-failure"
    case stopped = "demo-event-stop-confirmation"
    case conditionsRestored = "demo-event-condition-restoration"
    case originalExecutionVerified = "demo-event-original-execution-checked"
    case resultRead = "demo-event-result-read"
    case resultAdopted = "demo-event-result-adopted"
    case resultVerified = "demo-event-result-verified"
    case resultAccepted = "demo-event-historical-acceptance"
    var id: String { rawValue }
    var title: String {
        switch self {
        case .taskAuthorized: "本任务操作授权示例"
        case .installationSucceeded: "安装结果示例（不是安装按钮）"
        case .updateSucceeded: "新版结果示例（不更换在途版本）"
        case .updateFailed: "更新失败示例"
        case .stopped: "停止结果已核实示例"
        case .conditionsRestored: "资源条件恢复示例"
        case .originalExecutionVerified: "原执行结果核实示例"
        case .resultRead: "指定执行者读取事实示例"
        case .resultAdopted: "指定执行者采用事实示例"
        case .resultVerified: "技术核验事实示例"
        case .resultAccepted: "历史用户验收事实示例"
        }
    }
}
#endif
