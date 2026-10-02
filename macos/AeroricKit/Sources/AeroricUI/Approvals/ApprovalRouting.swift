import Foundation

/// P5-04 Action 2：审批路由的**纯决策**（无 UI）。两个请求同时等待时
/// 不得串题：路由键是 eventId（不是队列位置也不是 sessionId）。
public enum ApprovalRouting {
    /// 从 waterfall frame（emit payload）解析一个待处理请求。sessionId
    /// 先行回退 agentId（与 core `waterfall_session_id` 同语义）。
    public static func request(
        fromFrame frame: [String: Any],
        kind: PendingRequestStore.Kind
    ) -> PendingRequestStore.PendingRequest? {
        guard let eventId = frame["eventId"] as? String, !eventId.isEmpty else {
            return nil
        }
        var request = PendingRequestStore.PendingRequest(
            id: eventId,
            kind: kind,
            clientId: frame["clientId"] as? String,
            sessionId: (frame["sessionId"] as? String) ?? (frame["agentId"] as? String)
        )
        if kind == .approval {
            request.toolName = frame["toolName"] as? String
            request.callId = frame["callId"] as? String
            request.reason = frame["reason"] as? String
        } else {
            request.questions = (frame["questions"] as? [[String: Any]] ?? []).map(JSONBox.init)
        }
        return request
    }

    /// 串题守卫：提交回执时按 eventId 精确路由（不是按队列首）。
    /// 返回请求是否在簿记里（不在＝已 resolved，提交应拒绝）。
    public static func canSubmit(
        eventId: String,
        pendingIDs: [String]
    ) -> Bool {
        pendingIDs.contains(eventId)
    }
}
