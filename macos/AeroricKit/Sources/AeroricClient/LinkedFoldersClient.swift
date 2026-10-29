import AeroricProtocol
import Foundation

/// `project.linked_folders` / `link_folder` / `unlink_folder` (2026-10-29).
public struct LinkedFolder: Equatable, Sendable, Identifiable {
    public var id: String { path }
    public var path: String
    public var name: String
    /// False when the folder was moved or deleted (kept, skipped at launch).
    public var exists: Bool

    public init(path: String, name: String, exists: Bool) {
        self.path = path
        self.name = name
        self.exists = exists
    }

    static func decodeList(_ value: JSONValue?) -> [LinkedFolder] {
        (value?.arrayValue ?? []).compactMap { item in
            guard let path = item["path"]?.stringValue else { return nil }
            return LinkedFolder(path: path, name: item["name"]?.stringValue ?? path,
                                exists: item["exists"]?.boolValue ?? false)
        }
    }
}

extension WorkspaceClient {
    /// Project id → its linked folders (projects without links are absent).
    public func linkedFolders() async throws -> [String: [LinkedFolder]] {
        let frame = try await connection.request("project.linked_folders", params: .object([:]))
        var out: [String: [LinkedFolder]] = [:]
        for (project, rows) in frame.result["projects"]?.objectValue ?? [:] {
            out[project] = LinkedFolder.decodeList(rows)
        }
        return out
    }

    /// Throws `.refused` with the host's reason (inside the project, duplicate, …).
    public func linkFolder(projectId: String, path: String) async throws -> [LinkedFolder] {
        let frame = try await connection.request(
            "project.link_folder", params: .object(["projectId": .string(projectId), "path": .string(path)]))
        return LinkedFolder.decodeList(frame.result["folders"])
    }

    public func unlinkFolder(projectId: String, path: String) async throws -> [LinkedFolder] {
        let frame = try await connection.request(
            "project.unlink_folder", params: .object(["projectId": .string(projectId), "path": .string(path)]))
        return LinkedFolder.decodeList(frame.result["folders"])
    }
}
