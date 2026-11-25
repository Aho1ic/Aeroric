import Foundation

/// D4 (2026-11-23): one read the 文件与 Git page needs from the Mac. The store
/// asks for it (`CompanionStore.onRead`), the connector sends it, and the
/// answer comes back to `CompanionStore.applyRead`. Read only: the phone
/// never writes a file (backlog §4.4-8, 远程写文件默认关闭).
public enum CompanionRead: Equatable, Sendable {
    /// `project.files`; `directory` nil is the project's root.
    case files(projectId: String, directory: String?)
    /// `git.changes`; with a task, in its own copy when it has one.
    case changes(projectId: String, taskId: String? = nil)
    /// `git.diff` of one changed file; with a task, as `changes`.
    case diff(projectId: String, path: String, staged: Bool, taskId: String? = nil)
    /// `project.readFile`.
    case file(projectId: String, path: String)

    public var method: String {
        switch self {
        case .files: "project.files"
        case .changes: "git.changes"
        case .diff: "git.diff"
        case .file: "project.readFile"
        }
    }

    public var params: [String: Any] {
        switch self {
        case let .files(projectId, directory):
            var params: [String: Any] = ["projectId": projectId]
            if let directory { params["path"] = directory }
            return params
        case let .changes(projectId, taskId):
            var params: [String: Any] = ["projectId": projectId]
            if let taskId { params["taskId"] = taskId }
            return params
        case let .diff(projectId, path, staged, taskId):
            var params: [String: Any] = ["projectId": projectId, "path": path, "staged": staged]
            if let taskId { params["taskId"] = taskId }
            return params
        case let .file(projectId, path):
            return ["projectId": projectId, "path": path]
        }
    }
}

/// Something read from the Mac, or why not.
public enum CompanionLoad<Value: Equatable & Sendable>: Equatable, Sendable {
    case loading
    case loaded(Value)
    /// The Mac answered that it cannot show this, and why.
    case unavailable(String)
    /// No answer.
    case failed(String)

    public var value: Value? {
        if case let .loaded(value) = self { return value }
        return nil
    }
}

public struct CompanionFileEntry: Identifiable, Equatable, Sendable {
    public var id: String { path }
    public var name: String
    public var path: String
    public var isDirectory: Bool

    public init(name: String, path: String, isDirectory: Bool) {
        self.name = name
        self.path = path
        self.isDirectory = isDirectory
    }
}

public struct CompanionGitChange: Identifiable, Equatable, Sendable {
    public var id: String { "\(path):\(staged)" }
    public var path: String
    public var status: String
    public var staged: Bool

    public init(path: String, status: String, staged: Bool) {
        self.path = path
        self.status = status
        self.staged = staged
    }
}

/// An open file or diff.
public struct CompanionDocument: Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case file
        case diff(staged: Bool)
    }

    public var kind: Kind
    public var path: String
    public var content: CompanionLoad<CompanionDocumentText>

    public init(kind: Kind, path: String, content: CompanionLoad<CompanionDocumentText> = .loading) {
        self.kind = kind
        self.path = path
        self.content = content
    }
}

public struct CompanionDocumentText: Equatable, Sendable {
    public var text: String
    /// The Mac sent only the start (`project.readFile` at most 200 KB).
    public var truncated: Bool

    public init(text: String, truncated: Bool = false) {
        self.text = text
        self.truncated = truncated
    }
}

/// What the 文件与 Git page shows for one project.
public struct CompanionBrowse: Equatable, Sendable {
    public var projectId: String
    /// The folders opened from the root, outermost first; empty is the root.
    public var folders: [String]
    public var entries: CompanionLoad<[CompanionFileEntry]>
    public var changes: CompanionLoad<[CompanionGitChange]>
    public var document: CompanionDocument?

    public init(projectId: String) {
        self.projectId = projectId
        folders = []
        entries = .loading
        changes = .loading
        document = nil
    }

    public var directory: String? { folders.last }

