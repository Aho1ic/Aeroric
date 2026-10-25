import Foundation

/// Mirrors `aeroric_protocol::session_live` (contracts/native/v1/session-live.schema.json):
/// live structured agent sessions (ADR-043). Host-local, over the authenticated UDS.
/// Updates are read by cursor polling (`session.updates {after}`); an update whose
/// `type` this build does not know — or a known one with fields it cannot read —
/// is kept as `.unknown`, never dropped and never a failed batch.
public enum LiveSessionContract {
    public static let capability = "session.live"
    public static let promptMethod = "session.prompt"
    public static let interruptMethod = "session.interrupt"
    public static let respondMethod = "session.respond"
    public static let configureMethod = "session.configure"
    public static let commandMethod = "session.command"
    public static let closeMethod = "session.close"
    public static let stateMethod = "session.state"
    public static let updatesMethod = "session.updates"
    public static let defaultBatchLimit = 500
}

public enum LiveTurnEndReason: String, Equatable, Sendable {
    case completed, interrupted, failed
}

public enum LiveToolKind: String, Equatable, Sendable {
    case command, fileChange, mcp, webSearch, subagent, other
}

public enum LiveToolStatus: String, Equatable, Sendable {
    case running, completed, failed, declined
}

public struct LivePlanEntry: Equatable, Sendable {
    public var text: String
    /// `pending` | `inProgress` | `completed` (the agent's own vocabulary).
    public var status: String

    public init(text: String, status: String) {
        self.text = text
        self.status = status
    }
}

public enum LiveUpdateKind: Equatable, Sendable {
    case turnStarted(turnId: String)
    case turnEnded(turnId: String, reason: LiveTurnEndReason, error: String?)
    case messageChunk(messageId: String, role: String, text: String)
    case messageCompleted(messageId: String, text: String?)
    case thoughtChunk(id: String, text: String, summary: Bool)
    case toolCall(id: String, kind: LiveToolKind, title: String, input: JSONValue)
    case toolCallUpdate(id: String, status: LiveToolStatus, outputDelta: String?, result: JSONValue?)
    case plan([LivePlanEntry])
    case permissionRequest(id: String, toolCallId: String?, kind: String, summary: String, options: [String])
    case permissionResolved(id: String, decision: String)
    case subagent(id: String, title: String, status: String)
    case usage(contextUsed: UInt64?, contextLimit: UInt64?, inputTokens: UInt64?, outputTokens: UInt64?)
    case configChanged(model: String?, permission: String?, effort: String?)
    case error(message: String, recoverable: Bool)
    /// Kept with its source `type` (or the agent's own kind for host-side unknowns).
    case unknown(kind: String, payload: JSONValue)
}

public struct LiveSessionUpdate: Equatable, Sendable {
    public var taskId: String
    public var seq: UInt64
    public var atMs: UInt64
    public var kind: LiveUpdateKind

    public init(taskId: String, seq: UInt64, atMs: UInt64, kind: LiveUpdateKind) {
        self.taskId = taskId
        self.seq = seq
        self.atMs = atMs
        self.kind = kind
    }
}

public struct LivePendingPermission: Equatable, Sendable {
    public var id: String
    public var kind: String
    public var summary: String
    public var toolCallId: String?
}

public struct LiveSessionState: Equatable, Sendable {
    public var taskId: String
    public var live: Bool
    public var family: String?
    public var sessionId: String?
    public var turnActive: Bool
    public var pending: [LivePendingPermission]
    public var plan: [LivePlanEntry]
    public var model: String?
    public var permission: String?
    public var effort: String?
    public var contextUsed: UInt64?
    public var contextLimit: UInt64?
    public var inputTokens: UInt64?
    public var outputTokens: UInt64?
    public var latestSeq: UInt64
}

public struct LiveUpdatesBatch: Equatable, Sendable {
    public var updates: [LiveSessionUpdate]
    public var latestSeq: UInt64
    /// Set when the cursor is older than the host still holds (or belongs to an
    /// earlier log): re-read history and `session.state`, then continue from `latestSeq`.
    public var gapOldestAvailable: UInt64?
}

public enum LiveDecision: String, Equatable, Sendable {
    case allowOnce, allowForSession, deny
}

public enum LivePermission: String, Equatable, Sendable {
    case ask, autoEdit = "auto_edit", fullAccess = "full_access"
}

