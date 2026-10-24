import AeroricClient
import Foundation
import Observation

public enum ResourceListSection: String, Hashable, Sendable { case files, git }
public enum ResourcePreviewKind: String, Hashable, Sendable {
    case file, stagedDiff, unstagedDiff, untracked
    public var label: String {
        switch self {
        case .file: "只读文件"
        case .stagedDiff: "已暂存 diff"
        case .unstagedDiff: "未暂存 diff"
        case .untracked: "未跟踪文件"
        }
    }
}
public struct ResourcePreviewTarget: Hashable, Sendable {
    public let projectID: String
    public let path: String
    public let kind: ResourcePreviewKind
    public init(projectID: String, path: String, kind: ResourcePreviewKind) {
        self.projectID = projectID; self.path = path; self.kind = kind
    }
}
public enum ResourceReadStatus: Equatable, Sendable {
    case notLoaded, loading, loaded, cancelled
    case failed(String), cached(String), unavailable(String), unsupported(String)
    /// Used when entering a new visible resource context, not as a polling
    /// condition. A replacement task may supersede a cancelled predecessor
    /// that is still suspended in its reader; its UUID rejects the old reply.
    public var needsAutomaticLoad: Bool {
        switch self {
        case .notLoaded, .loading, .cancelled: true
        default: false
        }
    }
    public var notice: String? {
        switch self {
        case .notLoaded: "尚未读取"
        case .loading: "正在读取…"
        case .loaded: nil
        case .cancelled: "读取已取消，可重试"
        case let .failed(reason): "读取失败：\(reason)"
        case let .cached(reason): "缓存快照，\(reason)"
        case let .unavailable(reason), let .unsupported(reason): reason
        }
    }
}
public struct ResourcePreviewLine: Identifiable, Equatable, Sendable {
    public enum Kind: Equatable, Sendable { case plain, metadata, context, added, removed }
    public let id: Int
    public let oldNumber: Int?
    public let newNumber: Int?
    public let text: String
    public let kind: Kind

    /// Presentation-only line numbering. Original source is retained, including
    /// metadata, +/- markers and unknown records; this is not a Git engine.
    public static func lines(_ text: String, diff: Bool) -> [Self] {
        let hunk = try? NSRegularExpression(pattern: #"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@"#)
        var old: Int?, new: Int?
        return text.components(separatedBy: "\n").enumerated().map { index, line in
            if !diff {
                return Self(id: index, oldNumber: nil, newNumber: index + 1, text: line, kind: .plain)
            }
            if let match = hunk?.firstMatch(in: line, range: NSRange(line.startIndex..., in: line)),
               let a = Range(match.range(at: 1), in: line), let b = Range(match.range(at: 2), in: line) {
                old = Int(line[a]); new = Int(line[b])
                return Self(id: index, oldNumber: nil, newNumber: nil, text: line, kind: .metadata)
            }
            var before: Int?, after: Int?
            let kind: Kind
            if old != nil, new != nil, line.hasPrefix("+") {
                kind = .added; after = new; new = new.map { $0 + 1 }
            } else if old != nil, new != nil, line.hasPrefix("-") {
                kind = .removed; before = old; old = old.map { $0 + 1 }
            } else if old != nil, new != nil, line.hasPrefix(" ") {
                kind = .context; before = old; after = new
                old = old.map { $0 + 1 }; new = new.map { $0 + 1 }
            } else {
                kind = .metadata
                if !line.hasPrefix("\\") { old = nil; new = nil }
            }
            return Self(id: index, oldNumber: before, newNumber: after, text: line, kind: kind)
        }
    }
}

public struct InlineDiff: Equatable, Sendable {
    public var state: ResourceReadStatus = .notLoaded
    public var lines: [ResourcePreviewLine] = []
    /// Lines beyond `limit` are not rendered inline; the centre preview has all.
    public var totalLines = 0
    public static let limit = 300
}

public struct ProjectResourceScene: Sendable {
    public var section: ResourceListSection = .files
    public var expandedPaths: Set<String> = [""]
    public var directories: [String: [FilesClient.FileEntry]] = [:]
    public var directoryStates: [String: ResourceReadStatus] = [:]
    public var changes: [ResourceGitChange] = []
    public var branches: [ResourceGitBranch] = []
    public var gitState: ResourceReadStatus = .notLoaded
    public var branchState: ResourceReadStatus = .notLoaded
    public var target: ResourcePreviewTarget?
    public var previewActive = false
    public var previewState: ResourceReadStatus = .notLoaded
    public var previewText: String?
    public var previewLines: [ResourcePreviewLine] = []
    public var positions: [ResourcePreviewTarget: Int] = [:]
    /// Inline diff under a change row (M3), keyed by `ResourceGitChange.id`.
    /// Separate from the centre preview so expanding a row never replaces it.
    public var expandedChange: String?
    public var inlineDiffs: [String: InlineDiff] = [:]
    fileprivate var directoryRequests: [String: UUID] = [:]
    fileprivate var gitRequest: UUID?
    fileprivate var previewRequest: UUID?
    fileprivate var positionOrder: [ResourcePreviewTarget] = []
    public init() {}
}

/// One instance per window, keyed by project. Memory-only read presentation;
/// it never writes business data, opens documents, or controls an Agent.
@MainActor @Observable
public final class ProjectResourceBook {
    private var scenes: [String: ProjectResourceScene] = [:]
    public init() {}
    public func scene(_ projectID: String) -> ProjectResourceScene { scenes[projectID] ?? .init() }
    public func edit(_ projectID: String, _ body: (inout ProjectResourceScene) -> Void) {
        var value = scene(projectID); body(&value); scenes[projectID] = value
    }
    public func showTask(projectID: String) { edit(projectID) { $0.previewActive = false } }
    public func select(_ target: ResourcePreviewTarget) {
        edit(target.projectID) { value in
            if value.target != target {
                value.target = target
                value.previewRequest = nil
                value.previewText = nil
                value.previewLines = []
                value.previewState = .notLoaded
            }
            value.previewActive = true
        }
    }
    public func rememberPosition(_ position: Int?, for target: ResourcePreviewTarget) {
        guard let position else { return }
        edit(target.projectID) { value in
            value.positionOrder.removeAll { $0 == target }
            value.positionOrder.append(target)
            value.positions[target] = position
            while value.positionOrder.count > 32 {
                value.positions.removeValue(forKey: value.positionOrder.removeFirst())
            }
        }
    }
    public func invalidateSnapshots() {
        for project in Array(scenes.keys) {
            edit(project) { value in
                value.directoryRequests = [:]; value.gitRequest = nil; value.previewRequest = nil
                for path in value.directoryStates.keys {
                    value.directoryStates[path] = value.directories[path] == nil
                        ? .notLoaded : .cached("宿主连接已变化，需显式刷新")
                }
                value.gitState = .cached("宿主连接已变化，需显式刷新")
                value.branchState = .cached("宿主连接已变化，需显式刷新")
                value.previewState = .cached("宿主连接已变化，需显式刷新")
                value.inlineDiffs = [:]
            }
        }
    }

