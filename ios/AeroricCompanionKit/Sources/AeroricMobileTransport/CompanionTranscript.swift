import Foundation

/// D4 (2026-11-23): one line of a message as the phone shows it.
public struct CompanionMessageLine: Equatable, Sendable {
    public enum Kind: String, Equatable, Sendable {
        case text, thinking, tool, toolResult, failedToolResult, attachment
    }

    public var kind: Kind
    public var text: String
    /// The Mac cut this block (`session.messages` sends at most 4000
    /// characters per block, no attachment data).
    public var truncated: Bool

    public init(kind: Kind, text: String, truncated: Bool = false) {
        self.kind = kind
        self.text = text
        self.truncated = truncated
    }
}

public struct CompanionMessage: Identifiable, Equatable, Sendable {
    public var id: String
    /// `user` or `assistant`, as the transcript says.
    public var role: String
    public var lines: [CompanionMessageLine]

    public init(id: String, role: String, lines: [CompanionMessageLine]) {
        self.id = id
        self.role = role
        self.lines = lines
    }
}

/// The latest messages of one task's session, read from the Mac.
public struct CompanionTranscript: Equatable, Sendable {
    public var taskId: String
    public var messages: [CompanionMessage]
    /// Messages in the transcript, and how many older ones are not shown.
    public var total: Int
    public var omitted: Int

    public init(taskId: String, messages: [CompanionMessage], total: Int, omitted: Int) {
        self.taskId = taskId
        self.messages = messages
        self.total = total
        self.omitted = omitted
    }
}

/// What the task page shows for the session.
public enum CompanionTranscriptState: Equatable, Sendable {
    case closed
    case loading(taskId: String)
    /// `refreshing`: a newer read is on its way (the task changed).
    case shown(CompanionTranscript, refreshing: Bool)
    /// The Mac answered that there is nothing to show, and why.
    case unavailable(taskId: String, reason: String)
    /// The read did not get an answer.
    case failed(taskId: String, message: String)

    public var taskId: String? {
        switch self {
        case .closed: nil
        case let .loading(taskId), let .unavailable(taskId, _), let .failed(taskId, _): taskId
        case let .shown(transcript, _): transcript.taskId
        }
    }

    /// Reads the Mac's `session.messages` answer
    /// (`remote_headless.rs`, `phone_transcript`).
    public static func reading(taskId: String, answer: String) -> CompanionTranscriptState {
        guard let data = answer.data(using: .utf8),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return .failed(taskId: taskId, message: "Mac 的回答读不懂")
        }
        guard object["available"] as? Bool == true else {
            return .unavailable(taskId: taskId,
                                reason: unavailableReason(object["reason"] as? String, detail: object["detail"] as? String))
        }
        let items = object["messages"] as? [[String: Any]] ?? []
        let messages = items.enumerated().map { index, item in
            CompanionMessage(
                id: (item["messageId"] as? String) ?? "#\(index)",
                role: (item["role"] as? String) ?? "",
                lines: (item["content"] as? [[String: Any]] ?? []).compactMap(line))
        }
        let total = (object["total"] as? Int) ?? messages.count
        let omitted = (object["omitted"] as? Int) ?? max(0, total - messages.count)
        return .shown(CompanionTranscript(taskId: taskId, messages: messages, total: total, omitted: omitted),
                      refreshing: false)
    }

    static func unavailableReason(_ reason: String?, detail: String?) -> String {
        switch reason {
        case "no_session": "这个任务还没有会话记录"
        case "unreadable": "会话记录读不出来" + (detail.map { "：\($0)" } ?? "")
        case "ssh": "这个项目在远程主机（SSH）上，手机还看不了它的会话"
        case "wsl": "这个项目在 WSL 里，手机还看不了它的会话"
        case let other?: "Mac 没有给出会话：\(other)"
        case nil: "Mac 没有给出会话"
        }
    }

    private static func line(_ block: [String: Any]) -> CompanionMessageLine? {
        let truncated = block["truncated"] as? Bool ?? false
        switch block["type"] as? String {
        case "text":
            return CompanionMessageLine(kind: .text, text: block["text"] as? String ?? "", truncated: truncated)
        case "thinking":
            return CompanionMessageLine(kind: .thinking, text: block["thinking"] as? String ?? "", truncated: truncated)
        case "tool_use":
            let name = block["name"] as? String ?? "工具"
            let input = block["input"] as? String ?? ""
            return CompanionMessageLine(kind: .tool, text: input.isEmpty ? name : "\(name) \(input)",
                                        truncated: truncated)
        case "tool_result":
            let failed = block["failed"] as? Bool ?? false
            return CompanionMessageLine(kind: failed ? .failedToolResult : .toolResult,
                                        text: block["output"] as? String ?? "", truncated: truncated)
        case "attachment":
            return CompanionMessageLine(kind: .attachment, text: block["name"] as? String ?? "附件", truncated: truncated)
        default:
            return nil
        }
    }
}