public enum LiveCommand: String, Equatable, Sendable {
    case compact, review
}

public struct LiveSessionDecodeError: Error, Equatable, Sendable {
    public let reason: String
    public init(_ reason: String) { self.reason = reason }
}

// MARK: - Decoding

private func string(_ fields: [String: JSONValue], _ key: String) -> String? { fields[key]?.stringValue }
private func uint(_ value: JSONValue?) -> UInt64? { value?.numberText.flatMap { UInt64($0) } }

extension LivePlanEntry {
    static func decode(_ value: JSONValue) -> LivePlanEntry? {
        let fields = value.fields
        guard let text = string(fields, "text"), let status = string(fields, "status") else { return nil }
        return LivePlanEntry(text: text, status: status)
    }
}

extension LiveSessionUpdate {
    public static func decode(_ value: JSONValue) throws -> LiveSessionUpdate {
        let fields = value.fields
        guard let taskId = string(fields, "taskId"), let seq = uint(fields["seq"]), let atMs = uint(fields["atMs"]),
              let type = string(fields, "type")
        else { throw LiveSessionDecodeError("an update needs taskId, seq, atMs and type") }
        return LiveSessionUpdate(taskId: taskId, seq: seq, atMs: atMs, kind: LiveUpdateKind.decode(type: type, fields))
    }
}

extension LiveUpdateKind {
    static func decode(type: String, _ f: [String: JSONValue]) -> LiveUpdateKind {
        known(type: type, f) ?? .unknown(kind: type, payload: .object(f))
    }

    private static func known(type: String, _ f: [String: JSONValue]) -> LiveUpdateKind? {
        switch type {
        case "turnStarted":
            return string(f, "turnId").map { .turnStarted(turnId: $0) }
        case "turnEnded":
            guard let id = string(f, "turnId"), let reason = string(f, "reason").flatMap(LiveTurnEndReason.init) else { return nil }
            return .turnEnded(turnId: id, reason: reason, error: string(f, "error"))
        case "messageChunk":
            guard let id = string(f, "messageId"), let role = string(f, "role"), let text = string(f, "text") else { return nil }
            return .messageChunk(messageId: id, role: role, text: text)
        case "messageCompleted":
            return string(f, "messageId").map { .messageCompleted(messageId: $0, text: string(f, "text")) }
        case "thoughtChunk":
            guard let id = string(f, "id"), let text = string(f, "text"), let summary = f["summary"]?.boolValue else { return nil }
            return .thoughtChunk(id: id, text: text, summary: summary)
        case "toolCall":
            guard let id = string(f, "id"), let kind = string(f, "kind").flatMap(LiveToolKind.init),
                  let title = string(f, "title") else { return nil }
            return .toolCall(id: id, kind: kind, title: title, input: f["input"] ?? .null)
        case "toolCallUpdate":
            guard let id = string(f, "id"), let status = string(f, "status").flatMap(LiveToolStatus.init) else { return nil }
            return .toolCallUpdate(id: id, status: status, outputDelta: string(f, "outputDelta"), result: f["result"])
        case "plan":
            guard let entries = f["entries"]?.arrayValue else { return nil }
            let decoded = entries.compactMap(LivePlanEntry.decode)
            return decoded.count == entries.count ? .plan(decoded) : nil
        case "permissionRequest":
            guard let id = string(f, "id"), let kind = string(f, "kind"), let summary = string(f, "summary"),
                  let options = f["options"]?.arrayValue?.compactMap(\.stringValue) else { return nil }
            return .permissionRequest(id: id, toolCallId: string(f, "toolCallId"), kind: kind, summary: summary, options: options)
        case "permissionResolved":
            guard let id = string(f, "id"), let decision = string(f, "decision") else { return nil }
            return .permissionResolved(id: id, decision: decision)
        case "subagent":
            guard let id = string(f, "id"), let title = string(f, "title"), let status = string(f, "status") else { return nil }
            return .subagent(id: id, title: title, status: status)
        case "usage":
            return .usage(contextUsed: uint(f["contextUsed"]), contextLimit: uint(f["contextLimit"]),
                          inputTokens: uint(f["inputTokens"]), outputTokens: uint(f["outputTokens"]))
        case "configChanged":
            return .configChanged(model: string(f, "model"), permission: string(f, "permission"), effort: string(f, "effort"))
        case "error":
            guard let message = string(f, "message"), let recoverable = f["recoverable"]?.boolValue else { return nil }
            return .error(message: message, recoverable: recoverable)
        case "unknown":
            guard let kind = string(f, "kind") else { return nil }
            return .unknown(kind: kind, payload: f["payload"] ?? .null)
        default:
            return nil
        }
    }
}

