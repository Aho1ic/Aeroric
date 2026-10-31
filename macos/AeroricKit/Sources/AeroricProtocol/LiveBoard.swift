import Foundation

/// B4-3 (2026-10-31): a manager session's shared board as the host reports it.
public struct LiveBoard: Equatable, Sendable {
    public var ownerTaskId: String
    public var revision: UInt64
    public var entries: [LiveBoardEntry]

    public init(ownerTaskId: String, revision: UInt64 = 0, entries: [LiveBoardEntry] = []) {
        self.ownerTaskId = ownerTaskId
        self.revision = revision
        self.entries = entries
    }

    public static func decode(_ value: JSONValue?) -> LiveBoard? {
        guard let f = value?.objectValue, let owner = f["ownerTaskId"]?.stringValue else { return nil }
        return LiveBoard(ownerTaskId: owner, revision: f["revision"]?.numberText.flatMap { UInt64($0) } ?? 0,
                         entries: (f["entries"]?.arrayValue ?? []).compactMap(LiveBoardEntry.decode))
    }
}

public struct LiveBoardEntry: Equatable, Sendable, Identifiable {
    public var id: String
    /// `goal` `hypothesis` `claim` `decision` `evidence` `question`.
    public var kind: String
    public var text: String
    /// `open` `confirmed` `refuted` `proposed` `accepted` `rejected` `answered` `done`.
    public var state: String
    /// `user` or the task id of the session that wrote it.
    public var author: String
    public var claimedBy: String?
    public var updatedMs: UInt64

    public init(id: String, kind: String, text: String, state: String, author: String = "user",
                claimedBy: String? = nil, updatedMs: UInt64 = 0) {
        self.id = id
        self.kind = kind
        self.text = text
        self.state = state
        self.author = author
        self.claimedBy = claimedBy
        self.updatedMs = updatedMs
    }

    public static func decode(_ value: JSONValue) -> LiveBoardEntry? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let kind = f["kind"]?.stringValue,
              let text = f["text"]?.stringValue else { return nil }
        return LiveBoardEntry(id: id, kind: kind, text: text, state: f["state"]?.stringValue ?? "open",
                              author: f["author"]?.stringValue ?? "user", claimedBy: f["claimedBy"]?.stringValue,
                              updatedMs: f["updatedMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}

/// A board entry that needs the user: a proposed decision, or a stale claim.
public struct LiveBoardAttention: Equatable, Sendable {
    public var ownerTaskId: String
    public var entry: LiveBoardEntry
    /// `decision` or `staleClaim`.
    public var reason: String

    public init(ownerTaskId: String, entry: LiveBoardEntry, reason: String) {
        self.ownerTaskId = ownerTaskId
        self.entry = entry
        self.reason = reason
    }

    public static func decode(_ value: JSONValue) -> LiveBoardAttention? {
        let f = value.fields
        guard let owner = f["ownerTaskId"]?.stringValue, let entry = f["entry"].flatMap(LiveBoardEntry.decode),
              let reason = f["reason"]?.stringValue else { return nil }
        return LiveBoardAttention(ownerTaskId: owner, entry: entry, reason: reason)
    }
}

/// B4-3: one failure probe hit on a delegation.
public struct LiveDelegationProbe: Equatable, Sendable {
    /// `repeat` `oscillation` `nearBudget` `unverifiedDone` `noEvidence`.
    public var kind: String
    public var detail: String
    public var atMs: UInt64
    public var acknowledged: Bool

    public init(kind: String, detail: String, atMs: UInt64 = 0, acknowledged: Bool = false) {
        self.kind = kind
        self.detail = detail
        self.atMs = atMs
        self.acknowledged = acknowledged
    }

    static func decode(_ value: JSONValue) -> LiveDelegationProbe? {
        let f = value.fields
        guard let kind = f["kind"]?.stringValue else { return nil }
        return LiveDelegationProbe(kind: kind, detail: f["detail"]?.stringValue ?? "",
                                   atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                   acknowledged: f["acknowledged"]?.boolValue ?? false)
    }
}
