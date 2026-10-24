import Foundation

/// P5-04 Action 5：会话操作（export/search/resume/adopt/handoff 的纯
/// 决策面；跨协议动作显示新会话提示）。
public enum SessionActions {
    /// 跨协议 resume/adopt 是否需要「新会话」提示（旧历史仍能打开）。
    /// 判定：目标 agent family 与会话 transcript 格式不一致 → 需要提示。
    public static func requiresNewSessionHint(
        currentFamily: String,
        targetAgent: String
    ) -> Bool {
        let targetFamily: String
        switch targetAgent {
        case "dsh":
            targetFamily = "dsh"
        case "omp":
            targetFamily = "omp"
        case "codex":
            targetFamily = "codex"
        default:
            targetFamily = "claude"
        }
        return currentFamily != targetFamily
    }

    /// 搜索结果过滤（session.search 的客户端镜像；query 空白＝不过滤）。
    public static func matchesQuery(_ message: SessionMessage, query: String) -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return true }
        return message.content.contains { block in
            switch block {
            case let .text(text):
                return text.localizedCaseInsensitiveContains(trimmed)
            case let .toolUse(_, name, input):
                return name.localizedCaseInsensitiveContains(trimmed)
                    || input.localizedCaseInsensitiveContains(trimmed)
            case let .toolResult(_, output):
                return output.localizedCaseInsensitiveContains(trimmed)
            case let .thinking(thinking):
                return thinking.localizedCaseInsensitiveContains(trimmed)
            case let .attachment(name, _, _):
                return name.localizedCaseInsensitiveContains(trimmed)
            }
        }
    }
}
