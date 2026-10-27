import AeroricProtocol
import Foundation

/// P5-04 Action 5：会话/审批的类型化客户端（宿主 `session.list`/
/// `session.history`/`session.export`/`approval.list`/`approval.respond`，
/// 复用 HostConnection.request——INV-02 归属：wire 解析只在 AeroricClient）。
public struct SessionsClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - session.list

    public struct SessionSummary: Sendable, Equatable {
        public var taskId: String
        public var projectId: String
        public var sessionPath: String
        public var sessionId: String?
        public var agent: String?

        public init(
            taskId: String, projectId: String, sessionPath: String,
            sessionId: String?, agent: String?
        ) {
            self.taskId = taskId
            self.projectId = projectId
            self.sessionPath = sessionPath
            self.sessionId = sessionId
            self.agent = agent
        }
    }

    /// session.list：宽容镜像解析（无 taskId 丢弃；空可选不上 store）。
    public func listSessions() async throws -> [SessionSummary] {
        let frame = try await connection.request("session.list", params: .null)
        guard case let .array(items)? = frame.result["sessions"] else { return [] }
        return items.compactMap { item in
            guard case let .object(object) = item,
                  case let .string(taskId)? = object["taskId"],
                  case let .string(projectId)? = object["projectId"],
                  case let .string(path)? = object["sessionPath"]
            else { return nil }
            let sessionId: String?
            if case let .string(value)? = object["sessionId"] { sessionId = value } else { sessionId = nil }
            let agent: String?
            if case let .string(value)? = object["agent"] { agent = value } else { agent = nil }
            return SessionSummary(
                taskId: taskId, projectId: projectId, sessionPath: path,
                sessionId: sessionId, agent: agent
            )
        }
    }

    // MARK: - session.history（消息分页；与 read_session_message_page 兼容）

    public enum MalformedReply: Error, Equatable {
        case malformedReply(String)
    }

    /// session.history（taskId 寻址）：返回 v1 契约的 SessionPage（协议层
    /// 解码）。G13：宿主按 task 持久化的 agent id 精确取根，客户端只传
    /// taskId，不做全量放行。
    public func historyPage(taskId: String, cursor: String?) async throws -> SessionPage {
        var params: [String: JSONValue] = ["taskId": .string(taskId)]
        if let cursor {
            params["cursor"] = .string(cursor)
        }
        let frame = try await connection.request("session.history", params: .object(params))
        guard let page = SessionPage.decode(frame.result) else {
            throw MalformedReply.malformedReply("session.history reply is not a SessionPage")
        }
        return page
    }

    // MARK: - session.export

    /// session.export：taskId + out（输出路径由用户指定，宿主校验输出路径）。
    public func exportSession(taskId: String, out: String) async throws {
        _ = try await connection.request(
            "session.export",
            params: .object(["taskId": .string(taskId), "out": .string(out)])
        )
    }

    // MARK: - approval.list / approval.respond

    public struct HostApproval: Sendable, Equatable {
        public var eventId: String
        public var kind: String
        public var sessionId: String?

        public init(eventId: String, kind: String, sessionId: String?) {
            self.eventId = eventId
            self.kind = kind
            self.sessionId = sessionId
        }
    }

    /// approval.list（host restart/断线恢复的权威查询）。
    public func listApprovals() async throws -> [HostApproval] {
        let frame = try await connection.request("approval.list", params: .null)
        guard case let .array(items)? = frame.result["approvals"] else { return [] }
        return items.compactMap { item in
            guard case let .object(object) = item,
                  case let .string(eventId)? = object["eventId"],
                  case let .string(kind)? = object["kind"]
            else { return nil }
            let sessionId: String?
            if case let .string(value)? = object["sessionId"] { sessionId = value } else { sessionId = nil }
            return HostApproval(eventId: eventId, kind: kind, sessionId: sessionId)
        }
    }

    /// approval.respond：回执 outcome（值即结果本身，无信封）。
    public func respondApproval(eventId: String, outcome: [String: JSONValue]) async throws {
        var params: [String: JSONValue] = ["eventId": .string(eventId)]
        params["outcome"] = .object(outcome)
        _ = try await connection.request("approval.respond", params: .object(params))
    }
}

// MARK: - Live structured sessions（ADR-043 N3，能力 `session.live`）

extension SessionsClient {
    public static let liveCapability = LiveSessionContract.capability

    public func prompt(taskId: String, text: String) async throws {
        _ = try await connection.request(LiveSessionContract.promptMethod,
                                         params: LiveSessionRequest.prompt(taskId: taskId, text: text))
    }

