import CoreGraphics
import Foundation
import AeroricProtocol

/// v1 protocol message → UI message. Tool calls, results and attachments
/// arrive as opaque blocks claimed by `kind`; anything unclaimed is shown as
/// its kind rather than dropped (INV-12: no guessing from free text).
public enum SessionBlockMapping {
    public static func message(_ message: AeroricProtocol.SessionMessage) -> SessionMessage {
        SessionMessage(role: message.role.rawValue, content: message.blocks.map(content),
                       messageId: message.id)
    }

    public static func content(_ block: AeroricProtocol.ContentBlock) -> SessionContent {
        switch block {
        case let .text(kind, text):
            return kind == "thinking" ? .thinking(thinking: text) : .text(text: text)
        case let .unknown(opaque):
            let payload = opaque.payload.fields
            switch opaque.kind {
            case "tool_use":
                return .toolUse(id: opaque.sourceId ?? "", name: payload["name"]?.stringValue ?? "tool",
                                input: payload["input"].map(jsonText) ?? "{}")
            case "tool_result":
                return .toolResult(id: opaque.sourceId ?? "", output: payload["output"].map(jsonText) ?? "")
            case "attachment":
                return .attachment(name: payload["name"]?.stringValue ?? "附件",
                                   mediaType: payload["mediaType"]?.stringValue ?? "", source: "")
            default:
                return .text(text: "（未识别的内容：\(opaque.kind)）")
            }
        }
    }

    /// Strings stay verbatim; structured values are re-serialised as pretty JSON.
    static func jsonText(_ value: JSONValue) -> String {
        if let text = value.stringValue { return text }
        guard let data = try? JSONSerialization.data(withJSONObject: value.foundationValue,
                                                     options: [.prettyPrinted, .sortedKeys, .fragmentsAllowed]),
              let text = String(data: data, encoding: .utf8) else { return "" }
        return text
    }
}

private extension JSONValue {
    var foundationValue: Any {
        switch self {
        case .null: NSNull()
        case let .bool(value): value
        case let .number(text): Int(text).map { $0 as Any } ?? Double(text).map { $0 as Any } ?? text
        case let .string(value): value
        case let .array(values): values.map(\.foundationValue)
        case let .object(fields): fields.mapValues(\.foundationValue)
        }
    }
}

// MARK: - Tool calls

public enum ToolCallState: Equatable, Sendable {
    case running
    case done
    /// No result arrived and the task is no longer running.
    case unfinished

    public var activity: ActivityState {
        switch self {
        case .running: .running
        case .done: .done
        case .unfinished: .idle
        }
    }
}

public struct ToolCall: Identifiable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var input: String
    public var output: String?
    public var state: ToolCallState

    public var symbol: String { ToolCallPresentation.symbol(for: name) }
    public var summary: String { ToolCallPresentation.summary(input: input) }
    public var stats: ToolCallPresentation.LineStats? { ToolCallPresentation.stats(name: name, input: input) }
}

public enum ToolCallPresentation {
    public struct LineStats: Equatable, Sendable {
        public var added: Int
        public var removed: Int

        public init(added: Int, removed: Int) {
            self.added = added
            self.removed = removed
        }
    }

    public static func symbol(for name: String) -> String {
        switch name.lowercased() {
        case "read", "notebookread": "doc.text"
        case "edit", "multiedit", "notebookedit": "pencil"
        case "write": "square.and.pencil"
        case "bash", "shell", "exec_command": "terminal"
        case "grep", "search": "magnifyingglass"
        case "glob", "ls": "folder"
        case "webfetch", "websearch", "web_search": "globe"
        case "task", "agent": "person.2"
        case "todowrite", "update_plan": "checklist"
        default: "wrench.and.screwdriver"
        }
    }

    /// The most telling argument (path, command, pattern…), one line.
    public static func summary(input: String) -> String {
        let fields = jsonObject(input)
        for key in ["file_path", "path", "notebook_path", "command", "cmd", "pattern", "query", "url",
                    "description", "prompt"] {
            if let value = fields?[key] as? String, !value.isEmpty { return firstLine(value) }
            if let parts = fields?[key] as? [String], !parts.isEmpty { return firstLine(parts.joined(separator: " ")) }
        }
        return fields == nil ? firstLine(input) : ""
    }

