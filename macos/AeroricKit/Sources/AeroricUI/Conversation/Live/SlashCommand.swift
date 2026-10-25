import Foundation

/// Composer slash commands (Q15). Only these five are interpreted; any other
/// `/text` is sent to the Agent as an ordinary message.
public enum SlashCommand: Equatable, Sendable {
    case compact
    case review
    case model(String?)
    case permissions
    case clear

    public struct Suggestion: Equatable, Sendable, Identifiable {
        public var command: String
        public var detail: String
        public var enabled: Bool
        public var id: String { command }
    }

    public static func parse(_ text: String) -> SlashCommand? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("/") else { return nil }
        let parts = trimmed.dropFirst().split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        guard let head = parts.first else { return nil }
        let argument = parts.count > 1 ? String(parts[1]).trimmingCharacters(in: .whitespaces) : nil
        switch head.lowercased() {
        case "compact": return argument == nil ? .compact : nil
        case "review": return argument == nil ? .review : nil
        case "model": return .model(argument.flatMap { $0.isEmpty ? nil : $0 })
        case "permissions": return argument == nil ? .permissions : nil
        case "clear": return argument == nil ? .clear : nil
        default: return nil
        }
    }

    /// Suggestions while the input is a bare `/prefix` (no space yet).
    public static func suggestions(for text: String, family: String?) -> [Suggestion] {
        guard text.hasPrefix("/"), !text.contains(" "), !text.contains("\n") else { return [] }
        let prefix = text.dropFirst().lowercased()
        let codex = family == "codex"
        let all = [
            Suggestion(command: "/compact", detail: "压缩上下文", enabled: true),
            Suggestion(command: "/review", detail: codex ? "审查未提交的改动" : "仅 Codex 支持", enabled: codex),
            Suggestion(command: "/model", detail: "切换模型（下一轮生效）", enabled: true),
            Suggestion(command: "/permissions", detail: "切换权限（下一轮生效）", enabled: true),
            Suggestion(command: "/clear", detail: "以同样配置开新会话", enabled: true),
        ]
        return all.filter { $0.command.dropFirst().hasPrefix(prefix) }
    }
}

/// When to post "needs your approval" (N4 D5): only while the app is in the
/// background, once per task entering `input_required`.
public struct ApprovalNotificationGate: Equatable, Sendable {
    private var notified: Set<String> = []

    public init() {}

    public mutating func shouldNotify(taskID: String, previous: String?, current: String, appActive: Bool) -> Bool {
        guard current == "input_required" else {
            notified.remove(taskID)
            return false
        }
        guard previous != "input_required", !appActive, !notified.contains(taskID) else { return false }
        notified.insert(taskID)
        return true
    }
}
