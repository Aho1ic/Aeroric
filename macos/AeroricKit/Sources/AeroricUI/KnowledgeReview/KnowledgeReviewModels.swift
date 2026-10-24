#if DEBUG
import Foundation

// Finite, fictional values for the Q45 offline window. These are deliberately
// internal UI sample types, not a knowledge protocol, rule engine or storage
// contract. They contain no file locations, executable expressions or clients.

enum KnowledgeReviewScenario: String, CaseIterable, Identifiable, Hashable, Sendable {
    case candidates, admission, exceptions, references, revisions, retirement, impact, reset

    var id: String { rawValue }
    var code: String {
        switch self {
        case .candidates: "K1"
        case .admission: "K2"
        case .exceptions: "K3"
        case .references: "K4"
        case .revisions: "K5"
        case .retirement: "K6"
        case .impact: "K7"
        case .reset: "K8"
        }
    }
    var title: String {
        switch self {
        case .candidates: "集中审阅"
        case .admission: "自动入库与授权独立"
        case .exceptions: "例外仍留候选"
        case .references: "按需参考"
        case .revisions: "纠正与版本"
        case .retirement: "停用与后续引用"
        case .impact: "在途局部核对"
        case .reset: "无持久化与重置"
        }
    }
    var menuTitle: String { "\(code) \(title)" }
    var detail: String {
        switch self {
        case .candidates:
            "批量确认两条安全候选、修改或丢弃；来源与适用限制一直保留。"
        case .admission:
            "仅对一批固定新候选演示预设；自动入库与任务授权分别设置。"
        case .exceptions:
            "冲突、脱敏占位、依据不足和项目范围外内容不能强行入库。"
        case .references:
            "选择虚构任务或工作包；旧 SQLite 记录不能覆盖当前 Postgres 要求。"
        case .revisions:
            "用户纠正形成有依据的示例 v2；Agent 猜测不能替代用户知识。"
        case .retirement:
            "停用只排除后续推荐；获准历史和已经捕获的旧引用仍可回看。"
        case .impact:
            "通知与 ACK 不等于纠正；预置局部记录仅适用于一条接口工作。"
        case .reset:
            "重置会清除本窗口的草稿、勾选、配置及版本变化，不写真实偏好。"
        }
    }
}

enum KnowledgeReviewSection: String, CaseIterable, Identifiable, Hashable, Sendable {
    case candidates, reusable, history
    var id: String { rawValue }
    var title: String {
        switch self {
        case .candidates: "候选"
        case .reusable: "可复用内容"
        case .history: "历史"
        }
    }
}

enum KnowledgeReviewProject: String, CaseIterable, Identifiable, Hashable, Sendable {
    case harbor, other
    var id: String { rawValue }
    var title: String {
        switch self {
        case .harbor: "虚构项目 · 海港"
        case .other: "虚构项目 · 山谷（范围外）"
        }
    }
}

enum KnowledgeReviewItemID: String, CaseIterable, Identifiable, Hashable, Sendable {
    case conciseReplies, verifiedCheck, discardable, selfReport
    case databaseChoice, interfaceContract, docsGuidance, agentGuess
    case newPreference, newSolution, conflict, sensitive, insufficient, outOfScope
    var id: String { rawValue }
}

enum KnowledgeReviewKind: String, CaseIterable, Identifiable, Hashable, Sendable {
    case preference, decision, solution
    var id: String { rawValue }
    var title: String {
        switch self {
        case .preference: "偏好／纠正示例"
        case .decision: "决策背景示例"
        case .solution: "解法示例"
        }
    }
}

enum KnowledgeReviewSourceKind: String, Sendable {
    case userStatement, userCorrection, preloadedCheck, agentReport
    var title: String {
        switch self {
        case .userStatement: "虚构用户陈述"
        case .userCorrection: "虚构用户纠正"
        case .preloadedCheck: "虚构预置检查记录"
        case .agentReport: "虚构 Agent 自报"
        }
    }
}

