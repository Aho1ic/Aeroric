import Foundation

/// The v1 data types beyond the envelope: tasks, sessions, the terminal plane and
/// the capability report. Every rule here has a twin in `aeroric-protocol` (Rust)
/// and `src/lib/nativeHostProtocol.ts`, and `ProtocolGoldenTests` checks all three
/// against `contracts/native/v1/fixtures/`.

// MARK: - Vocabulary

public enum TaskStatus: String, Equatable, CaseIterable, Sendable {
    case todo, pending, running, inputRequired = "input_required", detached, interrupted
    case done, failed, cancelled

    /// States that still hold an agent or expect input; cleanup must leave them be.
    public var isLive: Bool {
        self == .pending || self == .running || self == .inputRequired || self == .detached
    }

    public var isTerminal: Bool { self == .done || self == .failed || self == .cancelled }
}

public enum ProtocolFamily: String, Equatable, CaseIterable, Sendable {
    case claude, codex, dsh, omp
}

public enum PermissionMode: String, Equatable, CaseIterable, Sendable {
    case ask, autoEdit = "auto_edit", fullAccess = "full_access"
}

public enum BuiltInAgent: String, Equatable, CaseIterable, Sendable {
    case claude, claudeGpt55 = "claude_gpt55", codex, dsh, omp

    /// The **session** family (ADR-028): which transcript format and config home this agent
    /// reads. Not the launch family — `claude_gpt55` is Claude here but is started with
    /// codex-style arguments, which the host resolves from the user's own script.
    public var family: ProtocolFamily {
        switch self {
        case .claude, .claudeGpt55: .claude
        case .codex: .codex
        case .dsh: .dsh
        case .omp: .omp
        }
    }
}

/// Built-ins are a closed set; a custom profile is an open string, so its family
/// travels with it. Inferring a family from an id would let a custom agent named
/// `codex-like` change the wire protocol.
public enum AgentId: Equatable, Sendable {
    case builtIn(BuiltInAgent)
    case custom(id: String, family: ProtocolFamily?)

    /// Only a fallback for records written before `sessionFamily` existed; a new
    /// launch must never pick a protocol from here.
    public var declaredFamily: ProtocolFamily? {
        switch self {
        case let .builtIn(agent): agent.family
        case let .custom(_, family): family
        }
    }

    public static func decode(_ value: JSONValue) -> AgentId? {
        if let text = value.stringValue, let agent = BuiltInAgent(rawValue: text) {
            return .builtIn(agent)
        }
        guard let fields = value.objectValue else { return nil }
        if let text = fields["builtIn"]?.stringValue, let agent = BuiltInAgent(rawValue: text) {
            return .builtIn(agent)
        }
        guard let id = fields["id"]?.stringValue else { return nil }
        let family = fields["family"]?.stringValue.flatMap(ProtocolFamily.init(rawValue:))
        return .custom(id: id, family: family)
    }
}

// MARK: - Task

public struct TaskSnapshot: Equatable, Sendable {
    public var id: String
    public var projectId: String
    public var status: TaskStatus
    public var agent: AgentId
    public var revision: Quantity
    public var generation: Quantity
    public var sessionOwner: SessionOwner?
    public var worktreeDiscarded: Bool?
    /// Keys this build does not model, including every legacy per-family session
    /// field. Preserved so a round trip cannot lose a record's history.
    public var additional: [String: JSONValue]

    private static let modelled: Set<String> = [
        "id", "projectId", "status", "agent", "revision", "generation", "sessionOwner",
        "worktreeDiscarded",
    ]

    public static func decode(_ value: JSONValue) -> TaskSnapshot? {
        guard let fields = value.objectValue,
              let id = fields["id"]?.stringValue,
              let projectId = fields["projectId"]?.stringValue,
              let statusText = fields["status"]?.stringValue,
              let status = TaskStatus(rawValue: statusText),
              let agent = AgentId.decode(fields["agent"] ?? .null) else { return nil }
        let revision = fields["revision"]?.stringValue.flatMap(Quantity.init)
        let generation = fields["generation"]?.stringValue.flatMap(Quantity.init)
        return TaskSnapshot(
            id: id, projectId: projectId, status: status, agent: agent,
            revision: revision ?? Quantity(0), generation: generation ?? Quantity(0),
            sessionOwner: fields["sessionOwner"].flatMap(SessionOwner.decode),
            worktreeDiscarded: fields["worktreeDiscarded"]?.boolValue,
            additional: fields.filter { !modelled.contains($0.key) }
        )
    }

    /// Mirrors `hasTaskSession` in `src/taskSession.ts`: an id *or* a path counts.
    public var hasSession: Bool { sessionOwner?.hasTranscript ?? false }

