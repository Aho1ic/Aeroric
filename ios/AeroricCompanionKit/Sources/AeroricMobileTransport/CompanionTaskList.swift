import Foundation

/// 1124d (2026-11-25): what the task list shows. Every task of every project
/// in storage order made a list nobody could find a task in (the test
/// project alone has 132). Now: what is still going on first, then the
/// latest ones, of one project or of all, newest first and capped; older
/// ones are on the Mac.
public enum CompanionTaskList {
    public struct Section: Identifiable, Equatable, Sendable {
        public var id: String
        public var title: String
        public var tasks: [CompanionTask]
        /// Tasks left out for the cap.
        public var omitted: Int
    }

    /// Still holding an agent or waiting on someone (`TaskStatus::is_live`).
    public static let activeStatuses: Set<String> = ["pending", "running", "input_required"]
    /// How many finished tasks the list shows: of all projects, of one.
    public static let recentLimit = 30
    public static let projectLimit = 50

    /// `projectId` nil: every project.
    public static func sections(_ tasks: [CompanionTask], projectId: String?) -> [Section] {
        let mine = tasks.filter { projectId == nil || $0.projectId == projectId }
        let newestFirst: (CompanionTask, CompanionTask) -> Bool = {
            ($0.activityMs ?? 0, $0.id) > ($1.activityMs ?? 0, $1.id)
        }
        let active = mine.filter { activeStatuses.contains($0.status) }.sorted(by: newestFirst)
        let rest = mine.filter { !activeStatuses.contains($0.status) }.sorted(by: newestFirst)
        let limit = projectId == nil ? recentLimit : projectLimit
        var sections: [Section] = []
        if !active.isEmpty {
            sections.append(Section(id: "active", title: "进行中", tasks: active, omitted: 0))
        }
        if !rest.isEmpty {
            sections.append(Section(id: "recent", title: projectId == nil ? "最近" : "这个项目的任务",
                                    tasks: Array(rest.prefix(limit)), omitted: max(0, rest.count - limit)))
        }
        return sections
    }

    /// The newest of a `tasks.list` record's times (ms): created, finished,
    /// updated. A record with none sorts last.
    public static func activity(of record: [String: Any]) -> Int64? {
        ["createdAt", "completedAt", "updatedAt"]
            .compactMap { key -> Int64? in
                if let number = record[key] as? NSNumber { return number.int64Value }
                return nil
            }
            .max()
    }
}
