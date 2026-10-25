import AeroricProtocol
import Foundation

/// Read-only Git queries for the Git page (B1 tool-page wiring). Writes
/// (stage, commit, push, …) are deliberately not here yet.
public struct GitClient: Sendable {
    public struct FileStatus: Sendable, Equatable {
        public var path: String
        public var status: String
        public var staged: Bool
    }

    public struct Branch: Sendable, Equatable {
        public var name: String
        public var current: Bool
        public var remote: String?
    }

    public struct Commit: Sendable, Equatable {
        public var hash: String
        public var shortHash: String
        public var author: String
        public var date: String
        public var message: String
    }

    public enum MalformedReply: Error { case malformed(String) }

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func status(projectPath: String) async throws -> [FileStatus] {
        let frame = try await connection.request("git.git_status", params: .object(["projectPath": .string(projectPath)]))
        return try Self.decodeStatus(frame.result)
    }

    public func branches(projectPath: String) async throws -> [Branch] {
        let frame = try await connection.request("git.git_list_branches", params: .object(["projectPath": .string(projectPath)]))
        return try Self.decodeBranches(frame.result)
    }

    public func log(projectPath: String, limit: Int = 100) async throws -> [Commit] {
        let frame = try await connection.request("git.git_log", params: .object([
            "projectPath": .string(projectPath), "limit": .number(String(limit)),
        ]))
        return try Self.decodeLog(frame.result)
    }

    public func fileDiff(projectPath: String, filePath: String, staged: Bool) async throws -> String {
        let frame = try await connection.request("git.git_file_diff", params: .object([
            "projectPath": .string(projectPath), "filePath": .string(filePath), "staged": .bool(staged),
        ]))
        guard let text = frame.result.stringValue else { throw MalformedReply.malformed("git.git_file_diff returns text") }
        return text
    }

    public static func decodeStatus(_ value: JSONValue) throws -> [FileStatus] {
        guard let items = value.arrayValue else { throw MalformedReply.malformed("git.git_status returns a list") }
        return try items.map { item in
            guard let path = item["path"]?.stringValue, let status = item["status"]?.stringValue,
                  let staged = item["staged"]?.boolValue else {
                throw MalformedReply.malformed("a status row needs path, status and staged")
            }
            return FileStatus(path: path, status: status, staged: staged)
        }
    }

    public static func decodeBranches(_ value: JSONValue) throws -> [Branch] {
        guard let items = value.arrayValue else { throw MalformedReply.malformed("git.git_list_branches returns a list") }
        return try items.map { item in
            guard let name = item["name"]?.stringValue, let current = item["current"]?.boolValue else {
                throw MalformedReply.malformed("a branch needs name and current")
            }
            return Branch(name: name, current: current, remote: item["remote"]?.stringValue)
        }
    }

    public static func decodeLog(_ value: JSONValue) throws -> [Commit] {
        guard let items = value.arrayValue else { throw MalformedReply.malformed("git.git_log returns a list") }
        return try items.map { item in
            guard let hash = item["hash"]?.stringValue, let message = item["message"]?.stringValue else {
                throw MalformedReply.malformed("a commit needs hash and message")
            }
            return Commit(hash: hash, shortHash: item["short_hash"]?.stringValue ?? String(hash.prefix(7)),
                          author: item["author"]?.stringValue ?? "", date: item["date"]?.stringValue ?? "",
                          message: message)
        }
    }
}