    /// Mirrors `canResumeTask` in `src/components/task-panel/TaskList.tsx` and
    /// `TaskSnapshot::can_resume`. Refusing here is what stops a resume from becoming
    /// a brand-new session on a task whose history is gone.
    public var canResume: Bool {
        guard status != .todo, !status.isLive, worktreeDiscarded != true else { return false }
        return hasSession
    }
}

// MARK: - Session

public enum SessionSource: String, Equatable, Sendable {
    case native, adopted, handoff
}

public struct SessionOwner: Equatable, Sendable {
    public var family: ProtocolFamily
    public var sessionId: String
    public var path: String?
    public var source: SessionSource
    public var recordedBy: String?
    public var originSessionId: String?

    public var hasTranscript: Bool { !sessionId.isEmpty || (path.map { !$0.isEmpty } ?? false) }

    public static func decode(_ value: JSONValue) -> SessionOwner? {
        guard let fields = value.objectValue,
              let familyText = fields["family"]?.stringValue,
              let family = ProtocolFamily(rawValue: familyText),
              let sessionId = fields["sessionId"]?.stringValue else { return nil }
        return SessionOwner(
            family: family, sessionId: sessionId,
            path: fields["path"]?.stringValue,
            source: fields["source"]?.stringValue.flatMap(SessionSource.init(rawValue:)) ?? .native,
            recordedBy: fields["recordedBy"]?.stringValue,
            originSessionId: fields["originSessionId"]?.stringValue
        )
    }
}

/// Content this build cannot interpret. Kept with its source id, because dropping it
/// is indistinguishable from "the agent never said that".
public struct OpaqueContent: Equatable, Sendable {
    public var kind: String
    public var sourceId: String?
    public var payload: JSONValue
    public var originFamily: ProtocolFamily?
}

public enum MessageRole: String, Equatable, Sendable {
    case user, assistant, tool, system
}

public enum ContentBlock: Equatable, Sendable {
    case text(kind: String, text: String)
    case unknown(OpaqueContent)

    public static func decode(_ value: JSONValue) -> ContentBlock {
        let fields = value.fields
        if let text = fields["text"]?.stringValue, let kind = fields["kind"]?.stringValue {
            return .text(kind: kind, text: text)
        }
        return .unknown(OpaqueContent(
            kind: fields["kind"]?.stringValue ?? "unknown",
            sourceId: fields["sourceId"]?.stringValue,
            payload: fields["payload"] ?? .object([:]),
            originFamily: fields["originFamily"]?.stringValue.flatMap(ProtocolFamily.init(rawValue:))
        ))
    }
}

public struct SessionMessage: Equatable, Sendable {
    public var id: String
    public var role: MessageRole
    public var blocks: [ContentBlock]
}

public struct SessionPage: Equatable, Sendable {
    public var sessionId: String
    public var family: ProtocolFamily
    public var messages: [SessionMessage]
    public var nextCursor: String?
    public var hasMore: Bool
    /// The whole session's 统计栏 figures, read from its transcript (first
    /// page only; Codex and Claude).
    public var stats: SessionTranscriptStats? = nil

    public static func decode(_ value: JSONValue) -> SessionPage? {
        guard let fields = value.objectValue,
              let sessionId = fields["sessionId"]?.stringValue,
              let familyText = fields["family"]?.stringValue,
              let family = ProtocolFamily(rawValue: familyText) else { return nil }
        let messages = (fields["messages"]?.arrayValue ?? []).compactMap { item -> SessionMessage? in
            let row = item.fields
            guard let id = row["id"]?.stringValue,
                  let role = row["role"]?.stringValue.flatMap(MessageRole.init(rawValue:)) else { return nil }
            return SessionMessage(
                id: id, role: role,
                blocks: (row["blocks"]?.arrayValue ?? []).map(ContentBlock.decode)
            )
        }
        return SessionPage(
            sessionId: sessionId, family: family, messages: messages,
            nextCursor: fields["nextCursor"]?.stringValue,
            hasMore: fields["hasMore"]?.boolValue ?? false,
            stats: fields["stats"].flatMap(SessionTranscriptStats.decode)
        )
    }
}

/// `session.history`'s `stats` (2026-10-29): usage and timing for the whole
/// session, so an ended or reopened session still fills the 统计栏.
public struct SessionTranscriptStats: Equatable, Sendable {
    public var inputTokens: UInt64?
    public var cachedInputTokens: UInt64?
    public var outputTokens: UInt64?
    public var contextUsed: UInt64?
    public var contextLimit: UInt64?
    public var modelMs: UInt64
    public var toolMs: UInt64
    public var firstTokenMsTotal: UInt64
    public var firstTokenSamples: Int
    /// Oldest first: each turn's start, end and tokens (2026-10-29).
    public var turns: [TurnStat] = []

