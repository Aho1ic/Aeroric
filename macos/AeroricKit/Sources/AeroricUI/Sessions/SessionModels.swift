import Foundation

/// P5-04 Action 1/4：会话读取的纯逻辑。消息形状与 core `sessions/mod.rs`
/// 的 `SessionMessage`/`SessionContent`（serde tag="type" snake_case）对齐；
/// 本文件是纯模型＋纯函数（分页合并/批量合并/去重），无 UI、无副作用。
public struct SessionMessage: Equatable, Sendable {
    public var role: String
    public var content: [SessionContent]
    public var messageId: String?

    public init(role: String, content: [SessionContent], messageId: String? = nil) {
        self.role = role
        self.content = content
        self.messageId = messageId
    }
}

/// 内容块。变体与字段名即旧客户端 JSON 的 `type`／字段，勿改（与 core
/// serde 形状一一对应）。
public enum SessionContent: Equatable, Sendable {
    case text(text: String)
    case toolUse(id: String, name: String, input: String)
    case toolResult(id: String, output: String)
    case thinking(thinking: String)
    case attachment(name: String, mediaType: String, source: String)

    /// 稳定 ID（Action 4：稳定 ID diff 更新的键）。tool 块用 id；文本块
    /// 用内容散列（跨页去重）。
    public var stableID: String {
        switch self {
        case let .toolUse(id, _, _), let .toolResult(id, _):
            return id
        case let .text(text):
            return "text:\(text)"
        case let .thinking(thinking):
            return "thinking:\(thinking)"
        case let .attachment(name, mediaType, _):
            return "attachment:\(mediaType):\(name)"
        }
    }
}

/// 一页历史（core `SessionMessagePage` 的镜像：messages/nextCursor/hasMore）。
public struct SessionMessagePage: Equatable, Sendable {
    public var messages: [SessionMessage]
    public var nextCursor: UInt64?
    public var hasMore: Bool

    public init(messages: [SessionMessage], nextCursor: UInt64?, hasMore: Bool) {
        self.messages = messages
        self.nextCursor = nextCursor
        self.hasMore = hasMore
    }
}

// MARK: - 纯函数（Action 4：稳定 ID diff、跨页合并去重）

public enum SessionHistoryLogic {
    /// 跨页合并（React `mergeSessionMessagePages` 的 Swift 对齐）：earlier
    /// 是已加载的更早页（时间正序），later 是本轮新读的页。Codex 重复事件
    /// 文本去重＝later 首条文本命中 earlier 尾部文本集合即丢弃。
    public static func mergePages(
        _ earlier: [SessionMessage],
        _ later: [SessionMessage],
        isCodex: Bool
    ) -> [SessionMessage] {
        guard !later.isEmpty else { return earlier }
        guard !earlier.isEmpty else { return later }
        if isCodex {
            let boundary = Set(
                earlier.suffix(8).flatMap { message in
                    message.content.compactMap { block -> String? in
                        if case let .text(text) = block { return text }
                        return nil
                    }
                }
            )
            var trimmed = later
            while !trimmed.isEmpty, firstIsDuplicateText(trimmed[0], in: boundary) {
                trimmed.removeFirst()
            }
            return earlier + trimmed
        }
        return earlier + later
    }

    /// 单条消息是否与去重集合命中（Codex 重复事件文本）。
    private static func firstIsDuplicateText(
        _ message: SessionMessage,
        in boundary: Set<String>
    ) -> Bool {
        let texts = message.content.compactMap { block -> String? in
            if case let .text(text) = block { return text }
            return nil
        }
        return !texts.isEmpty && texts.allSatisfy(boundary.contains)
    }

