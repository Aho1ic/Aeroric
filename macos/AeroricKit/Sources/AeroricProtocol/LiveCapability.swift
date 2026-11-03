import Foundation

/// R1-3a: one feature of an agent family where it runs (Q-S11).
public struct LiveCapabilityEntry: Equatable, Sendable {
    public var feature: String
    /// `yes` `no` `degraded` `unknown`.
    public var state: String
    /// Why not, how it differs, or who enforces it.
    public var note: String?

    public init(feature: String, state: String, note: String? = nil) {
        self.feature = feature
        self.state = state
        self.note = note
    }
}

/// R1-3a: a family's capability table; empty when the host has none (older host).
public struct LiveCapabilities: Equatable, Sendable {
    public var entries: [String: LiveCapabilityEntry]

    public init(_ entries: [LiveCapabilityEntry] = []) {
        self.entries = Dictionary(entries.map { ($0.feature, $0) }, uniquingKeysWith: { _, last in last })
    }

    public var isEmpty: Bool { entries.isEmpty }

    public func entry(_ feature: String) -> LiveCapabilityEntry? { entries[feature] }

    public static func decode(_ value: JSONValue?) -> LiveCapabilities {
        LiveCapabilities((value?.arrayValue ?? []).compactMap { item in
            guard let f = item.objectValue, let feature = f["feature"]?.stringValue,
                  let state = f["state"]?.stringValue else { return nil }
            return LiveCapabilityEntry(feature: feature, state: state, note: f["note"]?.stringValue)
        })
    }
}

/// R1-3a: a login the agent refused, until a turn under it completes.
public struct ExpiredLogin: Equatable, Sendable, Identifiable {
    public var id: String
    public var family: String
    /// "Claude（本机）".
    public var label: String
    public var reason: String
    public var atMs: UInt64
    /// The agent's own login: what to run to sign in again.
    public var command: String?
    /// An Agent configuration: the one to check in Settings.
    public var configId: String?
    public var connectionId: String?

    public init(id: String, family: String, label: String, reason: String, atMs: UInt64 = 0,
                command: String? = nil, configId: String? = nil, connectionId: String? = nil) {
        self.id = id
        self.family = family
        self.label = label
        self.reason = reason
        self.atMs = atMs
        self.command = command
        self.configId = configId
        self.connectionId = connectionId
    }

    public static func decodeList(_ value: JSONValue?) -> [ExpiredLogin] {
        (value?.arrayValue ?? []).compactMap { item in
            guard let f = item.objectValue, let id = f["id"]?.stringValue,
                  let family = f["family"]?.stringValue, let label = f["label"]?.stringValue else { return nil }
            return ExpiredLogin(id: id, family: family, label: label, reason: f["reason"]?.stringValue ?? "",
                                atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                command: f["command"]?.stringValue, configId: f["configId"]?.stringValue,
                                connectionId: f["connectionId"]?.stringValue)
        }
    }
}