struct KnowledgeReviewSource: Identifiable, Equatable, Sendable {
    let id: String
    let kind: KnowledgeReviewSourceKind
    let title: String
    /// Embedded excerpt only. nil means unavailable, never a request to read
    /// a file, fetch a URL, or invent a missing source.
    let excerpt: String?
    var isAvailable: Bool {
        guard let excerpt else { return false }
        return !excerpt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var availabilityNote: String {
        isAvailable ? "内嵌虚构摘录 · 不进行真实回源" : "示例来源缺失 · 不编造回源"
    }
}

struct KnowledgeReviewApplicability: Equatable, Sendable {
    let project: KnowledgeReviewProject
    var summary: String
    let limits: String
}

enum KnowledgeReviewEvidence: String, Sendable {
    case userStatement, preloadedVerified, unverifiedAgentReport
    var title: String {
        switch self {
        case .userStatement: "用户陈述示例 · 不代表核验"
        case .preloadedVerified: "预置已核验示例 · PRELOADED · 非本次实测"
        case .unverifiedAgentReport: "Agent 自报示例 · 未核验"
        }
    }
}

enum KnowledgeReviewCandidateIssue: String, CaseIterable, Sendable {
    case conflict, sensitivePlaceholder, insufficientEvidence, outsideProject, agentGuess
    var title: String {
        switch self {
        case .conflict: "与用户确认内容冲突"
        case .sensitivePlaceholder: "敏感内容脱敏占位"
        case .insufficientEvidence: "依据不足"
        case .outsideProject: "超出项目预设"
        case .agentGuess: "Agent 新猜测"
        }
    }
    var detail: String {
        switch self {
        case .conflict: "与虚构用户已确认的接口约定冲突，仍待用户复核。"
        case .sensitivePlaceholder: "仅有［已移除的敏感内容占位］；不保留敏感原文，不得强行收纳。"
        case .insufficientEvidence: "只有虚构自报或缺失依据，不能作为已核验成功经验入库。"
        case .outsideProject: "属于另一虚构项目，不扩到本项目、全局或可执行技能。"
        case .agentGuess: "Agent 猜测没有用户纠正依据，不得覆盖用户确认的知识。"
        }
    }
}

enum KnowledgeReviewItemState: String, Sendable {
    case candidate, reusable, discarded, retired
    var title: String {
        switch self {
        case .candidate: "候选 · 待审阅"
        case .reusable: "样例内可复用 · 不代表当前已核验"
        case .discarded: "已丢弃示例 · 仅历史"
        case .retired: "已停用示例 · 不再推荐"
        }
    }
}

enum KnowledgeReviewAdmissionOrigin: String, Sendable {
    case preloaded, manual, automatic
    var title: String {
        switch self {
        case .preloaded: "预置虚构知识 · 非本次入库"
        case .manual: "本窗口手动确认示例"
        case .automatic: "本窗口固定预设自动入库示例"
        }
    }
}

enum KnowledgeReviewVersionState: String, Sendable {
    case current, superseded, retired
    var title: String {
        switch self {
        case .current: "当前样例版本"
        case .superseded: "已被替代的历史版本"
        case .retired: "停用版本 · 仅历史"
        }
    }
}

struct KnowledgeReviewVersionID: Hashable, Identifiable, Sendable {
    let itemID: KnowledgeReviewItemID
    let number: Int
    var id: String { "\(itemID.rawValue).v\(number)" }
    var title: String { "v\(number)" }
}

struct KnowledgeReviewVersion: Identifiable, Equatable, Sendable {
    let itemID: KnowledgeReviewItemID
    let number: Int
    var content: String
    var source: KnowledgeReviewSource
    var applicability: KnowledgeReviewApplicability
    var evidence: KnowledgeReviewEvidence
    var state: KnowledgeReviewVersionState
    let correctionBasis: String?
    let replaces: KnowledgeReviewVersionID?
    var id: KnowledgeReviewVersionID { .init(itemID: itemID, number: number) }
    var title: String { "v\(number)" }
}

struct KnowledgeReviewItem: Identifiable, Equatable, Sendable {
    let id: KnowledgeReviewItemID
    let title: String
    let kind: KnowledgeReviewKind
    var state: KnowledgeReviewItemState
    let issue: KnowledgeReviewCandidateIssue?
    var current: KnowledgeReviewVersion
    var history: [KnowledgeReviewVersion]
    var admissionOrigin: KnowledgeReviewAdmissionOrigin?

