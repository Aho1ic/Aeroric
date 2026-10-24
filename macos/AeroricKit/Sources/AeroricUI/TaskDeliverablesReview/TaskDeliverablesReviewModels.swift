#if DEBUG
import Foundation

// These finite values belong only to the DEBUG offline specimen. They are not
// protocol DTOs, persistence records, message envelopes, or task states.

enum TaskDeliverablesReviewScenario: String, CaseIterable, Identifiable, Sendable {
    case collection = "TD1"
    case versions = "TD2"
    case reading = "TD3"
    case feedback = "TD4–TD5"
    case evidence = "TD6"
    case rework = "TD7"
    case continuation = "TD8"
    case reset = "TD1–TD8"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .collection: "收录与整理"
        case .versions: "交付版本与检查"
        case .reading: "只读查看与操作意图"
        case .feedback: "版本化反馈与发送"
        case .evidence: "说明、核验与接受"
        case .rework: "同任务返工"
        case .continuation: "条件接续与 Stop"
        case .reset: "完整离线样例"
        }
    }

    var instruction: String {
        switch self {
        case .collection:
            "明确提交项可集中展示；临时日志不自动成为成果，整理不删除原记录。"
        case .versions:
            "切换 v1 / v2 只改变阅读投影；检查范围跟随实际版本，当前工作文件另看。"
        case .reading:
            "查看保持只读；运行、安装和外部打开各自产生未执行的本地意图。"
        case .feedback:
            "意见绑定版本、位置和收件人；草稿取消或空内容不会交办，发送才形成一次意图。"
        case .evidence:
            "自报修复、示例核验和历史用户接受各自保留依据，不自动互相升级。"
        case .rework:
            "已结束工作在原任务内发起返工；旧交付、检查和接受事实保持不变。"
        case .continuation:
            "条件恢复只接续已交办轮次；暂停、Stop、失效授权和未知原执行仍阻止。"
        case .reset:
            "八类关系都在一个可重置的虚构内存窗口内展示，不读写真实资料。"
        }
    }
}

enum TaskDeliverableID: String, CaseIterable, Identifiable, Hashable, Sendable {
    case apiDocument = "deliverable.api-document"
    case validationDiff = "deliverable.validation-diff"
    case previewImage = "deliverable.preview-image"
    case installer = "deliverable.installer"
    case temporaryLog = "deliverable.temporary-log"

    var id: String { rawValue }
}

enum TaskDeliverableKind: String, CaseIterable, Identifiable, Hashable, Sendable {
    case document
    case diff
    case image
    case package
    case log

    var id: String { rawValue }

    var title: String {
        switch self {
        case .document: "阶段文档"
        case .diff: "代码差异"
        case .image: "图片示意"
        case .package: "安装包 / 未知格式"
        case .log: "日志说明"
        }
    }
}

enum TaskDeliverableCollectionState: String, Sendable {
    case collected
    case submitted
    case hidden
    case temporary

    var title: String {
        switch self {
        case .collected: "已收录"
        case .submitted: "已提交 · 待整理"
        case .hidden: "已移出清单 · 原事实保留"
        case .temporary: "临时记录 · 未作为成果提交"
        }
    }
}

enum TaskDeliverableContentKind: String, CaseIterable, Identifiable, Hashable, Sendable {
    case markdown
    case source
    case diff
    case image
    case unknown

    var id: String { rawValue }

    var title: String {
        switch self {
        case .markdown: "Markdown"
        case .source: "源码片段"
        case .diff: "Diff 片段"
        case .image: "图片示意"
        case .unknown: "未知格式"
        }
    }
}

enum TaskDeliverableReadback: String, Sendable {
    case embedded
    case referenceOnly
    case unavailable

    var title: String {
        switch self {
        case .embedded: "内容已内嵌 · 仅供离线阅读"
        case .referenceOnly: "仅有引用 · 当前不可回看完整内容"
        case .unavailable: "交付内容不可回看"
        }
    }
}

enum TaskCheckResult: String, Sendable {
    case passed
    case pending
    case notRun
    case unknown

