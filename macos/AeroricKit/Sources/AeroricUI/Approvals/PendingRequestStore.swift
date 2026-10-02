import Foundation
import Observation

/// Sendable JSON 容器（questions/answers 原始字典；[String: Any] 非
/// Sendable，包一层 @unchecked 是安全的：内容是 JSON 序列化产物）。
public struct JSONBox: @unchecked Sendable, Equatable {
    public let value: [String: Any]

    public init(_ value: [String: Any]) {
        self.value = value
    }

    public static func == (lhs: JSONBox, rhs: JSONBox) -> Bool {
        // JSON 序列化后逐字节比较（Any 字典无法直接 ==）。
        guard let a = try? JSONSerialization.data(withJSONObject: lhs.value),
              let b = try? JSONSerialization.data(withJSONObject: rhs.value)
        else { return false }
        return a == b
    }
}

/// P5-04 Action 2：审批/问题请求的**权威簿记**。每个 request 按 eventId
/// 去重（React `useDshHostEvents` 同款：requested 追加去重、resolved 精确
/// 清除）；approve/deny/cancel 只是**提交**回执——拒绝后的最终状态由 host
/// 确认（resolved 事件到达才清除），本地不抹掉就算处理。
@MainActor
@Observable
public final class PendingRequestStore {
    public enum Kind: String, Equatable, Sendable {
        case approval
        case question
    }

    /// 一个待处理请求（形状与 DSH waterfall frame 对齐：eventId/clientId/
    /// sessionId 先行回退 agentId）。
    public struct PendingRequest: Identifiable, Equatable, Sendable {
        public let id: String
        public var kind: Kind
        public var clientId: String?
        public var sessionId: String?
        /// 审批：toolName/callId/reason；问题：questions 数组（原始 JSON）。
        public var toolName: String?
        public var callId: String?
        public var reason: String?
        /// 问题请求的 questions（[{id, question, options...}] 原始字典）。
        public var questions: [JSONBox] = []

        public static func == (lhs: PendingRequest, rhs: PendingRequest) -> Bool {
            lhs.id == rhs.id && lhs.kind == rhs.kind && lhs.toolName == rhs.toolName
                && lhs.reason == rhs.reason && lhs.sessionId == rhs.sessionId
        }
    }

    /// 回执（与 DSH waterfall outcome 形状对齐；值即结果本身，无信封）。
    public enum Outcome: Equatable, Sendable {
        case approvalAllowedOnce
        case approvalRejected
        case answers([JSONBox])
        case next
    }

    public private(set) var pending: [PendingRequest] = []
    /// 已提交回执但 host 尚未 resolved 的 eventId（提交后 UI 禁重复点击；
    /// resolved 清除，断线/host restart 时由 fetchAndClear 恢复）。
    public private(set) var submittedEventIDs: Set<String> = []

    public init() {}

    // MARK: - 事件入口（requested 追加去重 / resolved 精确清除）

    /// approval/requested 或 question/requested：按 eventId 去重追加。
    public func upsert(_ request: PendingRequest) {
        if let index = pending.firstIndex(where: { $0.id == request.id }) {
            pending[index] = request
        } else {
            pending.append(request)
        }
    }

    /// approval/resolved 或 question/resolved：按 eventId **精确**清除
    /// （不是队首弹出——多请求并发时精确清除才不串题）。
    public func resolve(eventId: String) {
        pending.removeAll { $0.id == eventId }
        submittedEventIDs.remove(eventId)
    }

    /// 用户点 approve/deny/cancel：只记录已提交（host 确认前不抹条目）。
    public func markSubmitted(eventId: String) {
        submittedEventIDs.insert(eventId)
    }

    public func isSubmitted(eventId: String) -> Bool {
        submittedEventIDs.contains(eventId)
    }

    /// host restart/断线恢复：本地簿记全部失效（权威面在 host）。
    public func fetchAndClear() {
        pending.removeAll()
        submittedEventIDs.removeAll()
    }

    // MARK: - 回执构造（Action 2：approve/deny/cancel 明确三分）

    /// 构造 DSH waterfall outcome（值即结果本身，无 {ok,value} 信封）。
    public static func outcomePayload(_ outcome: Outcome) -> [String: Any] {
        switch outcome {
        case .approvalAllowedOnce:
            return ["kind": "result", "value": "allowed-once"]
        case .approvalRejected:
            return ["kind": "result", "value": "rejected"]
        case let .answers(answers):
            return ["kind": "result", "value": ["answers": answers.map(\.value)]]
        case .next:
            return ["kind": "next"]
        }
    }
}
