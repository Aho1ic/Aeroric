import Foundation

/// AP6: APNs push bridge for the native companion.
///
/// Boundaries this file exists to hold (MIGRATION_PLAN §D AP6, ADR-040, Q7/Q10):
///
/// 1. **A notification is never an authorization.** Nothing here creates an
///    approval, executes a tool, or mutates task state. A push only says "there
///    is something worth re-checking"; the companion must return to the E2EE
///    session and take an authoritative snapshot before it shows anything
///    actionable. `CompanionStore.ingestPushHint` is intentionally a
///    *hint-only* entry point with no path to `pendingApprovals`.
/// 2. **The payload is validated, not trusted.** The sender constructs a
///    minimal `aps` payload (`alert.loc-key` plus `kind`/`eventSeq`); the
///    receiver still re-validates the key set, so a compromised or third-party
///    sender cannot smuggle business content into a notification that the UI
///    would then render.
/// 3. **The device token is the only thing uploaded**, and only over the
///    already-authenticated channel, only while the Mac is online. Nothing is
///    queued while offline: a token registered against a stale epoch would
///    push notifications for work this device is no longer authorized to see.
public enum PushAuthorization: String, Equatable, Sendable {
    case notDetermined
    case denied
    case authorized
    /// Provisional/quiet delivery: authorized, but notifications arrive softly.
    case provisional
}

/// What a validated incoming notification is allowed to contain.
///
/// This mirrors the sender's payload builder. It is a *shape* allowlist: a
/// payload carrying anything else is refused rather than partially rendered,
/// because "render what we understand and ignore the rest" is how a title
/// leaks into a notification body.
public struct PushAlert: Equatable, Sendable {
    public enum Kind: String, Equatable, Sendable {
        case taskDone = "task_done"
        case taskFailed = "task_failed"
        case approvalPending = "approval_pending"
    }

    public var kind: Kind
    public var eventSeq: UInt64

    /// Keys the sender is allowed to put at the top level.
    public static let allowedTopLevelKeys: Set<String> = ["aps", "kind", "eventSeq"]
    /// Keys allowed inside `aps`.
    public static let allowedApsKeys: Set<String> = ["alert", "sound", "badge", "thread-id"]

    /// Parse and validate a raw notification dictionary.
    ///
    /// Returns `nil` for anything that is not a well-formed minimal alert. The
    /// caller must treat `nil` as "ignore", never as "show a generic notice":
    /// a malformed payload means the notification did not come from our sender.
    public static func parse(userInfo: [String: Any]) -> PushAlert? {
        let keys = Set(userInfo.keys)
        guard keys.isSubset(of: allowedTopLevelKeys) else { return nil }
        guard let aps = userInfo["aps"] as? [String: Any],
              Set(aps.keys).isSubset(of: allowedApsKeys) else { return nil }
        guard let rawKind = userInfo["kind"] as? String,
              let kind = Kind(rawValue: rawKind) else { return nil }
        // `eventSeq` arrives as a number; JSON booleans are not numbers here.
        let seq: UInt64
        if let number = userInfo["eventSeq"] as? NSNumber {
            seq = number.uint64Value
        } else if let text = userInfo["eventSeq"] as? String, let parsed = UInt64(text) {
            seq = parsed
        } else {
            return nil
        }
        // The visible text must be a bundle key, not content. Either shape is
        // accepted because both keep the text local; a literal `body` string
        // is refused.
        if let alert = aps["alert"] {
            if let text = alert as? String {
                // A bare string is only acceptable if it is one of our keys.
                guard text.hasPrefix("push.") else { return nil }
            } else if let dictionary = alert as? [String: Any] {
                let allowed: Set<String> = ["loc-key", "loc-args", "title-loc-key"]
                guard Set(dictionary.keys).isSubset(of: allowed) else { return nil }
                if let key = dictionary["loc-key"] as? String, !key.hasPrefix("push.") {
                    return nil
                }
            } else {
                return nil
            }
        }
        return PushAlert(kind: kind, eventSeq: seq)
    }
}

/// Where a registration request is in its lifecycle.
public enum PushRegistrationState: Equatable, Sendable {
    case unpaired
    /// Paired but the Mac is offline: registration is refused, not queued.
    case macOffline
    case registered(deviceId: String)
    case failed(reason: String)
}

/// The result of a `push.register` attempt.
public struct PushRegistrationOutcome: Equatable, Sendable {
    public var state: PushRegistrationState
    /// The APNs device token this outcome refers to (hex). Kept so the caller
    /// can tell "the host acknowledged *this* token" from a stale reply.
    public var deviceToken: String
}

public enum PushBridgeError: Error, Equatable, Sendable {
    case notPaired
    case macOfflineNoQueueing
    case invalidDeviceToken
    case registrationRefused(String)
}

/// Uploads the APNs device token to the paired Mac over the authenticated
/// channel. It deliberately has no API to upload anything else.
public final class PushRegistrationClient: @unchecked Sendable {
    /// The transport call, injected: production wires it to the E2EE RPC
    /// client, tests supply a recorder. The closure receives the *validated*
    /// hex token and must return the host's reply.
    public typealias Upload = @Sendable (_ deviceToken: String, _ deviceId: String, _ platform: String) async throws -> String

    private let upload: Upload

    public init(upload: @escaping Upload) {
        self.upload = upload
    }

    /// Validate then upload. Refuses rather than uploading a token it could not
    /// verify: a malformed token would be bound to this account by the sender
    /// and then rejected by APNs forever, hiding the real problem.
    public func register(
        deviceToken: Data,
        deviceId: String,
        platform: String
    ) async throws -> PushRegistrationOutcome {
        let hex = PushRegistrationClient.hexToken(deviceToken)
        guard !hex.isEmpty, hex.count <= 200,
              hex.allSatisfy({ $0.isHexDigit }) else {
            throw PushBridgeError.invalidDeviceToken
        }
        do {
            let reply = try await upload(hex, deviceId, platform)
            _ = reply
            return PushRegistrationOutcome(
                state: .registered(deviceId: deviceId),
                deviceToken: hex
            )
        } catch {
            throw PushBridgeError.registrationRefused(String(describing: error))
        }
    }

    /// Unbind: an empty token tells the host (and therefore the sender) to stop
    /// pushing to this device.
    public func unregister(deviceId: String, platform: String) async throws -> PushRegistrationOutcome {
        do {
            _ = try await upload("", deviceId, platform)
            return PushRegistrationOutcome(state: .registered(deviceId: deviceId), deviceToken: "")
        } catch {
            throw PushBridgeError.registrationRefused(String(describing: error))
        }
    }

    /// APNs hands back raw bytes; the wire format (and APNs' own `:path`) is
    /// lowercase hex. Validate before it ever reaches a URL.
    public static func hexToken(_ token: Data) -> String {
        token.map { String(format: "%02x", $0) }.joined()
    }
}