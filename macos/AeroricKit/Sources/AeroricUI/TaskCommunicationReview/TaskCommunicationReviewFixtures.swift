#if DEBUG
import Foundation

struct TCReviewFixture {
    var messages: [TCReviewMessage] = []
    var questions: [TCReviewQuestion] = []
    var conditions: [TCReviewMember: TCReviewConditions] = [:]
}

enum TaskCommunicationReviewFixtures {
    static let offlineNotice = "离线沟通示例 · 全部为虚构数据 · 不连接 Agent / API / PTY / host"
    static let bindings = "主执行器 A / 模型 A（示例）；反思 API R、治理 API G（独立安全别名）。四家族只是适配目标，未认证任何真实通道。"
    static let permissionBoundary = "权限审批不在此代答。Full Access 覆盖内免重复索权，不代答需求歧义，也不免新技能人工同意；消息序号和外观切换不授新权。"
    static let acceptanceBoundary = "未启动真实执行器；发送、交付、消费、采纳、技术核验和用户验收相互独立。本页不提供真实通过回执。"

    static func make(_ scenario: TCReviewScenario) -> TCReviewFixture {
        var fixture = TCReviewFixture(
            questions: [interfaceQuestion, documentationQuestion],
            conditions: Dictionary(uniqueKeysWithValues: TCReviewMember.allCases.map { ($0, TCReviewConditions()) })
        )
        switch scenario {
        case .input:
            fixture.conditions[.coordinator]?.capabilityKnown = false
        case .editing:
            fixture.messages = [
                message(1, "补充：请保留现有错误提示，先核对再改。")
            ]
        case .delivery:
            fixture.messages = [
                message(1, "颜色要求：红色提示。", delivery: .handedOff, scope: .color),
                message(2, "布局要求：紧凑侧栏。", delivery: .unknown, scope: .layout, operation: .unknown)
            ]
        case .targeting:
            fixture.messages = [
                message(1, "共享接口暂用 oldKey 字段；变更需局部核对。",
                        recipient: .interface, delivery: .handedOff, scope: .sharedInterface),
                message(2, "建议检查接口一致性；此意见不是用户批准。",
                        source: .machine, sourceDetail: "反思者的只读意见（示例）", delivery: .handedOff),
                message(3, "转达接口成员的风险说明；请核对实际影响。",
                        recipient: .verification, source: .relay,
                        sourceDetail: "原来源：接口成员意见；协调负责人转达（示例，不是人类批准）",
                        delivery: .handedOff)
            ]
        case .questions:
            break
        case .continuation:
            fixture.messages = [
                message(1, "先补充错误处理说明。", purpose: .nextRound),
                message(2, "再核对接口回退边界。", purpose: .nextRound),
                message(3, "独立整理术语表，无须等待接口答案。", recipient: .documentation)
            ]
        case .requirements:
            fixture.messages = [
                message(1, "按钮提示色：红色。", purpose: .nextRound, scope: .color),
                message(2, "保留键盘可达性说明。", purpose: .nextRound, scope: .keyboard)
            ]
        case .conflict:
            fixture.messages = [
                message(1, "侧栏固定为紧凑列。", delivery: .handedOff, scope: .layout),
                message(2, "侧栏应占满可用区域；适用范围尚不明确。", scope: .layout)
            ]
            fixture.questions = [conflictQuestion, documentationQuestion]
            fixture.conditions[.coordinator]?.answerQuestionID = conflictQuestion.id
        }
        return fixture
    }

    static func affectedMembers(for scope: TCReviewScope?) -> [TCReviewMember] {
        scope == .sharedInterface ? [.coordinator, .interface, .verification] : []
    }

    private static func message(
        _ id: Int, _ text: String,
        recipient: TCReviewMember = .coordinator,
        purpose: TCReviewPurpose = .current,
        source: TCReviewSource = .human,
        sourceDetail: String = "虚构用户交办",
        delivery: TCReviewDelivery = .notHandedOff,
        scope: TCReviewScope? = nil,
        operation: TCReviewOperation = .notOccurred
    ) -> TCReviewMessage {
        TCReviewMessage(
            id: id, text: text, recipient: recipient, purpose: purpose,
            source: source, sourceDetail: sourceDetail, questionID: nil, correctionOf: nil, scope: scope,
            intent: purpose == .nextRound ? .waitingNextRound : .pendingCurrent,
            delivery: delivery, ownerConfirmed: delivery == .notHandedOff,
            operation: operation,
            evidence: ["载入初始虚构 fixture；交付 / 操作状态仅是样例给定事实"]
        )
    }

    private static var interfaceQuestion: TCReviewQuestion {
        TCReviewQuestion(
            id: "q-interface", asker: .interface, work: "共享接口与契约核对",
            text: "错误响应是否统一为 code / message 两个字段？",
            basis: "现有两个虚构调用方的字段约定不一致，影响共享边界。",
            recommendation: "统一字段，并分别核对两个受影响调用方。",
            recommendationReason: "减少歧义，但必须由用户明确回答，不默认采纳推荐。",
            impact: "接口调整与相关契约核对等待；独立术语表仍可继续。",
            dependentMembers: [.interface, .verification], important: true,
            scope: .sharedInterface, conflictingMessages: []
        )
    }

    private static var documentationQuestion: TCReviewQuestion {
        TCReviewQuestion(
            id: "q-documentation", asker: .documentation, work: "文档呈现",
            text: "最终说明采用一页摘要还是分章目录？",
            basis: "仅影响最终说明的组织方式。",
            recommendation: "先给一页摘要，详细内容保留可展开章节。",
            recommendationReason: "方便阅读；这是建议，不是默认答案。",
            impact: "最终排版待答；术语整理与其他安全独立工作继续。",
            dependentMembers: [], important: false, scope: nil, conflictingMessages: []
        )
    }

    private static var conflictQuestion: TCReviewQuestion {
        TCReviewQuestion(
            id: "q-layout-conflict", asker: .coordinator, work: "布局范围澄清",
            text: "M1 的紧凑侧栏与 M2 的占满可用区域，分别适用于什么范围？",
            basis: "两条原文没有区分窄窗与宽窗，不能直接同时适用。",
            recommendation: "明确宽窗为紧凑列、窄窗为单列阅读面。",
            recommendationReason: "可能兼顾两种需求；只是机器整理的建议，未回答不视为同意。",
            impact: "只等待布局相关部分，独立术语表继续；既有操作事实不撤销。",
            dependentMembers: [.coordinator], important: true,
            scope: .layout, conflictingMessages: [1, 2]
        )
    }
}
#endif
