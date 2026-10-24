#if DEBUG
import Foundation

// Finite, window-local specimens. These are not message DTOs, a delivery
// protocol, permission enforcement or a production queue state machine.
enum TCReviewScenario: String, CaseIterable, Identifiable, Sendable {
    case input = "TC1", editing = "TC2", delivery = "TC3", targeting = "TC4"
    case questions = "TC5", continuation = "TC6", requirements = "TC7", conflict = "TC8"
    var id: String { rawValue }
    var title: String {
        switch self {
        case .input: "用途与原问题锚定"
        case .editing: "可控编辑与撤回竞争"
        case .delivery: "交付未知与关联更正"
        case .targeting: "成员定向与局部同步"
        case .questions: "集中待你决定"
        case .continuation: "下一轮与暂停 / Stop"
        case .requirements: "普通补充与明确更正"
        case .conflict: "含糊冲突与局部澄清"
        }
    }
    var instruction: String {
        switch self {
        case .input: "试着回答接口成员的问题，再查看另一成员；答复仍回原提问者。补充默认只记待投递意图，不假装即时注入。"
        case .editing: "编辑 M1 后可取消；也可在编辑期间载入交出或未知事件，再尝试提交 / 撤回，观察条件重核。"
        case .delivery: "分别对已交出与交付未知记录追加更正。原文和交付事实不变；消费事件也不代表整改或验收。"
        case .targeting: "更正共享接口 M1，查看协调者与实际受影响成员各自的同步证据；独立文档不被广播或暂停。"
        case .questions: "两个成员的问题集中在这里。稍后不等于同意，发送答案不是权限审批或工作完成。"
        case .continuation: "两条下一轮已交办。载入轮次边界或条件事件观察 FIFO；暂停 / Stop / 撤权 / 取消不会被恢复资源唤醒。"
        case .requirements: "将 M1 的颜色要求明确更正为蓝色。只替代该范围，键盘要求保留，不先做已过时的红色操作。"
        case .conflict: "两条布局说明不能直接同时适用。回答关联的冲突问题，只形成局部决定，独立文档仍可继续。"
        }
    }
}

enum TCReviewMember: String, CaseIterable, Identifiable, Sendable {
    case coordinator, interface, verification, documentation, reflection, governance
    var id: String { rawValue }
    var title: String {
        switch self {
        case .coordinator: "主 Agent / 协调负责人（示例）"
        case .interface: "接口成员（示例）"
        case .verification: "契约核对成员（示例）"
        case .documentation: "文档成员（示例）"
        case .reflection: "反思者（示例）"
        case .governance: "治理者（示例）"
        }
    }
    var shortTitle: String {
        switch self {
        case .coordinator: "协调负责人"
        case .interface: "接口成员"
        case .verification: "契约核对"
        case .documentation: "独立文档"
        case .reflection: "反思者"
        case .governance: "治理者"
        }
    }
    var work: String {
        switch self {
        case .coordinator: "任务协调"
        case .interface: "共享接口"
        case .verification: "契约核对"
        case .documentation: "独立术语表"
        case .reflection, .governance: "只读监督"
        }
    }
    var isReadOnly: Bool { self == .reflection || self == .governance }
    var roleBoundary: String {
        isReadOnly
            ? "只读意见，不写代码、不执行 shell；定向消息不会赋予写入或管理员权限。监督 API 与主执行器绑定分离。"
            : "仅演示此工作包内的沟通；选择收件人不授新权限，不改变执行器、模型/API或监督职责。"
    }
}

enum TCReviewPurpose: String, CaseIterable, Identifiable, Sendable {
    case current, nextRound, answer
    var id: String { rawValue }
    var title: String {
        switch self {
        case .current: "补充当前工作"
        case .nextRound: "留到下一轮"
        case .answer: "回答原问题"
        }
    }
}

enum TCReviewSource: String, Sendable {
    case human, machine, relay
    var title: String {
        switch self {
        case .human: "人类输入（示例）"
        case .machine: "机器意见（示例，不是人类批准）"
        case .relay: "协调转达（示例，保留原来源）"
        }
    }
}

enum TCReviewScope: String, CaseIterable, Identifiable, Sendable {
    case color, keyboard, sharedInterface, layout
    var id: String { rawValue }
    var title: String {
        switch self {
        case .color: "颜色要求"
        case .keyboard: "键盘可达性"
        case .sharedInterface: "共享接口约定"
        case .layout: "布局范围"
        }
    }
}

