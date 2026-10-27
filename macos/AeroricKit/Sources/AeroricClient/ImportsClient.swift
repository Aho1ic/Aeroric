import AeroricProtocol
import Foundation

/// 已有会话导入（UI 改版 M4）的类型化客户端：宿主 `imports.*` 与 `agents.detect`。
/// 能力门控名 `imports`；没有该能力的宿主不调用这些方法。
public struct ImportsClient: Sendable {
    public static let capability = "imports"
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - Models

    public struct AgentStatus: Sendable, Equatable, Identifiable {
        public var agent: String
        public var granted: Bool
        public var root: String
        public var grantedAtMs: UInt64?
        public var sessions: Int
        public var id: String { agent }
    }

    public struct Status: Sendable, Equatable {
        public var agents: [AgentStatus]
        public var lastScanAtMs: UInt64?
        public var lastError: String?
    }

    public struct Estimate: Sendable, Equatable {
        public var agent: String
        public var root: String
        public var rootExists: Bool
        public var sessions: Int
        public var bytes: UInt64

        public init(agent: String, root: String, rootExists: Bool, sessions: Int, bytes: UInt64) {
            self.agent = agent
            self.root = root
            self.rootExists = rootExists
            self.sessions = sessions
            self.bytes = bytes
        }
    }

    public struct ImportedSession: Sendable, Equatable, Identifiable {
        public var id: String
        public var agent: String
        public var cwd: String?
        public var title: String
        public var lastAtMs: UInt64
        public var projectId: String?
        public var tooLarge: Bool
        /// Hidden from the projects and 未归类, listed under 已归档.
        public var archived: Bool

        public init(id: String, agent: String, cwd: String?, title: String, lastAtMs: UInt64,
                    projectId: String?, tooLarge: Bool, archived: Bool = false) {
            self.id = id
            self.agent = agent
            self.cwd = cwd
            self.title = title
            self.lastAtMs = lastAtMs
            self.projectId = projectId
            self.tooLarge = tooLarge
            self.archived = archived
        }
    }

    /// One row of the import picker: a session on disk and whether it starts
    /// ticked (everything not left out last time).
    public struct PreviewSession: Sendable, Equatable, Identifiable {
        public var session: ImportedSession
        public var selected: Bool
        public var id: String { session.id }

        public init(session: ImportedSession, selected: Bool) {
            self.session = session
            self.selected = selected
        }
    }

    public struct ApplyResult: Sendable, Equatable {
        public var imported: Int
        public var createdProjects: [String]
        public var failedProjects: [String]
    }

    public struct DetectedAgent: Sendable, Equatable, Identifiable {
        public var agent: String
        public var installed: Bool
        public var path: String?
        public var id: String { agent }
    }

    public enum MalformedReply: Error, Equatable {
        case malformed(String)
    }

    // MARK: - Calls

    public func status() async throws -> Status {
        Self.decodeStatus(try await connection.request("imports.status", params: .object([:])).result)
    }

    public func estimate(agent: String) async throws -> Estimate {
        let frame = try await connection.request("imports.estimate", params: .object(["agent": .string(agent)]))
        guard let estimate = Self.decodeEstimate(frame.result) else { throw MalformedReply.malformed("imports.estimate") }
        return estimate
    }

    public func grant(agent: String) async throws {
        _ = try await connection.request("imports.grant", params: .object(["agent": .string(agent)]))
    }

    public func revoke(agent: String) async throws {
        _ = try await connection.request("imports.revoke", params: .object(["agent": .string(agent)]))
    }

    public func rescan() async throws {
        _ = try await connection.request("imports.rescan", params: .object([:]))
    }

    public func list() async throws -> [ImportedSession] {
        Self.decodeSessions(try await connection.request("imports.list", params: .object([:])).result)
    }

    /// Every session on disk for the import picker; saves nothing.
    public func preview() async throws -> [PreviewSession] {
        Self.decodePreview(try await connection.request("imports.preview", params: .object([:])).result)
    }

    /// The picker's answer: imports `selected`, adds their missing projects.
    public func apply(selected: [String]) async throws -> ApplyResult {
        let frame = try await connection.request(
            "imports.apply", params: .object(["selected": .array(selected.map(JSONValue.string))]))
        let fields = frame.result.fields
        return ApplyResult(imported: Int(Self.number(fields["imported"]) ?? 0),
                           createdProjects: (fields["createdProjects"]?.arrayValue ?? []).compactMap(\.stringValue),
                           failedProjects: (fields["failedProjects"]?.arrayValue ?? []).compactMap(\.stringValue))
    }

