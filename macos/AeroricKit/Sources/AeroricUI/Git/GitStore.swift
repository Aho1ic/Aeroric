import Foundation
import Observation

/// P6-04：Git 簿记（Action 3 十视图的数据源；快照来自宿主应答，本类型只
/// 记账——不做 git 推断）。
@MainActor
@Observable
public final class GitStore {
    public struct Change: Identifiable, Equatable {
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

    public struct Commit: Identifiable, Equatable {
        public var id: String { hash }
        public var hash: String
        public var shortHash: String
        public var author: String
        public var date: String
        public var message: String

        public init(hash: String, shortHash: String, author: String, date: String, message: String) {
            self.hash = hash
            self.shortHash = shortHash
            self.author = author
            self.date = date
            self.message = message
        }
    }

    public struct Branch: Identifiable, Equatable {
        public var id: String { name }
        public var name: String
        public var current: Bool
        public var remote: String?

        public init(name: String, current: Bool, remote: String? = nil) {
            self.name = name
            self.current = current
            self.remote = remote
        }
    }

    public struct StashEntry: Identifiable, Equatable {
        public var id: Int { index }
        public var index: Int
        public var message: String

        public init(index: Int, message: String) {
            self.index = index
            self.message = message
        }
    }

    public private(set) var changes: [Change] = []
    public private(set) var commits: [Commit] = []
    public private(set) var branches: [Branch] = []
    public private(set) var stashEntries: [StashEntry] = []
    /// 冲突文件（ChangeList 中高亮；ConflictResolver 数据源）。
    public private(set) var conflictPaths: Set<String> = []
    /// diff 内容（原始 diff 不截断；可下载到用户指定位置）。
    public private(set) var diffText: String = ""

    public init() {}

    public var stagedChanges: [Change] {
        changes.filter(\.staged)
    }

    public var unstagedChanges: [Change] {
        changes.filter { !$0.staged }
    }

    public func ingest(changes: [Change], conflictPaths: Set<String>) {
        self.changes = changes
        self.conflictPaths = conflictPaths
    }

    public func ingest(commits: [Commit]) {
        self.commits = commits
    }

    public func ingest(branches: [Branch]) {
        self.branches = branches
    }

    public func ingest(stash: [StashEntry]) {
        self.stashEntries = stash
    }

    public func setDiff(_ text: String) {
        diffText = text
    }
}