extension LiveUpdatesBatch {
    public static func decode(_ value: JSONValue) throws -> LiveUpdatesBatch {
        guard let items = value["updates"]?.arrayValue, let latest = uint(value["latestSeq"]) else {
            throw LiveSessionDecodeError("a batch needs updates and latestSeq")
        }
        let gap: UInt64?
        if let object = value["gap"], !object.isNull {
            guard let oldest = uint(object["oldestAvailable"]) else { throw LiveSessionDecodeError("gap needs oldestAvailable") }
            gap = oldest
        } else {
            gap = nil
        }
        return LiveUpdatesBatch(updates: try items.map(LiveSessionUpdate.decode), latestSeq: latest, gapOldestAvailable: gap)
    }
}

extension LiveSessionState {
    public static func decode(_ value: JSONValue) throws -> LiveSessionState {
        let f = value.fields
        guard let taskId = string(f, "taskId"), let live = f["live"]?.boolValue, let turnActive = f["turnActive"]?.boolValue,
              let pending = f["pending"]?.arrayValue, let plan = f["plan"]?.arrayValue, let latest = uint(f["latestSeq"])
        else { throw LiveSessionDecodeError("state needs taskId, live, turnActive, pending, plan and latestSeq") }
        let permissions = try pending.map { item -> LivePendingPermission in
            let p = item.fields
            guard let id = string(p, "id"), let kind = string(p, "kind"), let summary = string(p, "summary") else {
                throw LiveSessionDecodeError("a pending permission needs id, kind and summary")
            }
            return LivePendingPermission(id: id, kind: kind, summary: summary, toolCallId: string(p, "toolCallId"))
        }
        let entries = plan.compactMap(LivePlanEntry.decode)
        guard entries.count == plan.count else { throw LiveSessionDecodeError("a plan entry needs text and status") }
        return LiveSessionState(
            taskId: taskId, live: live, family: string(f, "family"), sessionId: string(f, "sessionId"),
            turnActive: turnActive, pending: permissions, plan: entries,
            model: string(f, "model"), permission: string(f, "permission"), effort: string(f, "effort"),
            contextUsed: uint(f["contextUsed"]), contextLimit: uint(f["contextLimit"]),
            inputTokens: uint(f["inputTokens"]), outputTokens: uint(f["outputTokens"]), latestSeq: latest
        )
    }
}

// MARK: - Requests

/// Request params exactly as the host's strict param types expect them.
public enum LiveSessionRequest {
    private static func task(_ taskId: String, _ extra: [String: JSONValue] = [:]) -> JSONValue {
        .object(extra.merging(["taskId": .string(taskId)]) { _, new in new })
    }

    public static func prompt(taskId: String, text: String) -> JSONValue { task(taskId, ["text": .string(text)]) }
    public static func interrupt(taskId: String) -> JSONValue { task(taskId) }
    public static func close(taskId: String) -> JSONValue { task(taskId) }
    public static func state(taskId: String) -> JSONValue { task(taskId) }

    public static func respond(taskId: String, requestId: String, decision: LiveDecision, reason: String?) -> JSONValue {
        var extra: [String: JSONValue] = ["requestId": .string(requestId), "decision": .string(decision.rawValue)]
        if let reason { extra["reason"] = .string(reason) }
        return task(taskId, extra)
    }

    public static func configure(taskId: String, model: String?, permission: LivePermission?, effort: String?) -> JSONValue {
        var extra: [String: JSONValue] = [:]
        if let model { extra["model"] = .string(model) }
        if let permission { extra["permission"] = .string(permission.rawValue) }
        if let effort { extra["effort"] = .string(effort) }
        return task(taskId, extra)
    }

    public static func command(taskId: String, command: LiveCommand) -> JSONValue {
        task(taskId, ["command": .string(command.rawValue)])
    }

    public static func updates(taskId: String, after: UInt64, limit: Int?) -> JSONValue {
        var extra: [String: JSONValue] = ["after": .number(String(after))]
        if let limit { extra["limit"] = .number(String(limit)) }
        return task(taskId, extra)
    }
}
