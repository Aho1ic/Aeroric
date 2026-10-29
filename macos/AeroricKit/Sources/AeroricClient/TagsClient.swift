import AeroricProtocol
import Foundation

/// `tags.*` (2026-10-29): the project and session tag library.
public struct TagRecord: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    /// A `ProjectColor` id.
    public var color: String
    /// Set for a 仅当前项目 tag.
    public var projectId: String?

    public init(id: String, name: String, color: String, projectId: String? = nil) {
        self.id = id
        self.name = name
        self.color = color
        self.projectId = projectId
    }

    static func decode(_ value: JSONValue) -> TagRecord? {
        guard let id = value["id"]?.stringValue, let name = value["name"]?.stringValue else { return nil }
        return TagRecord(id: id, name: name, color: value["color"]?.stringValue ?? "blue",
                         projectId: value["projectId"]?.stringValue)
    }
}

public struct TagLibrary: Equatable, Sendable {
    public var tags: [TagRecord]
    /// Sidebar row id → tag ids.
    public var assignments: [String: [String]]
    /// Sidebar row id → the project it sat in when tagged.
    public var rowProjects: [String: String]

    public init(tags: [TagRecord] = [], assignments: [String: [String]] = [:], rowProjects: [String: String] = [:]) {
        self.tags = tags
        self.assignments = assignments
        self.rowProjects = rowProjects
    }
}

extension WorkspaceClient {
    public func tagLibrary() async throws -> TagLibrary {
        let frame = try await connection.request("tags.list", params: .object([:]))
        let tags = (frame.result["tags"]?.arrayValue ?? []).compactMap(TagRecord.decode)
        var assignments: [String: [String]] = [:]
        for (row, ids) in frame.result["assignments"]?.objectValue ?? [:] {
            assignments[row] = (ids.arrayValue ?? []).compactMap(\.stringValue)
        }
        var rowProjects: [String: String] = [:]
        for (row, project) in frame.result["rowProjects"]?.objectValue ?? [:] {
            if let project = project.stringValue { rowProjects[row] = project }
        }
        return TagLibrary(tags: tags, assignments: assignments, rowProjects: rowProjects)
    }

    public func createTag(name: String, color: String, projectId: String?) async throws -> TagRecord {
        var params: [String: JSONValue] = ["name": .string(name), "color": .string(color)]
        if let projectId { params["projectId"] = .string(projectId) }
        let frame = try await connection.request("tags.create", params: .object(params))
        guard let tag = TagRecord.decode(frame.result) else { throw ConnectionError.io("tags.create: unexpected reply") }
        return tag
    }

    /// `scope`: `.some(nil)` = 所有会话, `.some(projectId)` = 仅那个项目.
    public func updateTag(id: String, name: String? = nil, color: String? = nil,
                          scope: String?? = nil) async throws {
        var params: [String: JSONValue] = ["id": .string(id)]
        if let name { params["name"] = .string(name) }
        if let color { params["color"] = .string(color) }
        if let scope { params["scope"] = .string(scope ?? "all") }
        _ = try await connection.request("tags.update", params: .object(params))
    }

    public func deleteTag(id: String) async throws {
        _ = try await connection.request("tags.delete", params: .object(["id": .string(id)]))
    }

    /// Rows take 所有会话 tags and their own project's (`projectId`; a
    /// project row passes its own id).
    public func assignTags(target: String, isProject: Bool, projectId: String?, tagIds: [String]) async throws {
        var params: [String: JSONValue] = [
            "target": .string(target), "kind": .string(isProject ? "project" : "session"),
            "tagIds": .array(tagIds.map(JSONValue.string)),
        ]
        if let projectId { params["projectId"] = .string(projectId) }
        _ = try await connection.request("tags.assign", params: .object(params))
    }
}