enum TCReviewDelivery: Equatable, Sendable {
    case notHandedOff, handedOff, unknown
    var title: String {
        switch self {
        case .notHandedOff: "尚未交出（示例）"
        case .handedOff: "已交出（示例）"
        case .unknown: "交付未知 · 待核实"
        }
    }
}

enum TCReviewIntent: Equatable, Sendable {
    case pendingCurrent, waitingNextRound, followUpPending
    var title: String {
        switch self {
        case .pendingCurrent: "待安全边界投递意图"
        case .waitingNextRound: "下一轮已交办 · 排队"
        case .followUpPending: "后续待处理（示例，未证明交付）"
        }
    }
}

enum TCReviewOperation: Equatable, Sendable {
    case notOccurred, occurred, unknown
    var title: String {
        switch self {
        case .notOccurred: "未操作（示例）"
        case .occurred: "旧操作已发生（示例，不自动回滚）"
        case .unknown: "原操作未知 · 待核实，不重放"
        }
    }
}

struct TCReviewMessage: Equatable, Identifiable, Sendable {
    let id: Int
    var text: String
    let recipient: TCReviewMember
    let purpose: TCReviewPurpose
    let source: TCReviewSource
    let sourceDetail: String
    let questionID: String?
    let correctionOf: Int?
    let scope: TCReviewScope?
    var intent: TCReviewIntent
    var delivery: TCReviewDelivery
    var ownerConfirmed: Bool
    var consumed = false
    var operation: TCReviewOperation = .notOccurred
    var withdrawn = false
    var supersededBy: Int?
    var revision = 0
    var evidence: [String] = []

    // This sample limits edit/withdraw to ordinary controlled records.
    // Withdrawing answers/corrections needs a separately designed protocol.
    var canEditOrWithdraw: Bool {
        ownerConfirmed && delivery == .notHandedOff && !withdrawn
            && questionID == nil && correctionOf == nil && supersededBy == nil
    }
    var canCorrect: Bool {
        !withdrawn && questionID == nil && supersededBy == nil
    }
    var controlSummary: String {
        if withdrawn { return "已撤回（示例，原记录保留）" }
        return ownerConfirmed && delivery == .notHandedOff
            ? "确认仍由样例 owner 控制" : "不再确认可控 · 不能承诺编辑 / 撤回"
    }
    var relationSummary: String {
        var parts: [String] = []
        if let questionID { parts.append("答复原问题 \(questionID)") }
        if let correctionOf { parts.append("关联更正 M\(correctionOf)，不是回滚或 Stop") }
        if let supersededBy { parts.append("对应要求被 M\(supersededBy) 更正，原交付 / 操作事实保留") }
        if let scope { parts.append("局部范围：\(scope.title)") }
        return parts.isEmpty ? "普通补充，不覆盖其他要求" : parts.joined(separator: "\n")
    }
    var consumptionSummary: String {
        consumed ? "示例事件确认消费；采纳 / 整改 / 验收仍无证据" : "消费未知；发送不等于采纳"
    }
}

struct TCReviewQuestion: Equatable, Identifiable, Sendable {
    let id: String
    let asker: TCReviewMember
    let work: String
    let text: String
    let basis: String
    let recommendation: String
    let recommendationReason: String
    let impact: String
    let dependentMembers: [TCReviewMember]
    let important: Bool
    let scope: TCReviewScope?
    let conflictingMessages: [Int]
    var deferred = false
    var availableEvidence: String?
}

struct TCReviewAlignment: Equatable, Identifiable, Sendable {
    let correctionID: Int
    let member: TCReviewMember
    var handedOff = false
    var consumed = false
    var id: String { "\(correctionID)-\(member.rawValue)" }
    var summary: String {
        let delivery = handedOff ? "示例已交出" : "需同步"
        let consumption = consumed ? "示例已消费，仍需核对实际影响" : "消费未知"
        return "\(delivery) · \(consumption)；不代表已按新版本完成"
    }
}

struct TCReviewConditions: Equatable, Sendable {
    var atBoundary = false
    var boundary = 0
    var usedBoundary = 0
    var authorized = true
    var approvalSatisfied = true
    var supervisionSatisfied = true
    var resourceAvailable = true
    var capabilityKnown = true
    var answerQuestionID: String?
    var operationUnknown = false
    var paused = false
    var stopRequested = false
    var stopVerified = false
    var authorizationRevoked = false
    var queueCancelled = false

