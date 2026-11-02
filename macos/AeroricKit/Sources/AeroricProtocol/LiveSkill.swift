import Foundation

/// B5-3b (2026-10-31): an experience proposed as a skill.
public struct LiveSkillProposal: Equatable, Sendable, Identifiable {
    public var id: String
    public var experienceId: String
    public var taskId: String?
    public var projectId: String?
    public var global: Bool
    public var name: String
    public var description: String
    public var body: String
    /// `pending` `accepted` `rejected` `disabled`.
    public var state: String
    public var versions: [LiveSkillVersion]
    public var activeVersion: Int?
    public var updatedMs: UInt64

    public init(id: String, experienceId: String = "", taskId: String? = nil, projectId: String? = nil,
                global: Bool = false, name: String, description: String = "", body: String = "",
                state: String = "pending", versions: [LiveSkillVersion] = [], activeVersion: Int? = nil,
                updatedMs: UInt64 = 0) {
        self.id = id
        self.experienceId = experienceId
        self.taskId = taskId
        self.projectId = projectId
        self.global = global
        self.name = name
        self.description = description
        self.body = body
        self.state = state
        self.versions = versions
        self.activeVersion = activeVersion
        self.updatedMs = updatedMs
    }

    public static func decode(_ value: JSONValue?) -> LiveSkillProposal? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue, let name = f["name"]?.stringValue
        else { return nil }
        return LiveSkillProposal(
            id: id, experienceId: f["experienceId"]?.stringValue ?? "", taskId: f["taskId"]?.stringValue,
            projectId: f["projectId"]?.stringValue, global: f["global"]?.boolValue ?? false, name: name,
            description: f["description"]?.stringValue ?? "", body: f["body"]?.stringValue ?? "",
            state: f["state"]?.stringValue ?? "pending",
            versions: (f["versions"]?.arrayValue ?? []).compactMap(LiveSkillVersion.decode),
            activeVersion: f["activeVersion"]?.numberText.flatMap { Int($0) },
            updatedMs: f["updatedMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}

public struct LiveSkillVersion: Equatable, Sendable, Identifiable {
    public var n: Int
    public var name: String
    public var atMs: UInt64
    public var id: Int { n }

    public init(n: Int, name: String = "", atMs: UInt64 = 0) {
        self.n = n
        self.name = name
        self.atMs = atMs
    }

    public static func decode(_ value: JSONValue) -> LiveSkillVersion? {
        let f = value.fields
        guard let n = f["n"]?.numberText.flatMap({ Int($0) }) else { return nil }
        return LiveSkillVersion(n: n, name: f["name"]?.stringValue ?? "",
                                atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}

/// What 编辑 changes (nil = unchanged).
public struct LiveSkillEdit: Equatable, Sendable {
    public var name: String?
    public var description: String?
    public var body: String?
    public var global: Bool?

    public init(name: String? = nil, description: String? = nil, body: String? = nil, global: Bool? = nil) {
        self.name = name
        self.description = description
        self.body = body
        self.global = global
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = [:]
        if let name { f["name"] = .string(name) }
        if let description { f["description"] = .string(description) }
        if let body { f["body"] = .string(body) }
        if let global { f["global"] = .bool(global) }
        return .object(f)
    }
}