    var versions: [KnowledgeReviewVersion] { history + [current] }
    var hasHistory: Bool { !history.isEmpty || state == .discarded || state == .retired }
    var source: KnowledgeReviewSource { current.source }
    var applicability: KnowledgeReviewApplicability { current.applicability }
    var content: String { current.content }
    var evidence: KnowledgeReviewEvidence { current.evidence }

    var confirmationBlockedReason: String? {
        guard state == .candidate else { return "当前项不是待审阅候选。" }
        if let issue { return issue.detail }
        guard applicability.project == .harbor else {
            return KnowledgeReviewCandidateIssue.outsideProject.detail
        }
        guard source.isAvailable else { return "示例来源缺失，仍待复核。" }
        guard evidence != .unverifiedAgentReport else {
            return KnowledgeReviewCandidateIssue.insufficientEvidence.detail
        }
        guard !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !applicability.summary.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return "内容与适用说明不能为空。"
        }
        return nil
    }
}

// MARK: - Fixed configuration and window-only drafts

enum KnowledgeReviewPermission: String, CaseIterable, Identifiable, Hashable, Sendable {
    case standard, fullAccess
    var id: String { rawValue }
    var title: String {
        switch self {
        case .standard: "默认授权示例"
        case .fullAccess: "Full Access 示例"
        }
    }
}

/// Exactly two preview presets, not combinable expressions or executable rules.
enum KnowledgeReviewAdmissionPreset: String, CaseIterable, Identifiable, Hashable, Sendable {
    case userPreferences, verifiedSolutions
    var id: String { rawValue }
    var title: String {
        switch self {
        case .userPreferences: "本项目 · 用户偏好"
        case .verifiedSolutions: "本项目 · 预置核验解法"
        }
    }
    var kind: KnowledgeReviewKind {
        switch self {
        case .userPreferences: .preference
        case .verifiedSolutions: .solution
        }
    }
    var sourceKind: KnowledgeReviewSourceKind {
        switch self {
        case .userPreferences: .userStatement
        case .verifiedSolutions: .preloadedCheck
        }
    }
    var project: KnowledgeReviewProject { .harbor }
    var detail: String {
        "\(kind.title) / \(sourceKind.title) / \(project.title)；仅固定样例，不扩权或生成技能。"
    }
}

struct KnowledgeReviewAdmissionConfiguration: Equatable, Sendable {
    var enabled: Bool = false
    var preset: KnowledgeReviewAdmissionPreset = .userPreferences
}

struct KnowledgeReviewContentDraft: Identifiable, Equatable, Sendable {
    let id: UUID
    let generation: UUID
    let originalVersion: KnowledgeReviewVersionID
    var content: String
    var applicability: String
}

struct KnowledgeReviewConfigurationDraft: Identifiable, Equatable, Sendable {
    let id: UUID
    let generation: UUID
    var configuration: KnowledgeReviewAdmissionConfiguration
}

struct KnowledgeReviewRevisionDraft: Identifiable, Equatable, Sendable {
    let id: UUID
    let generation: UUID
    let originalVersion: KnowledgeReviewVersionID
    var content: String
    var applicability: String
    var basis: String
}

// MARK: - Scoped reference projections and captured, immutable old references

enum KnowledgeReviewWorkID: String, CaseIterable, Identifiable, Hashable, Sendable {
    case storageTask, apiTask, clientPackage, docsPackage
    var id: String { rawValue }
}

enum KnowledgeReviewWorkKind: String, Sendable {
    case task, workPackage
    var title: String { self == .task ? "虚构任务" : "虚构工作包" }
}

