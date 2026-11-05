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
        return record
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

    public init(files: [File] = [], skipped: [Skipped] = [], blocked: String? = nil, turns: [String] = []) {
        self.files = files
        self.skipped = skipped
        self.blocked = blocked
        self.turns = turns
    }

    public static func decode(_ value: JSONValue?) -> LiveTurnPlan {
        let f = value?.fields ?? [:]
        let files = (f["files"]?.arrayValue ?? []).compactMap { item -> File? in
            guard let path = item.fields["path"]?.stringValue else { return nil }
            return File(path: path, delete: item.fields["delete"]?.boolValue ?? false)
        }
        let skipped = (f["skipped"]?.arrayValue ?? []).compactMap { item -> Skipped? in
            guard let path = item.fields["path"]?.stringValue else { return nil }
            return Skipped(path: path, reason: item.fields["reason"]?.stringValue ?? "")
        }
        let turns = (f["turns"]?.arrayValue ?? []).compactMap(\.stringValue)
        return LiveTurnPlan(files: files, skipped: skipped, blocked: f["blocked"]?.stringValue, turns: turns)
    }
}
