import Foundation

/// R2-1 (2026-11-04): a turn that changed files, as the host's ledger keeps
/// it. Mirrors `aeroric_protocol::turn_ledger`.
public struct LiveTurnRecord: Equatable, Sendable, Identifiable {
    public var id: String { turnId }
    public var turnId: String
    public var startedMs: UInt64
    public var endedMs: UInt64
    public var files: [String]
    public var additions: UInt64
    public var deletions: UInt64
    /// `pending` / `accepted` / `undone`.
    public var state: String
    /// R2-2: hunks of this turn the user rejected.
    public var rejected: Int = 0
    /// R2-3a: turns undone together share this; taking back takes back all.
    public var undoGroup: String?
    /// B2-6: the state the turn had when it was undone (a restore returns to it).
    public var stateBeforeUndo: String?
    /// B2-6: the acceptance check's result for this turn, when it ran.
    public var check: LiveTurnCheck?

    public init(turnId: String, startedMs: UInt64, endedMs: UInt64, files: [String] = [], additions: UInt64 = 0,
                deletions: UInt64 = 0, state: String = "pending") {
        self.turnId = turnId
        self.startedMs = startedMs
        self.endedMs = endedMs
        self.files = files
        self.additions = additions
        self.deletions = deletions
        self.state = state
    }

    public static func decodeList(_ value: JSONValue?) -> [LiveTurnRecord] {
        (value?.arrayValue ?? []).compactMap(decode)
    }

    public static func decode(_ value: JSONValue) -> LiveTurnRecord? {
        let f = value.fields
        guard let turnId = f["turnId"]?.stringValue else { return nil }
        func n(_ key: String) -> UInt64 { f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        let files = (f["files"]?.arrayValue ?? []).compactMap { $0.fields["path"]?.stringValue }
        var record = LiveTurnRecord(turnId: turnId, startedMs: n("startedMs"), endedMs: n("endedMs"), files: files,
                                    additions: n("additions"), deletions: n("deletions"),
                                    state: f["state"]?.stringValue ?? "pending")
        record.rejected = Int(n("rejected"))
        record.undoGroup = f["undoGroup"]?.stringValue
        record.stateBeforeUndo = f["stateBeforeUndo"]?.stringValue
        record.check = f["check"].flatMap(LiveTurnCheck.decode)
        return record
    }
}

/// B2-6: mirrors `aeroric_protocol::turn_ledger::TurnCheck`.
public struct LiveTurnCheck: Equatable, Sendable {
    public var passed: Bool
    /// Empty when it passed.
    public var summary: String

    public init(passed: Bool, summary: String = "") {
        self.passed = passed
        self.summary = summary
    }

    static func decode(_ value: JSONValue) -> LiveTurnCheck? {
        guard case let .bool(passed)? = value.fields["passed"] else { return nil }
        return LiveTurnCheck(passed: passed, summary: value.fields["summary"]?.stringValue ?? "")
    }
}

/// What undoing (or taking an undo back) would do.
public struct LiveTurnPlan: Equatable, Sendable {
    public struct File: Equatable, Sendable, Hashable {
        public var path: String
        public var delete: Bool
        public init(path: String, delete: Bool = false) { self.path = path; self.delete = delete }
    }
    public struct Skipped: Equatable, Sendable, Hashable {
        public var path: String
        public var reason: String
        public init(path: String, reason: String) { self.path = path; self.reason = reason }
    }
    public var files: [File]
    public var skipped: [Skipped]
    public var blocked: String?
    /// R2-3a: the turns it covers, oldest first.
    public var turns: [String]
    /// A7 (2026-11-17): files already as the action would leave them —
    /// nothing to write, not a conflict.
    public var inPlace: [String]
    /// A7: carrying it out, the files that could not be written; the turn
    /// kept its state and trying again finishes the rest.
    public var failed: [Skipped]

    public init(files: [File] = [], skipped: [Skipped] = [], blocked: String? = nil, turns: [String] = [],
                inPlace: [String] = [], failed: [Skipped] = []) {
        self.files = files
        self.skipped = skipped
        self.blocked = blocked
        self.turns = turns
        self.inPlace = inPlace
        self.failed = failed
    }

    /// Confirming does something: a file to write, or files already in place
    /// whose turn is then marked.
    public var actionable: Bool { !files.isEmpty || !inPlace.isEmpty }

    public static func decode(_ value: JSONValue?) -> LiveTurnPlan {
        let f = value?.fields ?? [:]
        let files = (f["files"]?.arrayValue ?? []).compactMap { item -> File? in
            guard let path = item.fields["path"]?.stringValue else { return nil }
            return File(path: path, delete: item.fields["delete"]?.boolValue ?? false)
        }
        func reasons(_ key: String) -> [Skipped] {
            (f[key]?.arrayValue ?? []).compactMap { item -> Skipped? in
                guard let path = item.fields["path"]?.stringValue else { return nil }
                return Skipped(path: path, reason: item.fields["reason"]?.stringValue ?? "")
            }
        }
        let turns = (f["turns"]?.arrayValue ?? []).compactMap(\.stringValue)
        let inPlace = (f["inPlace"]?.arrayValue ?? []).compactMap(\.stringValue)
        return LiveTurnPlan(files: files, skipped: reasons("skipped"), blocked: f["blocked"]?.stringValue,
                            turns: turns, inPlace: inPlace, failed: reasons("failed"))
    }
}