struct KnowledgeReviewWork: Identifiable, Equatable, Sendable {
    let id: KnowledgeReviewWorkID
    let kind: KnowledgeReviewWorkKind
    let title: String
    let request: String
    let relatedItemIDs: [KnowledgeReviewItemID]
}

enum KnowledgeReviewReferenceScenario: String, CaseIterable, Identifiable, Hashable, Sendable {
    case sourced, missingSource, currentUserRequest, outdatedConflict
    var id: String { rawValue }
    var title: String {
        switch self {
        case .sourced: "带来源参考"
        case .missingSource: "来源缺失"
        case .currentUserRequest: "当前明确要求优先"
        case .outdatedConflict: "过时／冲突待复核"
        }
    }
}

enum KnowledgeReviewReferenceCondition: String, Sendable {
    case reference, missingSource, currentRequestPriority, outdatedConflict, retired
    var title: String {
        switch self {
        case .reference: "带条件的虚构参考"
        case .missingSource: "来源缺失 · 不作为当前事实"
        case .currentRequestPriority: "当前明确要求优先 · 旧记录不强制"
        case .outdatedConflict: "过时／冲突 · 待复核"
        case .retired: "已停用 · 仅历史回看"
        }
    }
}

struct KnowledgeReviewReference: Identifiable, Equatable, Sendable {
    let itemID: KnowledgeReviewItemID
    let itemTitle: String
    let version: KnowledgeReviewVersion
    let condition: KnowledgeReviewReferenceCondition
    let note: String
    var id: KnowledgeReviewVersionID { version.id }
    var isRecommended: Bool { condition == .reference && version.state == .current }
    var summary: String { version.content }
    var source: KnowledgeReviewSource { version.source }
    var applicability: KnowledgeReviewApplicability { version.applicability }
}

struct KnowledgeReviewCapturedReference: Identifiable, Equatable, Sendable {
    let workID: KnowledgeReviewWorkID
    let itemTitle: String
    /// A value snapshot: corrections and retirement never rewrite it.
    let version: KnowledgeReviewVersion
    var id: String { "\(workID.rawValue).\(version.id.id)" }
}

// MARK: - One bounded preloaded local check, never a live execution result

enum KnowledgeReviewDelivery: String, Sendable {
    case notSent, notified, acknowledged
    var title: String {
        switch self {
        case .notSent: "尚未演示通知"
        case .notified: "已演示通知 · 不代表纠正"
        case .acknowledged: "已演示 ACK · 不代表核验"
        }
    }
}

enum KnowledgeReviewImpactState: String, Sendable {
    case unaffected, pending, checkedPreloaded, continuingDemo
    var title: String {
        switch self {
        case .unaffected: "安全独立文档 · 可继续"
        case .pending: "受影响 · 待局部核对"
        case .checkedPreloaded: "预置局部核对示例 · PRELOADED · 非本次实测"
        case .continuingDemo: "独立文档已演示继续 · 仅内存"
        }
    }
}

struct KnowledgeReviewCheckRecord: Identifiable, Equatable, Sendable {
    let id: String
    let workID: KnowledgeReviewWorkID
    let capturedVersion: KnowledgeReviewVersionID
    let applicableVersion: KnowledgeReviewVersionID
    let source: KnowledgeReviewSource
    let summary: String
    var title: String { "虚构预置局部核对记录 · PRELOADED · 非本次实测" }
}

struct KnowledgeReviewImpact: Identifiable, Equatable, Sendable {
    let id: KnowledgeReviewWorkID
    let title: String
    let capturedReference: KnowledgeReviewCapturedReference?
    var requiredVersion: KnowledgeReviewVersionID?
    var state: KnowledgeReviewImpactState
    var delivery: KnowledgeReviewDelivery
    var loadedRecord: KnowledgeReviewCheckRecord?
    var note: String
    var isAffected: Bool { capturedReference != nil }
}
#endif
