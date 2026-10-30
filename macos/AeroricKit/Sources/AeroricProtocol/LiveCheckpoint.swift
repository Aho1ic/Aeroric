import Foundation

/// B4′ (2026-10-30): a task's checkpoint, derived by the host from the
/// session's structured updates. Mirrors `aeroric_protocol::checkpoint`.
/// `status` is `verified` | `inferred` | `unknown`, set by where an entry
/// came from, never by the agent.
public struct LiveCheckpointEntry: Equatable, Sendable {
    public var text: String
    public var status: String
    public var evidence: [LiveEvidenceRef]

    public init(text: String, status: String, evidence: [LiveEvidenceRef] = []) {
        self.text = text
        self.status = status
        self.evidence = evidence
    }
}

public struct LiveEvidenceRef: Equatable, Sendable {
    /// `userMessage` | `toolResult` | `turnChanges` | `autoCheck` | `commit` | `permission`.
    public var kind: String
    public var seq: UInt64?
    public var turn: UInt64?
    public var detail: String

    public init(kind: String, seq: UInt64? = nil, turn: UInt64? = nil, detail: String = "") {
        self.kind = kind
        self.seq = seq
        self.turn = turn
        self.detail = detail
    }
}

public struct LiveEffect: Equatable, Sendable {
    public var summary: String
    /// `command` | `fileChange` | `mcp` | `changes` | `commit`.
    public var kind: String
    public var turn: UInt64
    public var status: String
    public var evidence: [LiveEvidenceRef]

    public init(summary: String, kind: String, turn: UInt64, status: String, evidence: [LiveEvidenceRef] = []) {
        self.summary = summary
        self.kind = kind
        self.turn = turn
        self.status = status
        self.evidence = evidence
    }
}

public struct LiveIntent: Equatable, Sendable {
    public var requestId: String
    public var summary: String
    public var verify: String
    /// `pending` | `denied` | `executed` | `outcomeUnknown`.
    public var state: String
    public var turn: UInt64

    public init(requestId: String, summary: String, verify: String, state: String, turn: UInt64) {
        self.requestId = requestId
        self.summary = summary
        self.verify = verify
        self.state = state
        self.turn = turn
    }
}

public struct LiveObligation: Equatable, Sendable {
    public var id: String
    public var text: String
    public var check: String?
    /// `open` | `passed` | `failed` | `unknown`.
    public var state: String
    /// `acceptance` | `plan` | `manual`.
    public var source: String

    public init(id: String, text: String, check: String? = nil, state: String, source: String) {
        self.id = id
        self.text = text
        self.check = check
        self.state = state
        self.source = source
    }
}

public struct LiveBranchResult: Equatable, Sendable {
    public var taskId: String
    public var title: String
    public var summary: String
    public var files: UInt64
    public var additions: UInt64
    public var deletions: UInt64
    public var returnedMs: UInt64

    public init(taskId: String, title: String, summary: String, files: UInt64 = 0, additions: UInt64 = 0,
                deletions: UInt64 = 0, returnedMs: UInt64) {
        self.taskId = taskId
        self.title = title
        self.summary = summary
        self.files = files
        self.additions = additions
        self.deletions = deletions
        self.returnedMs = returnedMs
    }
}

public struct LiveAcceptance: Equatable, Sendable {
    public var text: String
    public var check: String?

    public init(text: String, check: String? = nil) {
        self.text = text
        self.check = check
    }
}

/// What the clarification turn proposed; nothing runs until it is accepted.
public struct LiveClarification: Equatable, Sendable {
    public var turnId: String
    public var requirements: [String]
    public var acceptance: [LiveAcceptance]
    public var questions: [String]
    /// The turn's text when it held no criteria that could be read.
    public var raw: String?

    public init(turnId: String, requirements: [String] = [], acceptance: [LiveAcceptance] = [],
                questions: [String] = [], raw: String? = nil) {
        self.turnId = turnId
        self.requirements = requirements
        self.acceptance = acceptance
        self.questions = questions
        self.raw = raw
    }
}

public struct LiveCheckpoint: Equatable, Sendable {
    public var taskId: String
    public var revision: UInt64
    public var turn: UInt64
    public var updatedMs: UInt64
    public var goal: LiveCheckpointEntry?
    public var decisions: [LiveCheckpointEntry]
    public var effects: [LiveEffect]
    public var effectsFolded: UInt64
    public var unknown: [LiveEffect]
    public var intents: [LiveIntent]
    public var obligations: [LiveObligation]
    public var constraints: [LiveCheckpointEntry]
    public var branches: [LiveBranchResult]

    public init(taskId: String, revision: UInt64 = 0, turn: UInt64 = 0, updatedMs: UInt64 = 0,
                goal: LiveCheckpointEntry? = nil, decisions: [LiveCheckpointEntry] = [], effects: [LiveEffect] = [],
                effectsFolded: UInt64 = 0, unknown: [LiveEffect] = [], intents: [LiveIntent] = [],
                obligations: [LiveObligation] = [], constraints: [LiveCheckpointEntry] = [],
                branches: [LiveBranchResult] = []) {
        self.taskId = taskId
        self.revision = revision
        self.turn = turn
        self.updatedMs = updatedMs
        self.goal = goal
        self.decisions = decisions
        self.effects = effects
        self.effectsFolded = effectsFolded
        self.unknown = unknown
        self.intents = intents
        self.obligations = obligations
        self.constraints = constraints
        self.branches = branches
    }