    /// The reads that show what is on the page now.
    public var reads: [CompanionRead] {
        var reads: [CompanionRead] = [.changes(projectId: projectId), .files(projectId: projectId, directory: directory)]
        if let document {
            switch document.kind {
            case .file: reads.append(.file(projectId: projectId, path: document.path))
            case let .diff(staged): reads.append(.diff(projectId: projectId, path: document.path, staged: staged))
            }
        }
        return reads
    }

    // MARK: - Reading the Mac's answers (`remote_headless.rs`)

    static func object(_ answer: String) -> [String: Any]? {
        guard let data = answer.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    /// `available: false` → why, in the words the page shows.
    static func unavailable(_ object: [String: Any]) -> String? {
        guard object["available"] as? Bool != true else { return nil }
        switch object["reason"] as? String {
        case "ssh": return "这个项目在远程主机（SSH）上，手机还看不了它的文件"
        case "wsl": return "这个项目在 WSL 里，手机还看不了它的文件"
        case "not_git": return "这个项目不是 Git 仓库，没有改动可看"
        case let other?: return "Mac 没有给出：\(other)"
        case nil: return "Mac 没有给出"
        }
    }

    static func entries(_ answer: String) -> CompanionLoad<[CompanionFileEntry]> {
        guard let object = object(answer) else { return .failed("Mac 的回答读不懂") }
        if let reason = unavailable(object) { return .unavailable(reason) }
        let items = object["entries"] as? [[String: Any]] ?? []
        return .loaded(items.compactMap { item in
            guard let path = item["path"] as? String else { return nil }
            return CompanionFileEntry(name: (item["name"] as? String) ?? path, path: path,
                                      isDirectory: (item["isDirectory"] as? Bool) ?? false)
        })
    }

    static func changes(_ answer: String) -> CompanionLoad<[CompanionGitChange]> {
        guard let object = object(answer) else { return .failed("Mac 的回答读不懂") }
        if let reason = unavailable(object) { return .unavailable(reason) }
        let items = object["changes"] as? [[String: Any]] ?? []
        return .loaded(items.compactMap { item in
            guard let path = item["path"] as? String else { return nil }
            return CompanionGitChange(path: path, status: (item["status"] as? String) ?? "?",
                                      staged: (item["staged"] as? Bool) ?? false)
        })
    }

    /// `git.changes` says which tree it read (`worktree` / `project`).
    static func scope(_ answer: String) -> CompanionTaskChanges.Scope? {
        (object(answer)?["scope"] as? String).flatMap(CompanionTaskChanges.Scope.init(rawValue:))
    }

    static func text(_ answer: String, key: String) -> CompanionLoad<CompanionDocumentText> {
        guard let object = object(answer) else { return .failed("Mac 的回答读不懂") }
        if let reason = unavailable(object) { return .unavailable(reason) }
        return .loaded(CompanionDocumentText(text: (object[key] as? String) ?? "",
                                             truncated: (object["truncated"] as? Bool) ?? false))
    }
}

/// D4 (2026-11-23): the task page's 改动 — read with the task, so a task in
/// its own copy shows that copy's changes.
public struct CompanionTaskChanges: Equatable, Sendable {
    public enum Scope: String, Equatable, Sendable {
        /// The task's own copy (`worktreePath`).
        case worktree
        /// The project folder: every uncommitted change there, not only
        /// this task's.
        case project
    }

    public var taskId: String
    public var projectId: String
    public var scope: Scope?
    public var changes: CompanionLoad<[CompanionGitChange]>
    public var document: CompanionDocument?

    public init(taskId: String, projectId: String) {
        self.taskId = taskId
        self.projectId = projectId
        scope = nil
        changes = .loading
        document = nil
    }

    /// The reads that show what the section shows now.
    public var reads: [CompanionRead] {
        var reads: [CompanionRead] = [.changes(projectId: projectId, taskId: taskId)]
        if let document, case let .diff(staged) = document.kind {
            reads.append(.diff(projectId: projectId, path: document.path, staged: staged, taskId: taskId))
        }
        return reads
    }
}