    /// 同一列表内的 tool_result 稳定 ID 去重（Action 4：重复 tool_end 不
    /// 重复 row/usage）。按 content.stableID 全局去重；无 ID 的块保留。
    public static func deduplicateToolBlocks(
        _ messages: [SessionMessage]
    ) -> [SessionMessage] {
        var seenToolIDs = Set<String>()
        return messages.map { message in
            var content: [SessionContent] = []
            for block in message.content {
                switch block {
                case .toolResult:
                    let id = block.stableID
                    if seenToolIDs.contains(id) { continue }
                    seenToolIDs.insert(id)
                    content.append(block)
                default:
                    content.append(block)
                }
            }
            var copy = message
            copy.content = content
            return copy
        }
    }

    /// 分页游标守卫（React 同款：nextCursor >= cursor 是坏游标）。
    public static func isValidCursorAdvance(cursor: UInt64?, next: UInt64?) -> Bool {
        guard let cursor, let next else { return true }
        return next < cursor
    }
}

// MARK: - 用量（UsagePanel 数据源；字段名与旧客户端 SessionMetrics 对齐）

public struct SessionUsage: Equatable, Sendable {
    public var durationSecs: Double?
    public var totalTokens: UInt64?
    public var contextTokens: UInt64?

    public init(durationSecs: Double? = nil, totalTokens: UInt64? = nil, contextTokens: UInt64? = nil) {
        self.durationSecs = durationSecs
        self.totalTokens = totalTokens
        self.contextTokens = contextTokens
    }
}

// MARK: - 未知事件保留（Action 4：未知 tool/content 项及 source ID 保留）

/// 解码一批 JSONL 消息行（Swift 侧宽容镜像：未知 type 保留为 raw 块）。
public enum SessionMessageDecoder {
    /// 解码 core 风格的 SessionMessage JSON（tag="type" snake_case content）。
    /// 单行解码失败**不使整个会话解码失败**（返回 nil 跳过该行）。
    public static func decodeMessage(fromLine line: String) -> SessionMessage? {
        guard let data = line.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        let role = object["role"] as? String ?? "assistant"
        let messageId = object["messageId"] as? String
        var content: [SessionContent] = []
        if let array = object["content"] as? [[String: Any]] {
            for item in array {
                if let block = decodeContent(item) {
                    content.append(block)
                }
            }
        } else if let text = object["content"] as? String {
            content.append(.text(text: text))
        }
        return SessionMessage(role: role, content: content, messageId: messageId)
    }

    /// 内容块解码：已知 type 精确取字段；未知 type 保留原始字段（raw 块）。
    static func decodeContent(_ item: [String: Any]) -> SessionContent? {
        switch item["type"] as? String {
        case "text":
            guard let text = item["text"] as? String else { return nil }
            return .text(text: text)
        case "tool_use":
            guard let id = item["id"] as? String else { return nil }
            let name = item["name"] as? String ?? "tool"
            let input: String
            if let string = item["input"] as? String {
                input = string
            } else if let value = item["input"], let serialized = try? JSONSerialization.data(withJSONObject: value) {
                input = String(data: serialized, encoding: .utf8) ?? "{}"
            } else {
                input = "{}"
            }
            return .toolUse(id: id, name: name, input: input)
        case "tool_result":
            guard let id = item["id"] as? String else { return nil }
            let output: String
            if let string = item["output"] as? String {
                output = string
            } else if let value = item["output"], let serialized = try? JSONSerialization.data(withJSONObject: value) {
                output = String(data: serialized, encoding: .utf8) ?? ""
            } else {
                output = ""
            }
            return .toolResult(id: id, output: output)
        case "thinking":
            guard let thinking = item["thinking"] as? String else { return nil }
            return .thinking(thinking: thinking)
        case "attachment":
            guard let name = item["name"] as? String else { return nil }
            return .attachment(
                name: name,
                mediaType: item["mediaType"] as? String ?? "",
                source: item["source"] as? String ?? ""
            )
        default:
            // 未知 type：保留 source ID 与原始形状（raw 块，按 text 渲染）。
            let raw = (try? JSONSerialization.data(withJSONObject: item))
                .flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
            return .text(text: raw)
        }
    }
}
