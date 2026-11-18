import Foundation

/// 待办第二批: one run of a todo's 验收检查, for the turn it checked.
public struct LiveTodoCheck: Equatable, Sendable {
    /// `running` `passed` `failed`.
    public var state: String
    public var output: String
    public var atMs: UInt64

    public init(state: String, output: String = "", atMs: UInt64 = 0) {
        self.state = state
        self.output = output
        self.atMs = atMs
    }
}

/// 2026-11-17: what a todo is handed out to — an Agent, or a configuration
/// of the project's session catalog (which names its Agent) and a model.
public struct LiveTodoRunner: Equatable, Sendable {
    public var agent: String
    public var configurationId: String?
    public var model: String?

    public init(agent: String, configurationId: String? = nil, model: String? = nil) {
        self.agent = agent
        self.configurationId = configurationId
        self.model = model
    }

    /// `todo.assign`'s params beside the todo's id.
    public var params: [String: JSONValue] {
        var params: [String: JSONValue] = ["agent": .string(agent)]
        if let configurationId { params["configurationId"] = .string(configurationId) }
        if let model { params["model"] = .string(model) }
        return params
    }
}

/// 2026-11-15 工作安排第一批: one hand-out of a 待办 — the session it opened.
public struct LiveTodoAssignment: Equatable, Sendable {
    public var taskId: String
    public var agent: String
    public var model: String?
    /// 2026-11-17: the session catalog's configuration it ran with, and
    /// "配置名 · 模型" for the list; absent when only the Agent was picked.
    public var configurationId: String?
    public var label: String?
    public var atMs: UInt64
    /// 第二批: the latest 验收检查 of this session, and the failing ones
    /// sent back into it.
    public var check: LiveTodoCheck?
    public var fixes: Int

    public init(taskId: String, agent: String, model: String? = nil, configurationId: String? = nil,
                label: String? = nil, atMs: UInt64 = 0, check: LiveTodoCheck? = nil, fixes: Int = 0) {
        self.taskId = taskId
        self.agent = agent
        self.model = model
        self.configurationId = configurationId
        self.label = label
        self.atMs = atMs
        self.check = check
        self.fixes = fixes
    }
}

/// A 待办: written by the user, handed to an Agent by hand, accepted by the
/// user. `progress` is read by the host from the latest session.
public struct LiveTodo: Equatable, Sendable, Identifiable {
    public var id: String
    public var projectId: String
    public var title: String
    public var notes: String
    /// `proposed` `open` `assigned` `done` `cancelled`.
    public var state: String
    public var assignments: [LiveTodoAssignment]
    public var createdMs: UInt64
    public var updatedMs: UInt64
    public var closedMs: UInt64?
    /// `running` `waiting` `review` `notRun` `failed` `stopped` `missing`.
    public var progress: String?
    public var progressDetail: String?
    /// 第二批: the session that proposed it (`todo_propose`).
    public var proposedBy: String?
    /// 验收检查: run in the project after the session's turn; empty for none.
    public var check: String
    /// A failing check goes back into the session (at most twice).
    public var fixOnFail: Bool

    public init(id: String = "", projectId: String = "", title: String = "", notes: String = "",
                state: String = "open", assignments: [LiveTodoAssignment] = [], createdMs: UInt64 = 0,
                updatedMs: UInt64 = 0, closedMs: UInt64? = nil, progress: String? = nil,
                progressDetail: String? = nil, proposedBy: String? = nil, check: String = "",
                fixOnFail: Bool = false) {
        self.id = id
        self.projectId = projectId
        self.title = title
        self.notes = notes
        self.state = state
        self.assignments = assignments
        self.createdMs = createdMs
        self.updatedMs = updatedMs
        self.closedMs = closedMs
        self.progress = progress
        self.progressDetail = progressDetail
        self.proposedBy = proposedBy
        self.check = check
        self.fixOnFail = fixOnFail
    }

    /// The session that counts now.
    public var latestTaskId: String? { assignments.last?.taskId }
    public var isClosed: Bool { state == "done" || state == "cancelled" }
    public var isProposed: Bool { state == "proposed" }
    /// The latest session's 验收检查.
    public var lastCheck: LiveTodoCheck? { assignments.last?.check }

    public static func decode(_ value: JSONValue) -> LiveTodo? {
        guard let f = value.objectValue, let id = f["id"]?.stringValue else { return nil }
        let number = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } }
        let assignments = (f["assignments"]?.arrayValue ?? []).compactMap { item -> LiveTodoAssignment? in
            guard let a = item.objectValue, let task = a["taskId"]?.stringValue else { return nil }
            let check = a["check"]?.objectValue.flatMap { c -> LiveTodoCheck? in
                guard let state = c["state"]?.stringValue else { return nil }
                return LiveTodoCheck(state: state, output: c["output"]?.stringValue ?? "",
                                     atMs: c["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
            }
            return LiveTodoAssignment(taskId: task, agent: a["agent"]?.stringValue ?? "", model: a["model"]?.stringValue,
                                      configurationId: a["configurationId"]?.stringValue,
                                      label: a["label"]?.stringValue,
                                      atMs: a["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0, check: check,
                                      fixes: a["fixes"]?.numberText.flatMap { Int($0) } ?? 0)
        }
        return LiveTodo(id: id, projectId: f["projectId"]?.stringValue ?? "", title: f["title"]?.stringValue ?? "",
                        notes: f["notes"]?.stringValue ?? "", state: f["state"]?.stringValue ?? "open",
                        assignments: assignments, createdMs: number("createdMs") ?? 0,
                        updatedMs: number("updatedMs") ?? 0, closedMs: number("closedMs"),
                        progress: f["progress"]?.stringValue, progressDetail: f["progressDetail"]?.stringValue,
                        proposedBy: f["proposedBy"]?.stringValue, check: f["check"]?.stringValue ?? "",
                        fixOnFail: f["fixOnFail"]?.boolValue ?? false)
    }

    /// What a save sends: the words and the 验收检查, never the state (the
    /// host keeps it).
    public var json: JSONValue {
        let check = check.trimmingCharacters(in: .whitespacesAndNewlines)
        var fields: [String: JSONValue] = ["id": .string(id), "projectId": .string(projectId), "title": .string(title),
                                           "notes": .string(notes), "fixOnFail": .bool(fixOnFail && !check.isEmpty)]
        if !check.isEmpty { fields["check"] = .string(check) }
        return .object(fields)
    }
}
