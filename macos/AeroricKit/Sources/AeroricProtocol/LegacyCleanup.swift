import Foundation

/// Mirrors `aeroric_protocol::legacy_cleanup` (contracts/native/v1/legacy-cleanup.schema.json).
/// Host-local: `agentConnections.legacyCleanup.plan` / `.apply` over the authenticated UDS.
/// Decoding is strict like the Rust DTOs: unknown keys, unknown states and unknown
/// steps are malformed replies, never a degraded known state.
public enum LegacyCleanupContract {
    public static let schemaVersion = 1
    public static let capability = "agentConnections.legacyCleanup"
    public static let planMethod = "agentConnections.legacyCleanup.plan"
    public static let applyMethod = "agentConnections.legacyCleanup.apply"
    public static let sourcePrefix = "custom:"
    public static let maxSourceIDs = 256
}

public enum LegacyCleanupStep: String, Equatable, Sendable, CaseIterable {
    case removeProfile
    case quarantineSidecar
    case quarantineWrapper
    case deleteOldSecrets
}

public enum LegacyCleanupState: Equatable, Sendable {
    case eligible
    case blocked(reason: String)
    case partial(remaining: [LegacyCleanupStep], reason: String?)
    case done

    /// Only these can be confirmed for an apply (resume included).
    public var isActionable: Bool {
        switch self {
        case .eligible, .partial: true
        case .blocked, .done: false
        }
    }
}

public struct LegacyCleanupCandidate: Equatable, Sendable, Identifiable {
    public let source: String
    public let state: LegacyCleanupState
    public var id: String { source }

    public init(source: String, state: LegacyCleanupState) {
        self.source = source
        self.state = state
    }

    /// The legacy Agent id after `custom:`.
    public var agentID: String { String(source.dropFirst(LegacyCleanupContract.sourcePrefix.count)) }
}

public struct LegacyCleanupReport: Equatable, Sendable {
    public let candidates: [LegacyCleanupCandidate]
    /// Batch directory name under `<data dir>/legacy-agent-quarantine/`; nil for plan.
    public let quarantineBatch: String?

    public init(candidates: [LegacyCleanupCandidate], quarantineBatch: String?) {
        self.candidates = candidates
        self.quarantineBatch = quarantineBatch
    }
}

public struct LegacyCleanupDecodeError: Error, Equatable, Sendable {
    public let reason: String
    public init(_ reason: String) { self.reason = reason }
}

extension LegacyCleanupReport {
    public static func decode(_ value: JSONValue) throws -> LegacyCleanupReport {
        let fields = try exactObject(value, keys: ["schemaVersion", "candidates", "quarantineBatch"], what: "report")
        guard fields["schemaVersion"]?.numberText == String(LegacyCleanupContract.schemaVersion) else {
            throw LegacyCleanupDecodeError("unsupported schemaVersion")
        }
        guard let items = fields["candidates"]?.arrayValue else {
            throw LegacyCleanupDecodeError("candidates must be an array")
        }
        var seen = Set<String>()
        let candidates = try items.map { item -> LegacyCleanupCandidate in
            let candidate = try LegacyCleanupCandidate.decode(item)
            guard seen.insert(candidate.source).inserted else {
                throw LegacyCleanupDecodeError("duplicate source \(candidate.source)")
            }
            return candidate
        }
        let batch: String?
        switch fields["quarantineBatch"] {
        case .null?: batch = nil
        case let .string(name)?:
            guard !name.isEmpty, !name.contains("/"), name != ".", name != ".." else {
                throw LegacyCleanupDecodeError("quarantineBatch must be a bare directory name")
            }
            batch = name
        default: throw LegacyCleanupDecodeError("quarantineBatch must be a string or null")
        }
        return LegacyCleanupReport(candidates: candidates, quarantineBatch: batch)
    }
}

extension LegacyCleanupCandidate {
    static func decode(_ value: JSONValue) throws -> LegacyCleanupCandidate {
        let fields = try exactObject(value, keys: ["source", "state"], what: "candidate")
        guard let source = fields["source"]?.stringValue,
              source.hasPrefix(LegacyCleanupContract.sourcePrefix),
              source.count > LegacyCleanupContract.sourcePrefix.count
        else { throw LegacyCleanupDecodeError("candidate source must be custom:<id>") }
        return LegacyCleanupCandidate(source: source, state: try LegacyCleanupState.decode(fields["state"] ?? .null))
    }
}

extension LegacyCleanupState {
    static func decode(_ value: JSONValue) throws -> LegacyCleanupState {
        switch value["state"]?.stringValue {
        case "eligible":
            _ = try exactObject(value, keys: ["state"], what: "eligible state")
            return .eligible
        case "done":
            _ = try exactObject(value, keys: ["state"], what: "done state")
            return .done
        case "blocked":
            let fields = try exactObject(value, keys: ["state", "reason"], what: "blocked state")
            guard let reason = fields["reason"]?.stringValue else {
                throw LegacyCleanupDecodeError("blocked state needs a reason")
            }
            return .blocked(reason: reason)
        case "partial":
            let fields = try exactObject(value, keys: ["state", "remaining", "reason"], what: "partial state")
            guard let raw = fields["remaining"]?.arrayValue else {
                throw LegacyCleanupDecodeError("partial state needs remaining steps")
            }
            let steps = try raw.map { step -> LegacyCleanupStep in
                guard let name = step.stringValue, let parsed = LegacyCleanupStep(rawValue: name) else {
                    throw LegacyCleanupDecodeError("unknown cleanup step")
                }
                return parsed
            }
            let reason: String?
            switch fields["reason"] {
            case .null?: reason = nil
            case let .string(text)?: reason = text
            default: throw LegacyCleanupDecodeError("partial reason must be a string or null")
            }
            return .partial(remaining: steps, reason: reason)
        default:
            throw LegacyCleanupDecodeError("unknown candidate state")
        }
    }
}

/// The apply request body; ids are validated by the client before sending.
public func legacyCleanupApplyParams(_ sourceIDs: [String]) -> JSONValue {
    .object([
        "schemaVersion": .number(String(LegacyCleanupContract.schemaVersion)),
        "sourceIds": .array(sourceIDs.map(JSONValue.string)),
    ])
}

private func exactObject(_ value: JSONValue, keys: Set<String>, what: String) throws -> [String: JSONValue] {
    guard let fields = value.objectValue, Set(fields.keys) == keys else {
        throw LegacyCleanupDecodeError("\(what) has unexpected shape")
    }
    return fields
}
