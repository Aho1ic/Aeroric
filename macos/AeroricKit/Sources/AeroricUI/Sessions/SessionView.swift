import SwiftUI

/// P5-04 Action 1/4：会话视图（原生消息面板）。消息列表＋分页加载＋
/// 会话操作（export/copy）；分页不跳动、未知事件不使解码失败。
public struct SessionView: View {
    let paginator: SessionPaginator
    var onLoadMore: (() -> Void)?
    var onExport: (() -> Void)?
    var usage: SessionUsage?

    public init(
        paginator: SessionPaginator,
        onLoadMore: (() -> Void)? = nil,
        onExport: (() -> Void)? = nil,
        usage: SessionUsage? = nil
    ) {
        self.paginator = paginator
        self.onLoadMore = onLoadMore
        self.onExport = onExport
        self.usage = usage
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                if let onExport {
                    Button("导出 Markdown") { onExport() }
                        .accessibilityIdentifier("session.export")
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 8) {
                    if paginator.canLoadMore {
                        Button("加载更早消息") { onLoadMore?() }
                            .accessibilityIdentifier("session.load-more")
                    }
                    ForEach(Array(paginator.messages.enumerated()), id: \.offset) { index, message in
                        MessageRow(message: message)
                            .accessibilityIdentifier("session.message-row.\(index)")
                    }
                }
                .padding(12)
            }
            .accessibilityIdentifier("session.messages")
            if let usage {
                UsagePanel(usage: usage)
            }
        }
        .accessibilityIdentifier("session.view")
    }
}

/// P5-04 Action 1：用量面板（SessionMetrics 字段；无数据不显示假 0）。
public struct UsagePanel: View {
    let usage: SessionUsage

    public init(usage: SessionUsage) {
        self.usage = usage
    }

    public var body: some View {
        HStack(spacing: 16) {
            if let duration = usage.durationSecs {
                LabeledContent("耗时") { Text(String(format: "%.1fs", duration)) }
            }
            if let total = usage.totalTokens {
                LabeledContent("Token") { Text("\(total)") }
            }
            if let context = usage.contextTokens {
                LabeledContent("上下文") { Text("\(context)") }
            }
            Spacer()
        }
        .font(.caption)
        .foregroundStyle(Color.dsTextSecondary)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Color.dsSurface.opacity(0.55))
        .accessibilityIdentifier("session.usage")
    }
}

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
