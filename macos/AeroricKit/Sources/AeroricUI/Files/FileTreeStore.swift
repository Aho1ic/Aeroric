import Foundation
import Observation

/// P6-01 Action 4：文件树簿记。树懒加载（目录按需展开，不一次读整仓）；
/// 条目来自宿主快照（files.read_dir），本类型只记账——不做路径推断。
@MainActor
@Observable
public final class FileTreeStore {
    public struct Node: Identifiable, Equatable {
        public var id: String { path }
        public var name: String
        public var path: String
        public var isDir: Bool
        public var isSymlink: Bool
        /// 已展开（懒加载：首次展开才拉子目录）。
        public var expanded: Bool

        public init(name: String, path: String, isDir: Bool, isSymlink: Bool, expanded: Bool = false) {
            self.name = name
            self.path = path
            self.isDir = isDir
            self.isSymlink = isSymlink
            self.expanded = expanded
        }
    }

    /// 当前 projectId（切换项目时整树重置）。
    public private(set) var projectID: String?
    /// 懒加载缓存：path → 子条目（目录按需拉取）。
    public private(set) var children: [String: [Node]] = [:]
    /// 展开中的目录集合。
    public private(set) var expandedPaths: Set<String> = []
    /// 加载中标记（防重复触发）。
    public private(set) var loadingPaths: Set<String> = []
    /// Why the last create / rename / trash failed (2026-10-30), until dismissed.
    public var actionError: String?

    public init() {}

    /// 切换项目：整树重置。
    public func reset(projectID: String) {
        self.projectID = projectID
        children = [:]
        expandedPaths = []
        loadingPaths = []
    }

    /// 目录条目到达（懒加载缓存写入；幂等）。
    public func ingestChildren(of path: String, entries: [Node]) {
        children[path] = entries
        loadingPaths.remove(path)
    }

    /// 展开/收起（懒加载触发点：首次展开返回 true＝需要拉子目录）。
    @discardableResult
    /// Opens `path` (no-op when open); true when its children should load.
    public func expand(_ path: String) -> Bool {
        guard !path.isEmpty, !expandedPaths.contains(path) else { return false }
        return toggleExpanded(path)
    }

    public func toggleExpanded(_ path: String) -> Bool {
        if expandedPaths.contains(path) {
            expandedPaths.remove(path)
            return false
        }
        expandedPaths.insert(path)
        return children[path] == nil
    }

    public func setLoading(_ path: String, loading: Bool) {
        if loading {
            loadingPaths.insert(path)
        } else {
            loadingPaths.remove(path)
        }
    }

    public func isLoading(_ path: String) -> Bool {
        loadingPaths.contains(path)
    }

    /// 某目录的可见子节点（懒加载：未展开返回空；未加载返回 nil 语义由
    /// children[path] == nil 区分）。
    public func visibleChildren(of path: String) -> [Node]? {
        guard expandedPaths.contains(path) else { return [] }
        // children[path] == nil ＝已展开但未加载（nil 语义）；加载过（含空
        // 目录）返回实际条目。
        return children[path]
    }
}
