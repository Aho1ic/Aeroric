import Foundation

/// Board columns (D1). Pure grouping of the task snapshot; no field is
/// inferred that the host did not send.
public enum TaskBoardColumn: String, CaseIterable, Identifiable, Sendable {
    case active
    case waiting
    case done
    case stopped

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .active: "进行中"
        case .waiting: "等待你回应"
        case .done: "已完成"
        case .stopped: "失败与中断"
        }
    }

    public var emptyText: String {
        switch self {
        case .active: "没有进行中的任务"
        case .waiting: "没有等待你回应的任务"
        case .done: "还没有完成的任务"
        case .stopped: "没有失败或中断的任务"
        }
    }

    /// Unknown wire statuses land in "进行中" rather than disappearing.
    public init(status: String) {
        switch status {
        case "input_required": self = .waiting
        case "done": self = .done
        case "failed", "cancelled", "interrupted": self = .stopped
        default: self = .active
        }
    }

    /// Cancel is offered only where the task may still be running.
    public var allowsCancel: Bool { self == .active || self == .waiting }
}

public enum TaskBoard {
    public struct Column: Identifiable, Equatable, Sendable {
        public var id: TaskBoardColumn
        public var tasks: [TaskStore.TaskEntry]
    }

    /// Every column is present (empty ones too). Archived tasks are left
    /// out; starred first, then most recently updated.
    public static func columns(_ tasks: [TaskStore.TaskEntry]) -> [Column] {
        let visible = tasks.filter { !$0.archived }
        return TaskBoardColumn.allCases.map { column in
            Column(id: column, tasks: sorted(visible.filter { TaskBoardColumn(status: $0.status) == column }))
        }
    }

    public static func sorted(_ tasks: [TaskStore.TaskEntry]) -> [TaskStore.TaskEntry] {
        tasks.sorted { lhs, rhs in
            if lhs.starred != rhs.starred { return lhs.starred }
            let left = lhs.updatedAtMs ?? lhs.createdAtMs ?? 0
            let right = rhs.updatedAtMs ?? rhs.createdAtMs ?? 0
            if left != right { return left > right }
            return lhs.id < rhs.id
        }
    }

    public static func date(of task: TaskStore.TaskEntry) -> Date? {
        (task.updatedAtMs ?? task.createdAtMs).map { Date(timeIntervalSince1970: Double($0) / 1000) }
    }
}

/// Stable, low-saturation project tint index (0..<count) from the project id.
public enum ProjectTint {
    public static let count = 6

    public static func index(for projectID: String) -> Int {
        // FNV-1a: stable across launches, unlike `hashValue`.
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in projectID.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x0000_0100_0000_01b3
        }
        return Int(hash % UInt64(count))
    }
}

/// SF Symbol for a file name in the repository tree.
public enum FileSymbol {
    public static func symbol(forFile name: String) -> String {
        switch (name as NSString).pathExtension.lowercased() {
        case "swift", "rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "h", "cpp", "m", "kt", "java", "rb":
            "chevron.left.forwardslash.chevron.right"
        case "json", "yml", "yaml", "toml", "plist", "xml", "ini", "lock":
            "curlybraces"
        case "md", "txt", "rst":
            "doc.text"
        case "png", "jpg", "jpeg", "gif", "webp", "heic", "svg", "ico":
            "photo"
        case "sh", "zsh", "bash":
            "terminal"
        case "zip", "gz", "tar", "xz":
            "doc.zipper"
        default:
            "doc"
        }
    }
}

/// One-letter status badge for a git change and its meaning.
public enum ChangeBadge: Equatable, Sendable {
    case modified, added, deleted, renamed, untracked, other(String)

    public init(status: String) {
        switch status.uppercased() {
        case "M", "MODIFIED": self = .modified
        case "A", "ADDED": self = .added
        case "D", "DELETED": self = .deleted
        case "R", "RENAMED": self = .renamed
        case "?", "??", "UNTRACKED": self = .untracked
        default: self = .other(status)
        }
    }

    public var letter: String {
        switch self {
        case .modified: "M"
        case .added: "A"
        case .deleted: "D"
        case .renamed: "R"
        case .untracked: "U"
        case let .other(text): String(text.prefix(1)).uppercased()
        }
    }

    public var label: String {
        switch self {
        case .modified: "已修改"
        case .added: "新增"
        case .deleted: "已删除"
        case .renamed: "重命名"
        case .untracked: "未跟踪"
        case let .other(text): text
        }
    }
}
