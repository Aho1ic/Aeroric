import AeroricProtocol
import Foundation

/// Read-only client boundary. Project paths come from an authorized project
/// snapshot, never from terminal output or a UI filesystem scan.
public protocol ProjectResourceReading: Sendable {
    func directory(projectID: String, path: String) async throws -> [FilesClient.FileEntry]
    func text(projectID: String, path: String) async throws -> String
    func changes(projectPath: String) async throws -> [ResourceGitChange]
    func branches(projectPath: String) async throws -> [ResourceGitBranch]
    func diff(projectPath: String, path: String, staged: Bool) async throws -> String
}

public struct ResourceGitChange: Identifiable, Equatable, Hashable, Sendable {
    public var id: String { "\(staged):\(path)" }
    public let path: String
    public let status: String
    public let staged: Bool
    public init(path: String, status: String, staged: Bool) {
        self.path = path; self.status = status; self.staged = staged
    }
}

public struct ResourceGitBranch: Equatable, Sendable {
    public let name: String
    public let current: Bool
    public init(name: String, current: Bool) { self.name = name; self.current = current }
}

public struct ProjectResourcesClient: ProjectResourceReading {
    public enum ReadError: LocalizedError, Equatable {
        case malformed(String)
        public var errorDescription: String? {
            switch self {
            case let .malformed(method): "\(method) 返回结构不完整，不能当作空快照。"
            }
        }
    }

    private let connection: HostConnection
    public init(connection: HostConnection) { self.connection = connection }

    public func directory(projectID: String, path: String) async throws -> [FilesClient.FileEntry] {
        try await FilesClient(connection: connection).readDir(projectId: projectID, path: path)
    }
    public func text(projectID: String, path: String) async throws -> String {
        try await FilesClient(connection: connection).readFile(projectId: projectID, path: path)
    }
    public func changes(projectPath: String) async throws -> [ResourceGitChange] {
        let reply = try await connection.request("git.git_status", params: .object(["projectPath": .string(projectPath)]))
        return try Self.decodeChanges(reply.result)
    }
    public func branches(projectPath: String) async throws -> [ResourceGitBranch] {
        let reply = try await connection.request("git.git_list_branches", params: .object(["projectPath": .string(projectPath)]))
        return try Self.decodeBranches(reply.result)
    }
    public func diff(projectPath: String, path: String, staged: Bool) async throws -> String {
        let params = Self.diffParameters(projectPath: projectPath, path: path, staged: staged)
        let reply = try await connection.request("git.git_file_diff", params: params)
        return try Self.decodeDiff(reply.result)
    }
    static func diffParameters(projectPath: String, path: String, staged: Bool) -> JSONValue {
        .object(["projectPath": .string(projectPath), "filePath": .string(path), "staged": .bool(staged)])
    }

    static func decodeChanges(_ result: JSONValue) throws -> [ResourceGitChange] {
        guard case let .array(rows) = result else { throw ReadError.malformed("git.git_status") }
        return try rows.map { row in
            guard let path = row["path"]?.stringValue, !path.isEmpty,
                  let status = row["status"]?.stringValue, !status.isEmpty,
                  let staged = row["staged"]?.boolValue else { throw ReadError.malformed("git.git_status") }
            return ResourceGitChange(path: path, status: status, staged: staged)
        }
    }
    static func decodeBranches(_ result: JSONValue) throws -> [ResourceGitBranch] {
        guard case let .array(rows) = result else { throw ReadError.malformed("git.git_list_branches") }
        return try rows.map { row in
            guard let name = row["name"]?.stringValue,
                  let current = row["current"]?.boolValue else { throw ReadError.malformed("git.git_list_branches") }
            return ResourceGitBranch(name: name, current: current)
        }
    }
    static func decodeDiff(_ result: JSONValue) throws -> String {
        guard case let .string(text) = result else { throw ReadError.malformed("git.git_file_diff") }
        return text
    }
}
