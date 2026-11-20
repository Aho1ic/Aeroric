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
/// B2-4: the host answers `session.capabilities` for the Mac and for an SSH
/// host separately (DSH cannot run remotely; delegates and reviewers there
/// are unknown); a project's place picks which set applies.
public enum CapabilityPlace {
    public static func isRemote(projectPath: String?) -> Bool {
        projectPath?.hasPrefix("ssh://") ?? false
    }

    public static func tables(projectPath: String?, local: [String: LiveCapabilities],
                              remote: [String: LiveCapabilities]) -> [String: LiveCapabilities] {
        isRemote(projectPath: projectPath) ? remote : local
    }
}

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

/// H1 (2026-11-20): what one launch of a session had on each surface
/// through which a harness uses Aeroric (`session.capabilities {taskId}`'s
/// `surfaces`), in the host's order.
public struct LiveSurfaceLaunch: Equatable, Sendable {
    public var atMs: UInt64
    public var harness: String
    /// `local` `remote`.
    public var place: String
    public var surfaces: [LiveCapabilityEntry]

    public init(atMs: UInt64 = 0, harness: String, place: String = "local", surfaces: [LiveCapabilityEntry]) {
        self.atMs = atMs
        self.harness = harness
        self.place = place
        self.surfaces = surfaces
    }

    public static func decode(_ value: JSONValue?) -> LiveSurfaceLaunch? {
        guard let f = value?.objectValue, let harness = f["harness"]?.stringValue else { return nil }
        let surfaces: [LiveCapabilityEntry] = (f["surfaces"]?.arrayValue ?? []).compactMap { item in
            guard let e = item.objectValue, let feature = e["feature"]?.stringValue,
                  let state = e["state"]?.stringValue else { return nil }
            return LiveCapabilityEntry(feature: feature, state: state, note: e["note"]?.stringValue)
        }
        return LiveSurfaceLaunch(atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0, harness: harness,
                                 place: f["place"]?.stringValue ?? "local", surfaces: surfaces)
    }
}
