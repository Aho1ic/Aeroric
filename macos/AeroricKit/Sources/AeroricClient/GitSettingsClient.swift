import AeroricProtocol
import Foundation

/// 2026-11-24 设置 › Git 与工作副本 (`gitSettings.*`, `copies.list`).
public struct GitSettings: Sendable, Equatable {
    /// Added after the project's commit prompt when Aeroric writes a commit message.
    public var commitGuidance: String
    /// An isolated copy with nothing left to bring back goes this many days
    /// after it emptied; 0 keeps them.
    public var clearedCopyKeepDays: Int

    public init(commitGuidance: String = "", clearedCopyKeepDays: Int = 30) {
        self.commitGuidance = commitGuidance
        self.clearedCopyKeepDays = clearedCopyKeepDays
    }

    public static func decode(_ value: JSONValue) -> GitSettings {
        GitSettings(
            commitGuidance: value["commitGuidance"]?.stringValue ?? "",
            clearedCopyKeepDays: value["clearedCopyKeepDays"]?.numberText.flatMap { Int($0) } ?? 30)
    }

    var json: JSONValue {
        .object([
            "commitGuidance": .string(commitGuidance),
            "clearedCopyKeepDays": .number(String(clearedCopyKeepDays)),
        ])
    }
}

/// An isolated copy (R2-3b) as `copies.list` gives it.
public struct IsolatedCopy: Sendable, Equatable, Identifiable {
    public var taskID: String
    public var title: String?
    public var path: String
    /// The project's repository root.
    public var origin: String
    public var createdMs: Int64
    /// Set: nothing left to bring back, so it goes after the kept days.
    public var clearedMs: Int64?
    public var id: String { taskID }

    public init(taskID: String, title: String?, path: String, origin: String, createdMs: Int64, clearedMs: Int64?) {
        self.taskID = taskID
        self.title = title
        self.path = path
        self.origin = origin
        self.createdMs = createdMs
        self.clearedMs = clearedMs
    }

    public static func decodeList(_ value: JSONValue) -> [IsolatedCopy] {
        (value["copies"]?.arrayValue ?? []).compactMap { item in
            guard let taskID = item["taskId"]?.stringValue, let path = item["path"]?.stringValue else { return nil }
            return IsolatedCopy(
                taskID: taskID,
                title: item["title"]?.stringValue,
                path: path,
                origin: item["origin"]?.stringValue ?? "",
                createdMs: item["createdMs"]?.numberText.flatMap { Int64($0) } ?? 0,
                clearedMs: item["clearedMs"]?.numberText.flatMap { Int64($0) })
        }
    }
}

public struct GitSettingsClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func settings() async throws -> GitSettings {
        GitSettings.decode(try await connection.request("gitSettings.get", params: .object([:])).result)
    }

    /// Save; the host checks and answers with what it saved.
    public func save(_ settings: GitSettings) async throws -> GitSettings {
        GitSettings.decode(try await connection.request(
            "gitSettings.set", params: .object(["settings": settings.json])).result)
    }

    public func copies() async throws -> [IsolatedCopy] {
        IsolatedCopy.decodeList(try await connection.request("copies.list", params: .object([:])).result)
    }
}