    public func loadDirectory(projectID: String, path: String, reader: (any ProjectResourceReading)?) async {
        guard !Task.isCancelled else { return }
        let request = UUID()
        edit(projectID) { $0.directoryRequests[path] = request; $0.directoryStates[path] = .loading }
        guard let reader else {
            edit(projectID) { $0.directoryStates[path] = .unavailable("宿主不可用，未读取此目录") }
            return
        }
        do {
            let entries = try await reader.directory(projectID: projectID, path: path)
            try Task.checkCancellation()
            guard scene(projectID).directoryRequests[path] == request else { return }
            edit(projectID) { $0.directories[path] = entries; $0.directoryStates[path] = .loaded }
        } catch {
            guard scene(projectID).directoryRequests[path] == request else { return }
            edit(projectID) {
                $0.directoryStates[path] = Self.isCancellation(error) ? .cancelled : .failed(error.localizedDescription)
            }
        }
    }
    public func loadGit(projectID: String, projectPath: String, reader: (any ProjectResourceReading)?) async {
        guard !Task.isCancelled else { return }
        let request = UUID()
        edit(projectID) { $0.gitRequest = request; $0.gitState = .loading; $0.branchState = .loading }
        guard let reader else {
            edit(projectID) {
                $0.gitState = .unavailable("宿主不可用，未读取 Git")
                $0.branchState = .unavailable("分支尚未读取")
            }
            return
        }
        do {
            let changes = try await reader.changes(projectPath: projectPath)
            try Task.checkCancellation()
            guard scene(projectID).gitRequest == request else { return }
            edit(projectID) { $0.changes = changes; $0.gitState = .loaded; $0.inlineDiffs = [:]; $0.expandedChange = nil }
        } catch {
            guard scene(projectID).gitRequest == request else { return }
            if Self.isCancellation(error) {
                edit(projectID) { $0.gitState = .cancelled; $0.branchState = .cancelled }
                return
            }
            edit(projectID) { $0.gitState = .failed(error.localizedDescription) }
        }
        do {
            try Task.checkCancellation()
            let branches = try await reader.branches(projectPath: projectPath)
            try Task.checkCancellation()
            guard scene(projectID).gitRequest == request else { return }
            edit(projectID) { $0.branches = branches; $0.branchState = .loaded }
        } catch {
            guard scene(projectID).gitRequest == request else { return }
            edit(projectID) {
                $0.branchState = Self.isCancellation(error) ? .cancelled : .failed(error.localizedDescription)
            }
        }
    }
    public func loadPreview(_ target: ResourcePreviewTarget, projectPath: String,
                            reader: (any ProjectResourceReading)?) async {
        guard !Task.isCancelled, scene(target.projectID).target == target else { return }
        let request = UUID()
        edit(target.projectID) { $0.previewRequest = request; $0.previewState = .loading }
        guard let reader else {
            edit(target.projectID) { $0.previewState = .unavailable("宿主不可用，不能读取此目标") }
            return
        }
        let ext = (target.path as NSString).pathExtension.lowercased()
        if target.kind == .file {
            if [
                "png", "jpg", "jpeg", "gif", "webp", "heic", "pdf", "zip", "ttf", "woff", "woff2",
            ].contains(ext) {
                edit(target.projectID) {
                    $0.previewState = .unsupported("此类型尚无已接入的只读查看器，不会自动外开或授权。")
                }
                return
            }
        }

        do {
            let text: String
            if target.kind == .file {
                text = try await reader.text(projectID: target.projectID, path: target.path)
            } else {
                text = try await reader.diff(projectPath: projectPath, path: target.path, staged: target.kind == .stagedDiff)
            }
            try Task.checkCancellation()
            guard scene(target.projectID).previewRequest == request else { return }
            // Refuse oversized/binary content instead of silently clipping.
            guard text.utf8.count <= 1_048_576, !text.contains("\0") else {
                edit(target.projectID) { $0.previewState = .unsupported("内容过大或含二进制数据，未截断为伪完整预览。") }
                return
            }
            let lines = await Task.detached {
                ResourcePreviewLine.lines(text, diff: target.kind != .file)
            }.value
            try Task.checkCancellation()
            guard scene(target.projectID).previewRequest == request,
                  scene(target.projectID).target == target else { return }
            edit(target.projectID) {
                $0.previewText = text; $0.previewLines = lines; $0.previewState = .loaded
            }
        } catch {
            guard scene(target.projectID).previewRequest == request else { return }
            edit(target.projectID) {
                if Self.isCancellation(error) {
                    $0.previewState = .cancelled
                } else {
                    $0.previewState = .failed(error.localizedDescription)
                }
            }
        }
    }
    /// Toggle a change row's inline diff; loads it on first expand.
    public func toggleInlineDiff(_ change: ResourceGitChange, projectID: String, projectPath: String,
                                 reader: (any ProjectResourceReading)?) async {
        if scene(projectID).expandedChange == change.id {
            edit(projectID) { $0.expandedChange = nil }
            return
        }
        edit(projectID) { $0.expandedChange = change.id }
        if let cached = scene(projectID).inlineDiffs[change.id], cached.state == .loaded { return }
        edit(projectID) { $0.inlineDiffs[change.id] = InlineDiff(state: .loading) }
        guard let reader else {
            edit(projectID) { $0.inlineDiffs[change.id] = InlineDiff(state: .unavailable("宿主不可用，不能读取此目标")) }
            return
        }
        do {
            let untracked = change.status == "?"
            let text = untracked
                ? try await reader.text(projectID: projectID, path: change.path)
                : try await reader.diff(projectPath: projectPath, path: change.path, staged: change.staged)
            guard text.utf8.count <= 1_048_576, !text.contains("\0") else {
                edit(projectID) {
                    $0.inlineDiffs[change.id] = InlineDiff(state: .unsupported("内容过大或含二进制数据，请在中栏查看。"))
                }
                return
            }
            let lines = await Task.detached { ResourcePreviewLine.lines(text, diff: !untracked) }.value
            edit(projectID) {
                $0.inlineDiffs[change.id] = InlineDiff(state: .loaded, lines: Array(lines.prefix(InlineDiff.limit)),
                                                       totalLines: lines.count)
            }
        } catch {
            edit(projectID) {
                $0.inlineDiffs[change.id] = InlineDiff(
                    state: Self.isCancellation(error) ? .cancelled : .failed(error.localizedDescription))
            }
        }
    }

    private static func isCancellation(_ error: Error) -> Bool {
        Task.isCancelled || error is CancellationError || (error as? ConnectionError) == .cancelled
    }
}
