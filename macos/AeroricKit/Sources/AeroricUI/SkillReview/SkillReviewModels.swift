#if DEBUG
import Foundation

// Window-local specimens, NOT a skill registry, production DTO or install protocol.
enum SkillReviewScenario: String, CaseIterable, Identifiable, Hashable, Sendable {
    case drafts = "S1", consent = "S2", scope = "S3", tasks = "S4"
    case availability = "S5", updates = "S6", inFlight = "S7", reset = "S8"

    var id: String { rawValue }
    var title: String {
        switch self {
        case .drafts: "草稿分审"
        case .consent: "新技能人工同意"
        case .scope: "范围与权限分离"
        case .tasks: "任务按需选用"
        case .availability: "可用性有依据"
        case .updates: "更新先审阅"
        case .inFlight: "在途不是永久豁免"
        case .reset: "离线与重置"
        }
    }
    var menuTitle: String { "\(rawValue) · \(title)" }
    var summary: String {
        switch self {
        case .drafts: "示例：两份方法分开审阅；确认内容不是安装、启用或指令授权。"
        case .consent: "示例：先审阅具体内容、版本与项目范围；同意后另载入演示结果。"
        case .scope: "示例：统一管理不等于全局启用；Full Access 不扩大技能范围。"
        case .tasks: "示例：不同工作需要不同方法；只选已获准、启用且预设可用的相关项。"
        case .availability: "示例：四执行者均是预设，非实际兼容结论；未知不当作可用。"
        case .updates: "示例：安装同意、结果、版本启用、可用性与任务选用分开。"
        case .inFlight: "示例：旧引用保留，但 Stop、撤销、不安全和宿主硬拦截优先。"
        case .reset: "示例：重置、关闭重开、退出重启恢复内存 fixture；不保存真实偏好。"
        }
    }
}

enum SkillReviewSection: String, CaseIterable, Identifiable, Hashable, Sendable {
    case drafts, requests, skills, updates
    var id: String { rawValue }
    var title: String {
        switch self {
        case .drafts: "方法草稿（示例）"
        case .requests: "安装申请（示例）"
        case .skills: "项目技能（示例）"
        case .updates: "版本更新（示例）"
        }
    }
}

enum SkillReviewProject: String, CaseIterable, Identifiable, Hashable, Sendable {
    case harbor, tide
    var id: String { rawValue }
    var title: String { self == .harbor ? "星港排程（虚构项目）" : "潮汐笔记（虚构项目）" }
}

enum SkillReviewExecutor: String, CaseIterable, Identifiable, Hashable, Sendable {
    case claude, codex, omp, dsh
    var id: String { rawValue }
    var name: String {
        switch self {
        case .claude: "Claude Code"
        case .codex: "Codex"
        case .omp: "omp"
        case .dsh: "dsh"
        }
    }
    var title: String { "\(name)（预设，非实际兼容结论）" }
}

enum SkillReviewItemID: String, CaseIterable, Hashable, Sendable {
    case boundary, keyboard, dependency, unknown, unrelated, candidate, missingSource
}

enum SkillReviewWorkKind: String, Hashable, Sendable { case api, keyboard, archive }
enum SkillReviewWorkID: String, CaseIterable, Identifiable, Hashable, Sendable {
    case api, keyboard, inFlight, newTask
    var id: String { rawValue }
}

struct SkillReviewSource: Equatable, Sendable {
    let title: String
    let excerpt: String
    let limitation: String
}

enum SkillReviewAvailabilityState: String, Sendable {
    case available, blocked, unknown
    var title: String {
        switch self {
        case .available: "预设情景可用"
        case .blocked: "预设情景不可用"
        case .unknown: "预设可用性未知"
        }
    }
    var symbol: String {
        switch self {
        case .available: "checkmark.circle"
        case .blocked: "exclamationmark.triangle"
        case .unknown: "questionmark.circle"
        }
    }
}

struct SkillReviewAvailability: Equatable, Sendable {
    let state: SkillReviewAvailabilityState
    let reason: String
}

struct SkillReviewVersion: Equatable, Sendable {
    let itemID: SkillReviewItemID
    var title: String
    let number: String
    var content: String
    var source: SkillReviewSource?
    var conditions: String
    var validation: String
    var intendedUse: String
    var capabilities: [String]
    var dependencies: [String]
    var scopeNote: String
    let relevantWork: Set<SkillReviewWorkKind>
    var availability: [SkillReviewExecutor: SkillReviewAvailability]

    func availability(for executor: SkillReviewExecutor) -> SkillReviewAvailability {
        guard source != nil else {
            return .init(state: .unknown, reason: "示例没有来源；不当作已验证或可用。")
        }
        return availability[executor]
            ?? .init(state: .unknown, reason: "没有该执行者的预置记录，非实际兼容结论。")
    }
}

/// Equality binds consent to the displayed specimen, not a production signature/hash.
struct SkillReviewConsentTarget: Equatable, Sendable {
    let version: SkillReviewVersion
    let project: SkillReviewProject
    var summary: String { "\(version.title) · v\(version.number) · 仅\(project.title)（示例）" }
}

enum SkillReviewContentState: String, Sendable {
    case pending, confirmed, discarded
    var title: String {
        switch self {
        case .pending: "内容草稿待审（示例）"
        case .confirmed: "内容与当前项目范围已确认（示例，未授权安装）"
        case .discarded: "草稿已丢弃（示例）"
        }
    }
}