    public struct TurnStat: Equatable, Sendable {
        public var startedAt: UInt64
        public var endedAt: UInt64?
        public var tokens: UInt64

        public init(startedAt: UInt64, endedAt: UInt64?, tokens: UInt64) {
            self.startedAt = startedAt
            self.endedAt = endedAt
            self.tokens = tokens
        }
    }

    public init(inputTokens: UInt64? = nil, cachedInputTokens: UInt64? = nil, outputTokens: UInt64? = nil,
                contextUsed: UInt64? = nil, contextLimit: UInt64? = nil, modelMs: UInt64 = 0, toolMs: UInt64 = 0,
                firstTokenMsTotal: UInt64 = 0, firstTokenSamples: Int = 0) {
        self.inputTokens = inputTokens
        self.cachedInputTokens = cachedInputTokens
        self.outputTokens = outputTokens
        self.contextUsed = contextUsed
        self.contextLimit = contextLimit
        self.modelMs = modelMs
        self.toolMs = toolMs
        self.firstTokenMsTotal = firstTokenMsTotal
        self.firstTokenSamples = firstTokenSamples
    }

    public static func decode(_ value: JSONValue) -> SessionTranscriptStats? {
        guard let fields = value.objectValue else { return nil }
        func count(_ key: String) -> UInt64? { fields[key]?.numberText.flatMap { UInt64($0) } }
        var stats = SessionTranscriptStats(
            inputTokens: count("inputTokens"), cachedInputTokens: count("cachedInputTokens"),
            outputTokens: count("outputTokens"), contextUsed: count("contextUsed"),
            contextLimit: count("contextLimit"), modelMs: count("modelMs") ?? 0, toolMs: count("toolMs") ?? 0,
            firstTokenMsTotal: count("firstTokenMsTotal") ?? 0,
            firstTokenSamples: Int(count("firstTokenSamples") ?? 0))
        stats.turns = (fields["turns"]?.arrayValue ?? []).compactMap { item in
            let row = item.objectValue ?? [:]
            func n(_ key: String) -> UInt64? { row[key]?.numberText.flatMap { UInt64($0) } }
            guard let started = n("startedAt") else { return nil }
            return TurnStat(startedAt: started, endedAt: n("endedAt"), tokens: n("tokens") ?? 0)
        }
        return stats
    }
}

// MARK: - Terminal

/// Raw PTY bytes as base64. `Data(base64Encoded:)` is deliberately *not* used for the
/// verdict: `.ignoreUnknownCharacters` accepts garbage and hands back truncated
/// bytes, and a mangled chunk corrupts a screen nobody is looking at. The rule below
/// is the same one `BytesBase64::decode` implements, and both are checked against
/// `fixtures/terminal.json`.
public struct BytesBase64: Equatable, Sendable {
    public static let maxRawBytes = 65_536
    private static let alphabet = Array("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/")
    public let text: String

    public init?(_ text: String) {
        guard BytesBase64.decode(text) != nil else { return nil }
        self.text = text
    }

    public var bytes: [UInt8] { BytesBase64.decode(text) ?? [] }

    static func decode(_ text: String) -> [UInt8]? {
        if text.isEmpty { return [] }
        let body = Array(text)
        guard body.count % 4 == 0 else { return nil }
        var padding = 0
        while padding < 2, body[body.count - 1 - padding] == "=" { padding += 1 }
        var acc: UInt32 = 0
        var bits: UInt32 = 0
        var out: [UInt8] = []
        for character in body[0..<body.count - padding] {
            guard let value = alphabet.firstIndex(of: character) else { return nil }
            acc = (acc << 6) | UInt32(value)
            bits += 6
            if bits >= 8 {
                bits -= 8
                out.append(UInt8((acc >> bits) & 0xFF))
            }
        }
        if bits >= 6 || (acc & ((1 << bits) - 1)) != 0 { return nil }
        guard out.count == body.count / 4 * 3 - padding else { return nil }
        return out
    }
}

public struct TerminalIdentity: Equatable, Hashable, Sendable {
    public var taskId: String
    public var generation: Quantity
}

public struct TerminalOutput: Equatable, Sendable {
    public var identity: TerminalIdentity
    public var offset: Quantity
    public var nextOffset: Quantity
    public var truncated: Bool?
    public var reset: Bool?
    public var bytes: BytesBase64?