    public func interrupt(taskId: String) async throws {
        _ = try await connection.request(LiveSessionContract.interruptMethod,
                                         params: LiveSessionRequest.interrupt(taskId: taskId))
    }

    public func respond(taskId: String, requestId: String, decision: LiveDecision, reason: String? = nil) async throws {
        _ = try await connection.request(
            LiveSessionContract.respondMethod,
            params: LiveSessionRequest.respond(taskId: taskId, requestId: requestId, decision: decision, reason: reason)
        )
    }

    public func answer(taskId: String, questionId: String, value: String?, confirmed: Bool?, cancelled: Bool) async throws {
        _ = try await connection.request(
            LiveSessionContract.answerMethod,
            params: LiveSessionRequest.answer(taskId: taskId, questionId: questionId, value: value,
                                              confirmed: confirmed, cancelled: cancelled)
        )
    }

    public func revokeGrant(taskId: String, subject: String) async throws {
        _ = try await connection.request(LiveSessionContract.revokeGrantMethod,
                                         params: LiveSessionRequest.revokeGrant(taskId: taskId, subject: subject))
    }

    public func configure(taskId: String, model: String? = nil, permission: LivePermission? = nil,
                          effort: String? = nil) async throws {
        _ = try await connection.request(
            LiveSessionContract.configureMethod,
            params: LiveSessionRequest.configure(taskId: taskId, model: model, permission: permission, effort: effort)
        )
    }

    public func command(taskId: String, command: LiveCommand) async throws {
        _ = try await connection.request(LiveSessionContract.commandMethod,
                                         params: LiveSessionRequest.command(taskId: taskId, command: command))
    }

    public func close(taskId: String) async throws {
        _ = try await connection.request(LiveSessionContract.closeMethod,
                                         params: LiveSessionRequest.close(taskId: taskId))
    }

    /// A05: the task's project's auto-check choice (`session.autoCheck.get`).
    public func autoCheck(taskId: String) async throws -> (tests: Bool, diagnostics: Bool) {
        let frame = try await connection.request("session.autoCheck.get", params: .object(["taskId": .string(taskId)]))
        return try Self.decodeAutoCheck(frame.result)
    }

    public func setAutoCheck(taskId: String, tests: Bool, diagnostics: Bool) async throws {
        _ = try await connection.request("session.autoCheck.set", params: .object([
            "taskId": .string(taskId), "tests": .bool(tests), "diagnostics": .bool(diagnostics),
        ]))
    }

    /// Tasks whose latest auto-check run failed (`session.autoCheck.failures`):
    /// task id → (summary, finished at ms).
    public func autoCheckFailures() async throws -> [String: (summary: String, atMs: UInt64)] {
        let frame = try await connection.request("session.autoCheck.failures", params: .object([:]))
        return Self.decodeAutoCheckFailures(frame.result)
    }

    public static func decodeAutoCheckFailures(_ value: JSONValue) -> [String: (summary: String, atMs: UInt64)] {
        var result: [String: (summary: String, atMs: UInt64)] = [:]
        for row in value["failures"]?.arrayValue ?? [] {
            guard let task = row["taskId"]?.stringValue, let summary = row["summary"]?.stringValue else { continue }
            result[task] = (summary, row["atMs"].flatMap { UInt64($0.numberText ?? "") } ?? 0)
        }
        return result
    }

    public static func decodeAutoCheck(_ value: JSONValue) throws -> (tests: Bool, diagnostics: Bool) {
        guard let tests = value["tests"]?.boolValue, let diagnostics = value["diagnostics"]?.boolValue else {
            throw MalformedReply.malformedReply("session.autoCheck.get needs tests and diagnostics")
        }
        return (tests, diagnostics)
    }

    public func state(taskId: String) async throws -> LiveSessionState {
        let frame = try await connection.request(LiveSessionContract.stateMethod,
                                                 params: LiveSessionRequest.state(taskId: taskId))
        do {
            return try LiveSessionState.decode(frame.result)
        } catch let error as LiveSessionDecodeError {
            throw MalformedReply.malformedReply("session.state: \(error.reason)")
        }
    }

    public func updates(taskId: String, after: UInt64, limit: Int? = nil) async throws -> LiveUpdatesBatch {
        let frame = try await connection.request(LiveSessionContract.updatesMethod,
                                                 params: LiveSessionRequest.updates(taskId: taskId, after: after, limit: limit))
        do {
            return try LiveUpdatesBatch.decode(frame.result)
        } catch let error as LiveSessionDecodeError {
            throw MalformedReply.malformedReply("session.updates: \(error.reason)")
        }
    }
}