    var controlBlockers: [String] {
        var reasons: [String] = []
        if stopVerified { reasons.append("已核实停止（示例）") }
        else if stopRequested { reasons.append("Stop 请求已记下，在途是否已停尚未核实") }
        if paused { reasons.append("用户暂停（示例）") }
        if authorizationRevoked { reasons.append("授权已撤回（示例）") }
        if queueCancelled { reasons.append("后续交办已取消（示例，保留记录）") }
        return reasons
    }
}

enum TCReviewDraftContext: Equatable, Sendable {
    case compose
    case reply(String)
    case edit(Int, revision: Int)
    case correction(Int)
}

struct TCReviewDraft: Equatable, Sendable {
    // Local UI lifetime guards only, not distributed identities or leases.
    let owner: UUID
    let token: Int
    var context: TCReviewDraftContext = .compose
    var text = ""
    var purpose: TCReviewPurpose = .current
    var recipient: TCReviewMember = .coordinator
}

enum TCReviewMessageEvent: String, CaseIterable, Identifiable, Sendable {
    case handedOff, deliveryUnknown, consumed, operationOccurred
    var id: String { rawValue }
    var title: String {
        switch self {
        case .handedOff: "示例已交给执行器"
        case .deliveryUnknown: "示例交付状态未知"
        case .consumed: "示例确认已消费"
        case .operationOccurred: "示例旧操作已发生"
        }
    }
}

enum TCReviewGateEvent: String, CaseIterable, Identifiable, Sendable {
    case roundEnded, resourceBlocked, resourceRestored, authorizationMissing, authorizationAvailable
    case approvalMissing, approvalAvailable, supervisionMissing, supervisionAvailable
    case capabilityUnknown, capabilityKnown, waitForAnswer, operationUnknown, operationVerified
    case authorizationRevoked, queueCancelled, stopVerified
    var id: String { rawValue }
    var title: String {
        switch self {
        case .roundEnded: "示例本轮结束 / 新输入边界"
        case .resourceBlocked: "示例必需资源不足"
        case .resourceRestored: "示例获准资源恢复"
        case .authorizationMissing: "示例缺少授权"
        case .authorizationAvailable: "示例已有授权条件满足（不授真实权）"
        case .approvalMissing: "示例必需审批未齐"
        case .approvalAvailable: "示例必需审批证据齐备（不代批）"
        case .supervisionMissing: "示例必需监督未齐"
        case .supervisionAvailable: "示例必需监督证据齐备"
        case .capabilityUnknown: "示例接续能力不明"
        case .capabilityKnown: "示例可在边界接收"
        case .waitForAnswer: "示例等待关联问题的答案"
        case .operationUnknown: "示例原操作状态未知"
        case .operationVerified: "示例原操作已核实（不重放）"
        case .authorizationRevoked: "示例授权已撤回"
        case .queueCancelled: "示例后续交办已取消"
        case .stopVerified: "示例已核实停止"
        }
    }
}

struct TCReviewRecord: Equatable, Identifiable, Sendable {
    let id: Int
    let text: String
    let isExampleEvent: Bool
}

enum TCReviewColorScheme: String, CaseIterable, Identifiable, Sendable {
    case system, light, dark
    var id: String { rawValue }
    var title: String {
        switch self { case .system: "跟随系统"; case .light: "浅色"; case .dark: "深色" }
    }
}

struct TCReviewAppearance: Equatable, Sendable {
    var surface: SurfacePreference = .glass
    var colorScheme: TCReviewColorScheme = .system
    var advanced = false
    var reduceTransparency = false
    var increaseContrast = false
    var reduceMotion = false
}

struct TCReviewAccessibility: Equatable {
    let surface: SurfacePreference
    let reduceTransparency: Bool
    let increaseContrast: Bool
    let reduceMotion: Bool

    init(appearance: TCReviewAppearance, systemTransparency: Bool, systemContrast: Bool, systemMotion: Bool) {
        reduceTransparency = systemTransparency || appearance.reduceTransparency
        increaseContrast = systemContrast || appearance.increaseContrast
        reduceMotion = systemMotion || appearance.reduceMotion
        surface = SurfaceChoice.resolve(preference: appearance.surface, reduceTransparency: reduceTransparency)
    }
}
#endif
