import Foundation

/// R1-2 (2026-11-02): one queued message for a session — a delegation's
/// result, a review's comments, later the user's own — and whether it
/// arrived. The latest `delivery` update of an `id` replaces it.
public struct LiveDelivery: Equatable, Sendable, Identifiable {
    public var id: String
    public var taskId: String
    /// `delegation` `review` `handoff` `user`.
    public var kind: String
    /// `now` `afterTurn`.
    public var mode: String
    public var label: String
    public var text: String
    /// `waiting` `sending` `accepted` `delivered` `failed` `unknown` `paused` `withdrawn`.
    public var state: String
    public var attempts: Int
    public var reason: String?
    /// `protocol` `echo` `turnStarted` `transcript`.
    public var evidence: String?
    /// Only the user can fix why it failed.
    public var blocked: Bool
    public var createdMs: UInt64
    public var updatedMs: UInt64

    public init(id: String, taskId: String = "", kind: String = "delegation", mode: String = "afterTurn",
                label: String = "", text: String = "", state: String, attempts: Int = 0, reason: String? = nil,
                evidence: String? = nil, blocked: Bool = false, createdMs: UInt64 = 0, updatedMs: UInt64 = 0) {
        self.id = id
        self.taskId = taskId
        self.kind = kind
        self.mode = mode
        self.label = label
        self.text = text
        self.state = state
        self.attempts = attempts
        self.reason = reason
        self.evidence = evidence
        self.blocked = blocked
        self.createdMs = createdMs
        self.updatedMs = updatedMs
    }

    /// Not settled: still queued, or waiting on the user.
    public var isOpen: Bool { state != "delivered" && state != "withdrawn" }

    public static func decode(_ value: JSONValue?) -> LiveDelivery? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue, let state = f["state"]?.stringValue else {
            return nil
        }
        let u64 = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        return LiveDelivery(id: id, taskId: f["taskId"]?.stringValue ?? "", kind: f["kind"]?.stringValue ?? "delegation",
                            mode: f["mode"]?.stringValue ?? "afterTurn", label: f["label"]?.stringValue ?? "",
                            text: f["text"]?.stringValue ?? "", state: state,
                            attempts: f["attempts"]?.numberText.flatMap { Int($0) } ?? 0,
                            reason: f["reason"]?.stringValue, evidence: f["evidence"]?.stringValue,
                            blocked: f["blocked"]?.boolValue ?? false, createdMs: u64("createdMs"),
                            updatedMs: u64("updatedMs"))
    }
}