    var title: String {
        switch self {
        case .passed: "示例已通过 · 仅适用该版本/范围"
        case .pending: "待核对 · 新版本影响点未确认"
        case .notRun: "未运行"
        case .unknown: "结果未知"
        }
    }
}

struct TaskDeliverableCheck: Identifiable, Equatable, Sendable {
    let id: String
    let title: String
    let scope: String
    var result: TaskCheckResult
    let evidence: String
}

struct TaskDeliverableVersion: Identifiable, Equatable, Sendable {
    let id: String
    let number: String
    let source: String
    let contentKind: TaskDeliverableContentKind
    let content: String?
    let readback: TaskDeliverableReadback
    let checks: [TaskDeliverableCheck]
    let knownLocation: String?
    let limitation: String?

    var displayTitle: String { "v\(number)" }

    var contentSummary: String {
        switch readback {
        case .embedded:
            return contentKind.title
        case .referenceOnly:
            return "仅引用 / \(contentKind.title)"
        case .unavailable:
            return "不可回看 / \(contentKind.title)"
        }
    }
}

struct TaskCurrentWorkFile: Equatable, Sendable {
    let title: String
    let content: String
    let note: String
}

struct TaskDeliverable: Identifiable, Equatable, Sendable {
    let id: TaskDeliverableID
    var title: String
    var description: String
    let kind: TaskDeliverableKind
    let sourcePackage: String
    let submittedBy: String
    var collectionState: TaskDeliverableCollectionState
    let versions: [TaskDeliverableVersion]
    let currentWorkFile: TaskCurrentWorkFile?
    let originalTitle: String
    let originalDescription: String

    var isVisibleInCollection: Bool {
        collectionState == .collected || collectionState == .submitted
    }

    var isTemporary: Bool {
        collectionState == .temporary
    }
}

enum TaskDeliverablesReviewLocation: Equatable, Sendable {
    case whole
    case known(String)
    case unknown(String)

    var title: String {
        switch self {
        case .whole: "成果整体"
        case .known(let value): value
        case .unknown(let value): "位置未知 · \(value)"
        }
    }

    var isReliable: Bool {
        if case .known = self { return true }
        return false
    }
}

enum TaskDeliverablesReviewLocationChoice: String, CaseIterable, Identifiable, Sendable {
    case whole
    case known
    case unknown

    var id: String { rawValue }

    var title: String {
        switch self {
        case .whole: "成果整体"
        case .known: "可靠位置"
        case .unknown: "位置未知"
        }
    }
}

enum TaskDeliverablesReviewRecipient: String, CaseIterable, Identifiable, Hashable, Sendable {
    case coordinator
    case apiMember
    case verificationMember

    var id: String { rawValue }

    var title: String {
        switch self {
        case .coordinator: "协调负责人（示例）"
        case .apiMember: "API 成员（示例）"
        case .verificationMember: "核验成员（示例）"
        }
    }
}

struct TaskDeliverablesReviewDraft: Equatable, Sendable {
    let owner: UUID
    let token: Int
    var deliverableID: TaskDeliverableID?
    var versionID: String?
    var location: TaskDeliverablesReviewLocation = .whole
    var recipient: TaskDeliverablesReviewRecipient = .coordinator
    var correctionOf: String?
    var text = ""
}

enum TaskDeliverablesReviewFeedbackState: String, Sendable {
    case sent
    case correctionSent

    var title: String {
        switch self {
        case .sent: "已发送修改要求 · 不等于已消费"
        case .correctionSent: "更正已追加 · 原意见保留"
        }
    }
}

struct TaskDeliverablesReviewFeedback: Identifiable, Equatable, Sendable {
    let id: String
    let taskID: String
    let deliverableID: TaskDeliverableID
    let versionID: String
    let location: TaskDeliverablesReviewLocation
    let text: String
    let recipient: TaskDeliverablesReviewRecipient
    var state: TaskDeliverablesReviewFeedbackState
    let correctionOf: String?

    var isCorrection: Bool { correctionOf != nil }
}

