import AeroricProtocol
import Foundation

/// Git queries and the Git page's first batch of writes (2026-10-30: stage,
/// commit, branches, stash, discarding one file). Push and pull are not here
/// yet.
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

    public struct StashEntry: Sendable, Equatable {
        public var index: Int
        public var name: String
        public var message: String
    }

    public struct RemoteCounts: Sendable, Equatable {
        public var ahead: Int
        public var behind: Int
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

    /// D: switch the project's working copy to `branch` (`git checkout`).
    /// Callers check the working tree is clean and confirm with the user first.
    /// `isRemote` is required by the host; a local branch until remote
    /// branches are offered.
    public func checkoutBranch(projectPath: String, branch: String) async throws {
        try await call("git.git_checkout_branch", projectPath, [
            "branchName": .string(branch), "isRemote": .bool(false),
        ])
    }

    public func stage(projectPath: String, filePath: String) async throws {
        try await call("git.git_stage", projectPath, ["filePath": .string(filePath)])
    }

    public func unstage(projectPath: String, filePath: String) async throws {
        try await call("git.git_unstage", projectPath, ["filePath": .string(filePath)])
    }

    public func stageAll(projectPath: String) async throws {
        try await call("git.git_stage_all", projectPath)
    }

    public func unstageAll(projectPath: String) async throws {
        try await call("git.git_unstage_all", projectPath)
    }

    public func commit(projectPath: String, message: String) async throws {
        try await call("git.git_commit", projectPath, ["message": .string(message)])
    }

    /// Written by the user's Codex from the staged diff; fails when nothing is staged.
    public func generateCommitMessage(projectPath: String) async throws -> String {
        let frame = try await connection.request("git.generate_commit_message",
                                                 params: .object(["projectPath": .string(projectPath)]))
        guard let text = frame.result.stringValue else {
            throw MalformedReply.malformed("git.generate_commit_message returns text")
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Creates `name` from `from` and switches to it.
    public func createBranch(projectPath: String, name: String, from: String) async throws {
        try await call("git.git_create_branch", projectPath, [
            "branchName": .string(name), "fromBranch": .string(from), "checkout": .bool(true),
        ])
    }

    /// One file back to its committed state; an untracked file is removed.
    public func discard(projectPath: String, filePath: String, untracked: Bool) async throws {
        try await call("git.git_discard_files", projectPath, [
            "filePaths": .array([.string(filePath)]), "untracked": .bool(untracked),
        ])
    }

    public func stashPush(projectPath: String, message: String?) async throws {
        var params: [String: JSONValue] = ["includeUntracked": .bool(true)]
        if let message, !message.isEmpty { params["message"] = .string(message) }
        try await call("git.git_stash_push", projectPath, params)
    }

    public func stashList(projectPath: String) async throws -> [StashEntry] {
        let frame = try await connection.request("git.git_stash_list", params: .object(["projectPath": .string(projectPath)]))
        return try Self.decodeStash(frame.result)
    }

    public func stashApply(projectPath: String, name: String) async throws {
        try await call("git.git_stash_apply", projectPath, ["stashRef": .string(name)])
    }

    public func stashDrop(projectPath: String, name: String) async throws {
        try await call("git.git_stash_drop", projectPath, ["stashRef": .string(name)])
    }

    /// Commits ahead of and behind the upstream; nil without an upstream.
    public func remoteCounts(projectPath: String) async throws -> RemoteCounts? {
        let frame = try await connection.request("git.git_remote_counts", params: .object(["projectPath": .string(projectPath)]))
        return Self.decodeCounts(frame.result)
    }

    private func call(_ method: String, _ projectPath: String, _ extra: [String: JSONValue] = [:]) async throws {
        var params = extra
        params["projectPath"] = .string(projectPath)
        _ = try await connection.request(method, params: .object(params))
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

    public static func decodeStash(_ value: JSONValue) throws -> [StashEntry] {
        guard let items = value.arrayValue else { throw MalformedReply.malformed("git.git_stash_list returns a list") }
        return try items.map { item in
            guard let index = Int(item["index"]?.numberText ?? ""), let name = item["name"]?.stringValue else {
                throw MalformedReply.malformed("a stash entry needs index and name")
            }
            return StashEntry(index: index, name: name, message: item["message"]?.stringValue ?? "")
        }
    }

    public static func decodeCounts(_ value: JSONValue) -> RemoteCounts? {
        guard let ahead = Int(value["ahead"]?.numberText ?? ""), let behind = Int(value["behind"]?.numberText ?? "") else { return nil }
        return RemoteCounts(ahead: ahead, behind: behind)
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