    /// Something a successor must check before going on.
    public var needsAttention: Bool {
        !unknown.isEmpty
            || obligations.contains { $0.state == "open" || $0.state == "unknown" }
            || intents.contains { $0.state == "outcomeUnknown" }
    }
}

// MARK: - Decoding (lenient: a malformed row is dropped, the rest kept)

extension LiveCheckpoint {
    public static func decode(_ value: JSONValue) -> LiveCheckpoint? {
        let f = value.fields
        guard let taskId = f["taskId"]?.stringValue else { return nil }
        return LiveCheckpoint(
            taskId: taskId,
            revision: cpUInt(f["revision"]) ?? 0,
            turn: cpUInt(f["turn"]) ?? 0,
            updatedMs: cpUInt(f["updatedMs"]) ?? 0,
            goal: f["goal"].flatMap(LiveCheckpointEntry.decode),
            decisions: cpList(f["decisions"], LiveCheckpointEntry.decode),
            effects: cpList(f["effects"], LiveEffect.decode),
            effectsFolded: cpUInt(f["effectsFolded"]) ?? 0,
            unknown: cpList(f["unknown"], LiveEffect.decode),
            intents: cpList(f["intents"], LiveIntent.decode),
            obligations: cpList(f["obligations"], LiveObligation.decode),
            constraints: cpList(f["constraints"], LiveCheckpointEntry.decode),
            branches: cpList(f["branches"], LiveBranchResult.decode)
        )
    }
}

extension LiveCheckpointEntry {
    static func decode(_ value: JSONValue) -> LiveCheckpointEntry? {
        let f = value.fields
        guard let text = f["text"]?.stringValue, let status = f["status"]?.stringValue else { return nil }
        return LiveCheckpointEntry(text: text, status: status, evidence: cpList(f["evidence"], LiveEvidenceRef.decode))
    }
}

extension LiveEvidenceRef {
    static func decode(_ value: JSONValue) -> LiveEvidenceRef? {
        let f = value.fields
        guard let kind = f["kind"]?.stringValue else { return nil }
        return LiveEvidenceRef(kind: kind, seq: cpUInt(f["seq"]), turn: cpUInt(f["turn"]),
                               detail: f["detail"]?.stringValue ?? "")
    }
}

extension LiveEffect {
    static func decode(_ value: JSONValue) -> LiveEffect? {
        let f = value.fields
        guard let summary = f["summary"]?.stringValue, let kind = f["kind"]?.stringValue,
              let status = f["status"]?.stringValue else { return nil }
        return LiveEffect(summary: summary, kind: kind, turn: cpUInt(f["turn"]) ?? 0, status: status,
                          evidence: cpList(f["evidence"], LiveEvidenceRef.decode))
    }
}

extension LiveIntent {
    static func decode(_ value: JSONValue) -> LiveIntent? {
        let f = value.fields
        guard let id = f["requestId"]?.stringValue, let summary = f["summary"]?.stringValue,
              let state = f["state"]?.stringValue else { return nil }
        return LiveIntent(requestId: id, summary: summary, verify: f["verify"]?.stringValue ?? "", state: state,
                          turn: cpUInt(f["turn"]) ?? 0)
    }
}

extension LiveObligation {
    static func decode(_ value: JSONValue) -> LiveObligation? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let text = f["text"]?.stringValue, let state = f["state"]?.stringValue,
              let source = f["source"]?.stringValue else { return nil }
        return LiveObligation(id: id, text: text, check: f["check"]?.stringValue, state: state, source: source)
    }
}

extension LiveBranchResult {
    static func decode(_ value: JSONValue) -> LiveBranchResult? {
        let f = value.fields
        guard let taskId = f["taskId"]?.stringValue, let title = f["title"]?.stringValue,
              let summary = f["summary"]?.stringValue else { return nil }
        return LiveBranchResult(taskId: taskId, title: title, summary: summary, files: cpUInt(f["files"]) ?? 0,
                                additions: cpUInt(f["additions"]) ?? 0, deletions: cpUInt(f["deletions"]) ?? 0,
                                returnedMs: cpUInt(f["returnedMs"]) ?? 0)
    }
}

extension LiveClarification {
    static func decode(_ f: [String: JSONValue]) -> LiveClarification? {
        guard let turnId = f["turnId"]?.stringValue else { return nil }
        let acceptance = cpList(f["acceptance"]) { item -> LiveAcceptance? in
            guard let text = item["text"]?.stringValue else { return nil }
            return LiveAcceptance(text: text, check: item["check"]?.stringValue)
        }
        return LiveClarification(turnId: turnId,
                                 requirements: f["requirements"]?.arrayValue?.compactMap(\.stringValue) ?? [],
                                 acceptance: acceptance,
                                 questions: f["questions"]?.arrayValue?.compactMap(\.stringValue) ?? [],
                                 raw: f["raw"]?.stringValue)
    }
}

private func cpUInt(_ value: JSONValue?) -> UInt64? { value?.numberText.flatMap { UInt64($0) } }

private func cpList<T>(_ value: JSONValue?, _ decode: (JSONValue) -> T?) -> [T] {
    (value?.arrayValue ?? []).compactMap(decode)
}