    public static func decode(_ value: JSONValue) -> TerminalOutput? {
        let fields = value.fields
        guard let taskId = fields["taskId"]?.stringValue,
              let generationText = fields["generation"]?.stringValue,
              let generation = Quantity(generationText),
              let offsetText = fields["offset"]?.stringValue, let offset = Quantity(offsetText),
              let nextText = fields["nextOffset"]?.stringValue, let nextOffset = Quantity(nextText),
              let base64 = fields["bytesBase64"], let raw = base64.stringValue,
              let bytes = BytesBase64(raw) else { return nil }
        return TerminalOutput(
            identity: TerminalIdentity(taskId: taskId, generation: generation),
            offset: offset, nextOffset: nextOffset,
            truncated: fields["truncated"]?.boolValue, reset: fields["reset"]?.boolValue,
            bytes: bytes
        )
    }
}

/// The terminal plane's own reset notice — not a control-stream gap. A renderer that
/// confused them would keep feeding bytes into an unknown escape-sequence position.
public struct TerminalGap: Equatable, Sendable {
    public enum Reason: String, Equatable, Sendable {
        case snapshotRequired = "SNAPSHOT_REQUIRED", backpressure = "BACKPRESSURE", epochChanged = "EPOCH_CHANGED"
    }

    public var identity: TerminalIdentity
    public var reason: Reason
    public var reset: Bool
    public var truncated: Bool?
    public var fromOffset: Quantity?

    public static func decode(_ value: JSONValue) -> TerminalGap? {
        let fields = value.fields
        guard let taskId = fields["taskId"]?.stringValue,
              let generationText = fields["generation"]?.stringValue,
              let generation = Quantity(generationText),
              let reasonText = fields["reason"]?.stringValue,
              let reason = Reason(rawValue: reasonText) else { return nil }
        return TerminalGap(
            identity: TerminalIdentity(taskId: taskId, generation: generation),
            reason: reason,
            reset: fields["reset"]?.boolValue ?? false,
            truncated: fields["truncated"]?.boolValue,
            fromOffset: fields["fromOffset"]?.stringValue.flatMap(Quantity.init)
        )
    }
}

public struct TerminalLease: Equatable, Sendable {
    public var identity: TerminalIdentity
    public var holder: String
    public var acquiredAt: String

    public static func decode(_ value: JSONValue) -> TerminalLease? {
        let fields = value.fields
        guard let taskId = fields["taskId"]?.stringValue,
              let generationText = fields["generation"]?.stringValue,
              let generation = Quantity(generationText),
              let holder = fields["holder"]?.stringValue,
              let acquiredAt = fields["acquiredAt"]?.numberText else { return nil }
        return TerminalLease(
            identity: TerminalIdentity(taskId: taskId, generation: generation),
            holder: holder, acquiredAt: acquiredAt
        )
    }
}

public struct TerminalResize: Equatable, Sendable {
    public var cols: Int
    public var rows: Int

    /// A hidden view reporting 0x0 must not overwrite the size the user is looking at.
    public var isMeaningful: Bool { cols > 0 && rows > 0 }
}

// MARK: - Capability

public struct Capability: Equatable, Sendable {
    public var name: String
    public var enabled: Bool
    public var since: Int?
    public var note: String?

    public static func decode(_ value: JSONValue) -> Capability? {
        let fields = value.fields
        guard let name = fields["name"]?.stringValue, let enabled = fields["enabled"]?.boolValue else {
            return nil
        }
        return Capability(
            name: name, enabled: enabled,
            since: fields["since"]?.numberText.flatMap(Int.init),
            note: fields["note"]?.stringValue
        )
    }
}

public struct HostCapabilities: Equatable, Sendable {
    public var protocolVersion: Int
    public var buildVersion: String
    public var dataFormat: String?
    public var capabilities: [String: Capability]
    public var additional: [String: JSONValue]

    private static let modelled: Set<String> = [
        "protocolVersion", "buildVersion", "dataFormat", "capabilities", "agents",
    ]

    public static func decode(_ value: JSONValue) -> HostCapabilities? {
        let fields = value.fields
        guard let versionText = fields["protocolVersion"]?.numberText,
              let protocolVersion = Int(versionText),
              let buildVersion = fields["buildVersion"]?.stringValue else { return nil }
        var capabilities: [String: Capability] = [:]
        for (key, item) in fields["capabilities"]?.fields ?? [:] {
            if let capability = Capability.decode(item) { capabilities[key] = capability }
        }
        return HostCapabilities(
            protocolVersion: protocolVersion, buildVersion: buildVersion,
            dataFormat: fields["dataFormat"]?.stringValue, capabilities: capabilities,
            additional: fields.filter { !modelled.contains($0.key) }
        )
    }

    public func enabled(_ capability: String) -> Bool { capabilities[capability]?.enabled ?? false }

    /// What to show a user whose control is disabled — the reason, not just "no".
    public func reason(for capability: String) -> String {
        if let entry = capabilities[capability] { return entry.note ?? "\(capability) is disabled on this host" }
        return "\(capability) is not reported by this host"
    }
}
