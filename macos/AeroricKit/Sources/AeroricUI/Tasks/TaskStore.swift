import Foundation
import Observation
import AeroricClient

/// P5-03 Action 2：项目/任务列表的 authoritative 簿记。快照字段（status/
/// time/agent）全部来自宿主应答——本类型不做任何业务推断，只记账与
/// 暴露查询视图。UI 层的局部过滤（filter/search）不在这里（窗口局部
/// 状态归视图）；archive/star 的**权威面**在宿主，这里只镜像。
@MainActor
@Observable
public final class TaskStore {
    public struct ProjectEntry: Identifiable, Equatable, Hashable, Sendable {
        public let id: String
        public var path: String
        public var name: String
        public init(id: String, path: String, name: String? = nil) {
            self.id = id
            self.path = path
            self.name = name ?? URL(fileURLWithPath: path).lastPathComponent
        }
    }

    public struct TaskEntry: Identifiable, Equatable, Sendable {
        public let id: String
        public var projectID: String
        public var title: String
        public var status: String
        public var agent: String?
        public var createdAtMs: UInt64?
        public var updatedAtMs: UInt64?
        /// 归档/置星是宿主权威状态（ADR-037 词汇表），这里镜像。
        public var archived: Bool
        public var starred: Bool

        public init(
            id: String,
            projectID: String,
            title: String,
            status: String,
            agent: String? = nil,
            createdAtMs: UInt64? = nil,
            updatedAtMs: UInt64? = nil,
            archived: Bool = false,
            starred: Bool = false
        ) {
            self.id = id
            self.projectID = projectID
            self.title = title
            self.status = status
            self.agent = agent
            self.createdAtMs = createdAtMs
            self.updatedAtMs = updatedAtMs
            self.archived = archived
            self.starred = starred
        }
    }

    public private(set) var projects: [ProjectEntry] = []
    public private(set) var tasks: [TaskEntry] = []
    /// 每窗口选中的项目（§2.4：selected project 归窗口）。
    public var selectedProjectID: String?
    /// 窗口局部过滤（TaskListView 消费；不是宿主 archive）。
    public var filterText: String = "" {
        didSet { rebuildVisible() }
    }
    public private(set) var visibleTasks: [TaskEntry] = []

    public init() {}

    // MARK: - 镜像（宿主应答 → 条目）

    public func upsertProject(id: String, path: String, name: String? = nil) -> ProjectEntry {
        let entry = ProjectEntry(id: id, path: path, name: name)
        if let index = projects.firstIndex(where: { $0.id == id }) {
            projects[index] = entry
        } else {
            projects.append(entry)
        }
        return entry
    }

    public func upsertTask(_ task: TaskEntry) {
        if let index = tasks.firstIndex(where: { $0.id == task.id }) {
            tasks[index] = task
        } else {
            tasks.append(task)
        }
        rebuildVisible()
    }

    public func removeTask(id: String) {
        tasks.removeAll { $0.id == id }
        rebuildVisible()
    }

    // MARK: - 修订冲突（Action：多窗口 revision conflict）

    /// 多窗口写冲突：宿主按 revision 拒绝时记录冲突 id，UI 提示用户
    /// 重试；**自动刷新后不覆盖其他窗口的变更**（冲突条目不本地覆写）。
    public private(set) var conflictedTaskIDs: Set<String> = []

    public func recordConflict(taskID: String) {
        conflictedTaskIDs.insert(taskID)
    }

    /// 自动刷新（宿主快照重新到达）后清除冲突标记，用户再点重试。
    public func clearConflict(taskID: String) {
        conflictedTaskIDs.remove(taskID)
    }

    // MARK: - 批量操作（Action 4）：逐项回填结果——成功/失败**分开记账**，
    // 不允许一个全局 Toast 吞掉局部失败。
    public struct BatchOutcome {
        public let succeeded: [String]
        public let failed: [(id: String, reason: String)]
    }

    public func applyBatchResults(_ results: [(id: String, outcome: Result<Void, Error>)]) -> BatchOutcome {
        var succeeded: [String] = []
        var failed: [(id: String, reason: String)] = []
        for (id, outcome) in results {
            switch outcome {
            case .success:
                succeeded.append(id)
            case let .failure(error):
                failed.append((id, String(describing: error)))
            }
        }
        return BatchOutcome(succeeded: succeeded, failed: failed)
    }

    // MARK: - 查询

    public func tasks(inProject projectID: String) -> [TaskEntry] {
        tasks.filter { $0.projectID == projectID }
    }

    private func rebuildVisible() {
        let trimmed = filterText.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            visibleTasks = tasks
        } else {
            visibleTasks = tasks.filter {
                $0.title.localizedCaseInsensitiveContains(trimmed)
                    || $0.status.localizedCaseInsensitiveContains(trimmed)
            }
        }
    }
}
