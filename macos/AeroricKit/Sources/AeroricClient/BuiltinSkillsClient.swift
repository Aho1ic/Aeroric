import AeroricProtocol
import Foundation

/// `builtinSkills.*` (B2 工作配方库, 2026-10-30): the built-in skills, their
/// switches and the project copies a direct Codex session leaves.
public struct BuiltinSkillsClient: Sendable {
    public struct Skill: Sendable, Equatable {
        public var id: String
        public var group: String
        public var title: String
        public var detail: String
        public var symbol: String
        public var needs: [String]
        public var enabled: Bool
        public var shadowedBy: [String]
        public var missing: [String]
    }

    public struct ProjectCopies: Sendable, Equatable {
        public var path: String
        public var name: String
        public var count: Int
    }

    public enum MalformedReply: Error { case malformed(String) }

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func list(projectPath: String? = nil) async throws -> [Skill] {
        var params: [String: JSONValue] = [:]
        if let projectPath { params["projectPath"] = .string(projectPath) }
        let frame = try await connection.request("builtinSkills.list", params: .object(params))
        return try Self.decodeSkills(frame.result)
    }

    public func setEnabled(id: String, enabled: Bool) async throws {
        _ = try await connection.request("builtinSkills.setEnabled",
                                         params: .object(["id": .string(id), "enabled": .bool(enabled)]))
    }

    public func projectsWithCopies() async throws -> [ProjectCopies] {
        let frame = try await connection.request("builtinSkills.projectsWithCopies", params: .object([:]))
        return try Self.decodeProjects(frame.result)
    }

    /// Moves Aeroric's copies in the project to the Trash; returns how many.
    public func removeFromProject(projectPath: String) async throws -> Int {
        let frame = try await connection.request("builtinSkills.removeFromProject",
                                                 params: .object(["projectPath": .string(projectPath)]))
        return Int(frame.result["removed"]?.numberText ?? "") ?? 0
    }

    public static func decodeSkills(_ value: JSONValue) throws -> [Skill] {
        guard let items = value["skills"]?.arrayValue else { throw MalformedReply.malformed("builtinSkills.list returns skills") }
        let strings = { (item: JSONValue, key: String) in
            (item[key]?.arrayValue ?? []).compactMap(\.stringValue)
        }
        return try items.map { item in
            guard let id = item["id"]?.stringValue, let title = item["title"]?.stringValue else {
                throw MalformedReply.malformed("a skill needs id and title")
            }
            return Skill(id: id, group: item["group"]?.stringValue ?? "develop", title: title,
                         detail: item["detail"]?.stringValue ?? "", symbol: item["symbol"]?.stringValue ?? "sparkles",
                         needs: strings(item, "needs"), enabled: item["enabled"]?.boolValue ?? true,
                         shadowedBy: strings(item, "shadowedBy"), missing: strings(item, "missing"))
        }
    }

    public static func decodeProjects(_ value: JSONValue) throws -> [ProjectCopies] {
        guard let items = value["projects"]?.arrayValue else {
            throw MalformedReply.malformed("builtinSkills.projectsWithCopies returns projects")
        }
        return items.compactMap { item in
            guard let path = item["path"]?.stringValue else { return nil }
            return ProjectCopies(path: path, name: item["name"]?.stringValue ?? path,
                                 count: Int(item["count"]?.numberText ?? "") ?? 0)
        }
    }
}
