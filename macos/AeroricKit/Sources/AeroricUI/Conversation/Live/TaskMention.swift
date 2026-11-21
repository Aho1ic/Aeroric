import Foundation

/// E12 (2026-11-21): "@" in the session composer references other tasks of
/// the same project. Picking one takes the "@…" out of the text and keeps
/// the task as a chip; the host names the picked tasks in front of the
/// message, in the attached-context envelope, and the agent reads them with
/// `read_session`.
public enum TaskMention {
    /// The host takes at most this many per message (`MAX_REFERENCES`).
    public static let maxReferences = 5
    /// Rows the suggestion list shows.
    public static let shownSuggestions = 8

    public struct Option: Equatable, Sendable, Identifiable {
        public var id: String
        public var title: String
        public var status: String

        public init(id: String, title: String, status: String) {
            self.id = id
            self.title = title
            self.status = status
        }
    }

    /// The project's other tasks, the most recently changed first;
    /// archived ones are left out.
    public static func options(_ tasks: [TaskStore.TaskEntry], for task: TaskStore.TaskEntry,
                               limit: Int = 50) -> [Option] {
        tasks.filter { $0.projectID == task.projectID && $0.id != task.id && !$0.archived }
            .sorted { ($0.updatedAtMs ?? $0.createdAtMs ?? 0) > ($1.updatedAtMs ?? $1.createdAtMs ?? 0) }
            .prefix(limit)
            .map { Option(id: $0.id, title: $0.title.isEmpty ? "（无标题）" : $0.title, status: $0.status) }
    }

    /// What follows the "@" being typed at the end of `draft`: an "@" at the
    /// start or after a space or line break, with no space after it. Nil
    /// otherwise (an e-mail address, a finished word).
    public static func query(_ draft: String) -> String? {
        guard let at = draft.lastIndex(of: "@") else { return nil }
        let after = draft[draft.index(after: at)...]
        guard !after.contains(where: \.isWhitespace) else { return nil }
        if at != draft.startIndex, !draft[draft.index(before: at)].isWhitespace { return nil }
        return String(after)
    }

    /// The tasks the "@…" being typed matches, not yet picked; none once
    /// the message has as many as the host takes.
    public static func suggestions(_ draft: String, options: [Option], picked: [Option]) -> [Option] {
        guard let typed = query(draft), picked.count < maxReferences else { return [] }
        let needle = typed.lowercased()
        return Array(options.filter { option in
            !picked.contains { $0.id == option.id }
                && (needle.isEmpty || option.title.lowercased().contains(needle)
                    || option.id.lowercased().hasPrefix(needle))
        }.prefix(shownSuggestions))
    }

    /// `draft` without the "@…" being typed.
    public static func removingQuery(_ draft: String) -> String {
        guard query(draft) != nil, let at = draft.lastIndex(of: "@") else { return draft }
        return String(draft[..<at])
    }
}