    public func archive(id: String, archived: Bool) async throws {
        _ = try await connection.request(
            "imports.archive", params: .object(["id": .string(id), "archived": .bool(archived)]))
    }

    /// Hides the session in Aeroric; the agent's own file is never touched.
    public func remove(id: String) async throws {
        _ = try await connection.request("imports.remove", params: .object(["id": .string(id)]))
    }

    /// One page of history, newest first: the messages before `before`
    /// (the previous page's `nextCursor`); nil asks for the latest page.
    public func history(id: String, before: String? = nil) async throws -> SessionPage {
        var params: [String: JSONValue] = ["id": .string(id)]
        if let before { params["before"] = .string(before) }
        let frame = try await connection.request("imports.history", params: .object(params))
        guard let page = SessionPage.decode(frame.result) else { throw MalformedReply.malformed("imports.history") }
        return page
    }

    public func detectAgents() async throws -> [DetectedAgent] {
        Self.decodeDetected(try await connection.request("agents.detect", params: .object([:])).result)
    }

    // MARK: - Decoding (lenient: unknown rows are dropped, not fatal)

    static func number(_ value: JSONValue?) -> UInt64? {
        if case let .number(text)? = value { return UInt64(text) ?? Double(text).map { UInt64(max(0, $0)) } }
        return nil
    }

    public static func decodeStatus(_ value: JSONValue) -> Status {
        let agents = (value.fields["agents"]?.arrayValue ?? []).compactMap { item -> AgentStatus? in
            let row = item.fields
            guard let agent = row["agent"]?.stringValue else { return nil }
            return AgentStatus(agent: agent, granted: row["granted"]?.boolValue ?? false,
                               root: row["root"]?.stringValue ?? "", grantedAtMs: number(row["grantedAtMs"]),
                               sessions: Int(number(row["sessions"]) ?? 0))
        }
        return Status(agents: agents, lastScanAtMs: number(value.fields["lastScanAtMs"]),
                      lastError: value.fields["lastError"]?.stringValue)
    }

    public static func decodeEstimate(_ value: JSONValue) -> Estimate? {
        let fields = value.fields
        guard let agent = fields["agent"]?.stringValue else { return nil }
        let estimate = fields["estimate"]?.fields ?? [:]
        return Estimate(agent: agent, root: fields["root"]?.stringValue ?? "",
                        rootExists: estimate["rootExists"]?.boolValue ?? false,
                        sessions: Int(number(estimate["sessions"]) ?? 0), bytes: number(estimate["bytes"]) ?? 0)
    }

    public static func decodeSessions(_ value: JSONValue) -> [ImportedSession] {
        (value.fields["sessions"]?.arrayValue ?? []).compactMap(decodeSession)
    }

    static func decodeSession(_ item: JSONValue) -> ImportedSession? {
        let row = item.fields
        guard let id = row["id"]?.stringValue, let agent = row["agent"]?.stringValue else { return nil }
        return ImportedSession(id: id, agent: agent, cwd: row["cwd"]?.stringValue,
                               title: row["title"]?.stringValue ?? id, lastAtMs: number(row["lastAtMs"]) ?? 0,
                               projectId: row["projectId"]?.stringValue,
                               tooLarge: row["tooLarge"]?.boolValue ?? false,
                               archived: row["archived"]?.boolValue ?? false)
    }

    /// `PreviewSession` is the session flattened plus `selected`.
    public static func decodePreview(_ value: JSONValue) -> [PreviewSession] {
        (value.fields["sessions"]?.arrayValue ?? []).compactMap { item in
            decodeSession(item).map { PreviewSession(session: $0, selected: item.fields["selected"]?.boolValue ?? true) }
        }
    }

    public static func decodeDetected(_ value: JSONValue) -> [DetectedAgent] {
        (value.fields["agents"]?.arrayValue ?? []).compactMap { item in
            let row = item.fields
            guard let agent = row["agent"]?.stringValue else { return nil }
            return DetectedAgent(agent: agent, installed: row["installed"]?.boolValue ?? false,
                                 path: row["path"]?.stringValue)
        }
    }
}