enum SkillReviewConsentResponse: String, Sendable {
    case unanswered, refused, agreed
    var title: String {
        switch self {
        case .unanswered: "安装申请未答（示例）"
        case .refused: "安装申请已拒绝（示例）"
        case .agreed: "已同意所展示内容、版本与项目范围（示例）"
        }
    }
}

enum SkillReviewInstallOutcome: String, CaseIterable, Identifiable, Sendable {
    case unloaded, success, failure
    var id: String { rawValue }
    var title: String {
        switch self {
        case .unloaded: "演示安装结果尚未载入"
        case .success: "已载入预置安装成功记录（示例）"
        case .failure: "已载入预置安装失败记录（示例）"
        }
    }
}

struct SkillReviewItem: Identifiable, Equatable, Sendable {
    let id: SkillReviewItemID
    var proposal: SkillReviewVersion
    var contentState: SkillReviewContentState = .confirmed
    var response: SkillReviewConsentResponse = .unanswered
    var consentIsPreloaded = false
    var approvedTarget: SkillReviewConsentTarget?
    var installOutcome: SkillReviewInstallOutcome = .unloaded
    var installedTarget: SkillReviewConsentTarget?
    var enabledTarget: SkillReviewConsentTarget?
    // Captured when staging an update, independent of in-flight references.
    // Approving/enabling the candidate never turns a diff into a self-compare.
    var updateBaseline: SkillReviewConsentTarget?
    var approvedHistory: [SkillReviewConsentTarget] = []
    var installedHistory: [SkillReviewConsentTarget] = []
    // Historical evidence is immutable; these finite records describe current
    // withdrawal/failure without erasing or resurrecting that evidence.
    var withdrawnTargets: [SkillReviewConsentTarget] = []
    var unusableInstallTargets: [SkillReviewConsentTarget] = []

    var target: SkillReviewConsentTarget { .init(version: proposal, project: .harbor) }
    var updateComparisonTarget: SkillReviewConsentTarget? {
        updateBaseline ?? enabledTarget ?? installedHistory.last
    }
    // The same specimen that select() captures must label its actual controls.
    var taskSelectionVersion: SkillReviewVersion { enabledTarget?.version ?? proposal }
    var hasMatchingConsent: Bool { response == .agreed && approvedTarget == target }
    var consentTitle: String {
        consentIsPreloaded ? "情景预置同意记录（示例，并非本次用户批准）" : response.title
    }
    var canEnable: Bool {
        hasMatchingConsent && installOutcome == .success && installedTarget == target
    }
}

enum SkillReviewSelectionOrigin: String, Sendable {
    case manual, agent, preloaded
    var title: String {
        switch self {
        case .manual: "用户手选（示例）"
        case .agent: "演示 Agent 按需选用"
        case .preloaded: "预置在途引用（示例）"
        }
    }
}

struct SkillReviewReference: Identifiable, Equatable, Sendable {
    var id: SkillReviewItemID { target.version.itemID }
    let target: SkillReviewConsentTarget
    let origin: SkillReviewSelectionOrigin
}

struct SkillReviewWork: Identifiable, Equatable, Sendable {
    let id: SkillReviewWorkID
    let title: String
    let requirement: String
    let kind: SkillReviewWorkKind
    let isInFlight: Bool
    var references: [SkillReviewReference] = []
}

enum SkillReviewInFlightCondition: String, CaseIterable, Identifiable, Hashable, Sendable {
    case allowed, revoked, unsafe, hostBlocked
    var id: String { rawValue }
    var title: String {
        switch self {
        case .allowed: "载入：旧版仍获准、安全且宿主允许（预置）"
        case .revoked: "载入：旧版授权已撤销（预置）"
        case .unsafe: "载入：旧版不安全（预置）"
        case .hostBlocked: "载入：旧版被宿主硬拦截（预置）"
        }
    }
    var blockingReason: String? {
        switch self {
        case .allowed: nil
        case .revoked: "预置记录：旧版授权已撤销，不得继续使用。"
        case .unsafe: "预置记录：旧版不安全，不得继续使用。"
        case .hostBlocked: "预置记录：宿主硬拦截优先，不得继续使用。"
        }
    }
}

enum SkillReviewAppearance: String, CaseIterable, Identifiable, Hashable, Sendable {
    case system, light, dark
    var id: String { rawValue }
    var title: String {
        switch self {
        case .system: "跟随系统"
        case .light: "浅色预览"
        case .dark: "深色预览"
        }
    }
}

enum SkillReviewInteraction: String, CaseIterable, Identifiable, Hashable, Sendable {
    case native, advanced
    var id: String { rawValue }
    var title: String { self == .native ? "native 呈现（示例）" : "advanced 呈现（示例，无 PTY）" }
}

struct SkillReviewPreview: Equatable, Sendable {
    var appearance: SkillReviewAppearance = .system
    var glass = true
    var interaction: SkillReviewInteraction = .native
    var fullAccess = false
    var reduceTransparency = false
    var increaseContrast = false
    var reduceMotion = false
    var sizeID = "regular"
}

struct SkillReviewEditDraft: Identifiable, Equatable, Sendable {
    let id: UUID
    let generation: UUID
    let viewingProject: SkillReviewProject
    var version: SkillReviewVersion
}

enum SkillReviewSheet: Identifiable, Equatable {
    case edit(SkillReviewEditDraft), source(SkillReviewVersion), scope, appearance
    var id: String {
        switch self {
        case let .edit(draft): "edit-\(draft.id)"
        case let .source(version): "source-\(version.itemID.rawValue)-\(version.number)"
        case .scope: "scope"
        case .appearance: "appearance"
        }
    }
}
#endif