enum TaskDeliverablesReviewEvidenceKind: String, CaseIterable, Identifiable, Sendable {
    case agentReport
    case verification
    case userAccepted

    var id: String { rawValue }

    var title: String {
        switch self {
        case .agentReport: "Agent 说明已修改（示例自报）"
        case .verification: "示例核对结果"
        case .userAccepted: "历史用户接受事实"
        }
    }
}

struct TaskDeliverablesReviewEvidence: Identifiable, Equatable, Sendable {
    let id: String
    let feedbackID: String?
    let versionID: String
    let kind: TaskDeliverablesReviewEvidenceKind
    let detail: String
    let isExampleEvent: Bool
}

enum TaskDeliverablesReviewOperationKind: String, CaseIterable, Identifiable, Sendable {
    case run
    case install
    case externalOpen

    var id: String { rawValue }

    var title: String {
        switch self {
        case .run: "运行"
        case .install: "安装"
        case .externalOpen: "外部打开"
        }
    }
}

struct TaskDeliverablesReviewOperationIntent: Identifiable, Equatable, Sendable {
    let id: String
    let versionID: String
    let kind: TaskDeliverablesReviewOperationKind
    let state: String
    let detail: String
}

enum TaskDeliverablesReviewRoundState: String, Sendable {
    case completed
    case awaitingAcceptance
    case queued
    case blocked
    case readyToContinue

    var title: String {
        switch self {
        case .completed: "历史本轮已完成"
        case .awaitingAcceptance: "历史交付待验收"
        case .queued: "返工已交办 · 等待条件"
        case .blocked: "返工局部受阻"
        case .readyToContinue: "条件满足 · 示例轮次可接续"
        }
    }
}

struct TaskDeliverablesReviewRound: Identifiable, Equatable, Sendable {
    let id: String
    let taskID: String
    let feedbackID: String
    let sourceVersionID: String
    var state: TaskDeliverablesReviewRoundState
    let detail: String
}

struct TaskDeliverablesReviewConditions: Equatable, Sendable {
    var authorized = true
    var resourcesAvailable = false
    var supervisionAvailable = true
    var paused = false
    var stopRequested = false
    var stopVerified = false
    var originalExecutionUnknown = true

    var blockers: [String] {
        var values: [String] = []
        if !authorized { values.append("当前授权不足") }
        if !resourcesAvailable { values.append("资源条件不足") }
        if !supervisionAvailable { values.append("监督条件不足") }
        if paused { values.append("用户暂停") }
        if stopRequested { values.append("Stop 已请求") }
        if stopVerified { values.append("Stop 已核实") }
        if originalExecutionUnknown { values.append("原执行状态未知") }
        return values
    }

    var canContinue: Bool {
        authorized && resourcesAvailable && supervisionAvailable
            && !paused && !stopRequested && !stopVerified && !originalExecutionUnknown
    }
}

enum TaskDeliverablesReviewConditionEvent: String, CaseIterable, Identifiable, Sendable {
    case resourcesRestored
    case resourcesBlocked
    case authorizationRestored
    case authorizationRevoked
    case supervisionRestored
    case supervisionMissing
    case pause
    case resume
    case stopRequested
    case stopVerified
    case executionVerified

    var id: String { rawValue }

    var title: String {
        switch self {
        case .resourcesRestored: "载入资源恢复"
        case .resourcesBlocked: "载入资源不足"
        case .authorizationRestored: "载入当前授权满足"
        case .authorizationRevoked: "载入授权撤回"
        case .supervisionRestored: "载入监督条件满足"
        case .supervisionMissing: "载入监督条件不足"
        case .pause: "载入用户暂停"
        case .resume: "载入明确恢复"
        case .stopRequested: "载入 Stop 请求"
        case .stopVerified: "载入已核实停止"
        case .executionVerified: "载入原执行已核实"
        }
    }
}

struct TaskDeliverablesReviewRecord: Identifiable, Equatable, Sendable {
    let id: Int
    let text: String
    let isExampleEvent: Bool
}
#endif