    /// Only computed where the arguments say it exactly; otherwise nil.
    public static func stats(name: String, input: String) -> LineStats? {
        guard let fields = jsonObject(input) else { return nil }
        switch name.lowercased() {
        case "edit":
            guard let old = fields["old_string"] as? String, let new = fields["new_string"] as? String else { return nil }
            return LineStats(added: lineCount(new), removed: lineCount(old))
        case "multiedit":
            guard let edits = fields["edits"] as? [[String: Any]], !edits.isEmpty else { return nil }
            return edits.reduce(LineStats(added: 0, removed: 0)) { total, edit in
                LineStats(added: total.added + lineCount(edit["new_string"] as? String ?? ""),
                          removed: total.removed + lineCount(edit["old_string"] as? String ?? ""))
            }
        case "write":
            guard let content = fields["content"] as? String else { return nil }
            return LineStats(added: lineCount(content), removed: 0)
        default:
            return nil
        }
    }

    static func lineCount(_ text: String) -> Int {
        text.isEmpty ? 0 : text.split(separator: "\n", omittingEmptySubsequences: false).count
            - (text.hasSuffix("\n") ? 1 : 0)
    }

    static func firstLine(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        return String(line.trimmingCharacters(in: .whitespaces).prefix(120))
    }

    static func jsonObject(_ text: String) -> [String: Any]? {
        guard let data = text.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }
}

// MARK: - Transcript items

public enum TranscriptItem: Identifiable, Equatable, Sendable {
    /// A user or assistant turn's prose (text, thinking, attachments).
    case message(id: String, role: String, content: [SessionContent])
    case tool(ToolCall)
    /// Three or more consecutive finished tool calls, folded into one line.
    case toolGroup(id: String, calls: [ToolCall])

    public var id: String {
        switch self {
        case let .message(id, _, _): "message:\(id)"
        case let .tool(call): "tool:\(call.id)"
        case let .toolGroup(id, _): "group:\(id)"
        }
    }
}

public enum TranscriptBuilder {
    public static let groupThreshold = 3

    /// Pairs tool_use with tool_result by id. Result-only messages are absorbed
    /// into their call instead of rendering as empty user bubbles.
    public static func items(from messages: [SessionMessage], taskRunning: Bool) -> [TranscriptItem] {
        var results: [String: String] = [:]
        var usedIDs = Set<String>()
        for message in messages {
            for case let .toolUse(id, _, _) in message.content { usedIDs.insert(id) }
            for case let .toolResult(id, output) in message.content { results[id] = output }
        }

        var items: [TranscriptItem] = []
        for (index, message) in messages.enumerated() {
            let messageID = message.messageId ?? "m\(index)"
            var prose: [SessionContent] = []
            func flushProse() {
                guard !prose.isEmpty else { return }
                items.append(.message(id: "\(messageID).\(items.count)", role: message.role, content: prose))
                prose = []
            }
            for block in message.content {
                switch block {
                case let .toolUse(id, name, input):
                    flushProse()
                    let output = results[id]
                    let state: ToolCallState = output != nil ? .done : (taskRunning ? .running : .unfinished)
                    items.append(.tool(ToolCall(id: id, name: name, input: input, output: output, state: state)))
                case let .toolResult(id, output):
                    // Orphan result (its call is on an older page): keep it visible.
                    if !usedIDs.contains(id) {
                        flushProse()
                        items.append(.tool(ToolCall(id: id, name: "结果", input: "", output: output, state: .done)))
                    }
                default:
                    prose.append(block)
                }
            }
            flushProse()
        }
        return grouped(items)
    }

    static func grouped(_ items: [TranscriptItem]) -> [TranscriptItem] {
        var output: [TranscriptItem] = []
        var run: [ToolCall] = []
        func flush() {
            if run.count >= groupThreshold {
                output.append(.toolGroup(id: run[0].id, calls: run))
            } else {
                output += run.map(TranscriptItem.tool)
            }
            run = []
        }
        for item in items {
            if case let .tool(call) = item, call.state == .done {
                run.append(call)
            } else {
                flush()
                output.append(item)
            }
        }
        flush()
        return output
    }
}

/// Conversation column: clamp(680, centre × 0.64, 880), never wider than the
/// centre minus a 16pt gutter each side.
public enum TranscriptGeometry {
    public static let minimum: CGFloat = 680
    public static let maximum: CGFloat = 880
    public static let ratio: CGFloat = 0.64
    public static let gutter: CGFloat = 16

    public static func columnWidth(for centreWidth: CGFloat) -> CGFloat {
        let preferred = min(maximum, max(minimum, centreWidth * ratio))
        return max(0, min(preferred, centreWidth - gutter * 2))
    }
}
