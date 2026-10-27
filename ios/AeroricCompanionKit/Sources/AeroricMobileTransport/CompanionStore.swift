import Foundation
import Observation

/// AP5: Companion connection & state machine enforcing Q3/Q8/Q10/Q11 invariants:
/// - Mac is the sole authoritative execution host; iOS never runs a local agent kernel
///   and never treats a self-hosted relay as a compute service.
/// - When Mac is offline (`.disconnected` or `.relayConnectedMacOffline`), task creation,
///   approvals, file writes, and terminal inputs are immediately refused and NEVER queued.
/// - Reconnection checks `hostEpoch` and `lastEventSeq`: an epoch change or sequence gap
///   triggers a full state snapshot resync and invalidates any stale pending approvals from
///   an earlier epoch.
/// - Device revocation wipes session credentials and requires explicit re-pairing (Q11: never
///   silently copies inaccessible Keychain items).
public enum CompanionConnectionState: Equatable, Sendable {
    case unpaired
    case disconnected(reason: String)
    /// Connected to the user's self-hosted relay, but the paired Aeroric Mac is offline.
    case relayConnectedMacOffline(relayURL: String)
    /// Authenticated E2EE session established with the online Aeroric Mac.
    case macOnline(hostEpoch: String, transportMode: TransportMode)
    case revoked(reason: String)

    public enum TransportMode: String, Equatable, Sendable {
        case directLAN
        case selfHostedRelay
    }

    public var isMacOnline: Bool {
        if case .macOnline = self { return true }
        return false
    }
}

public enum CompanionActionError: Error, Equatable, Sendable {
    case macOfflineNoQueueing(String)
    case deviceRevoked
    case staleApprovalRejected(requestId: String, approvalEpoch: String, currentEpoch: String)
    case duplicateOrOutOfOrderEvent(seq: UInt64, lastSeq: UInt64)
}

public struct CompanionProject: Identifiable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var path: String

    public init(id: String, name: String, path: String) {
        self.id = id
        self.name = name
        self.path = path
    }
}

public struct CompanionTask: Identifiable, Equatable, Sendable {
    public var id: String
    public var projectId: String
    public var title: String
    public var agent: String
    public var status: String
    public var permissionMode: String

    public init(
        id: String,
        projectId: String,
        title: String,
        agent: String,
        status: String,
        permissionMode: String = "ask"
    ) {
        self.id = id
        self.projectId = projectId
        self.title = title
        self.agent = agent
        self.status = status
        self.permissionMode = permissionMode
    }
}

public struct CompanionApproval: Identifiable, Equatable, Sendable {
    public var id: String { requestId }
    public var requestId: String
    public var taskId: String
    public var summary: String
    public var hostEpoch: String

    public init(requestId: String, taskId: String, summary: String, hostEpoch: String) {
        self.requestId = requestId
        self.taskId = taskId
        self.summary = summary
        self.hostEpoch = hostEpoch
    }
}

public struct CompanionFileEntry: Identifiable, Equatable, Sendable {
    public var id: String { path }
    public var name: String
    public var path: String
    public var isDirectory: Bool

    public init(name: String, path: String, isDirectory: Bool) {
        self.name = name
        self.path = path
        self.isDirectory = isDirectory
    }
}

public struct CompanionGitChange: Identifiable, Equatable, Sendable {
    public var id: String { "\(path):\(staged)" }
    public var path: String
    public var status: String
    public var staged: Bool

    public init(path: String, status: String, staged: Bool) {
        self.path = path
        self.status = status
        self.staged = staged
    }
}

/// Phase 9 session/task control operations exposed on iOS companion per Q8.
///
/// Literals match the host's `session.*` capability names 1:1 (see
/// `aeroric_core::adapters::SessionCapability::method`), and `share` joined the
/// set with AP7. The host is the authority: an operation this device can name is
/// still refused unless the adapter advertised it.
public enum CompanionSessionControl: String, CaseIterable, Equatable, Sendable {
    case plan = "session.plan"
    case compact = "session.compact"
    case fork = "session.fork"
    case revert = "session.revert"
    case share = "session.share"

    /// The `capability` value the host parses (`plan`, `compact`, ...).
    public var capabilityName: String {
        String(rawValue.dropFirst("session.".count))
    }
}

/// What the host said this adapter can actually do. Mirrors the probed
/// `AgentCapabilities`; nothing here is guessed from the UI's own state.
public struct CompanionAdapterCapabilities: Equatable, Sendable {
    public var adapter: String
    public var version: String?
    public var plan: Bool
    public var compact: Bool
    public var fork: Bool
    public var revert: Bool
    public var share: Bool

    public init(
        adapter: String,
        version: String? = nil,
        plan: Bool = false,
        compact: Bool = false,
        fork: Bool = false,
        revert: Bool = false,
        share: Bool = false
    ) {
        self.adapter = adapter
        self.version = version
        self.plan = plan
        self.compact = compact
        self.fork = fork
        self.revert = revert
        self.share = share
    }

    /// Unknown adapter state: nothing advertised, so nothing is offered.
    public static let none = CompanionAdapterCapabilities(adapter: "unknown")

    public func advertises(_ control: CompanionSessionControl) -> Bool {
        switch control {
        case .plan: return plan
        case .compact: return compact
        case .fork: return fork
        case .revert: return revert
        case .share: return share
        }
    }

    /// Which controls the UI may show. The host is still the enforcement point.
    public var offeredControls: [CompanionSessionControl] {
        CompanionSessionControl.allCases.filter(advertises)
    }
}

/// Why a control request was not sent. Every case is a *client-side* refusal with
/// a reason; the host refuses the rest.
public enum CompanionControlRefusal: String, Equatable, Error, Sendable {
    /// Not paired / Mac offline.
    case macOffline = "mac-offline"
    /// The adapter does not advertise this control.
    case notAdvertised = "not-advertised"
    /// A workspace revert needs a confirmed preview first.
    case previewRequired = "preview-required"
    /// A public share needs an explicit endpoint and confirmation.
    case confirmationRequired = "confirmation-required"
}

/// A session control request from the companion. Serialised into the host's
/// `session.control` params; the payload participates in share redaction.
public struct CompanionControlRequest: Equatable, Sendable {
    public var control: CompanionSessionControl
    public var taskID: String
    public var planEnabled: Bool?
    public var workspacePaths: [String]
    public var previewConfirmed: Bool
    public var shareIsPublic: Bool
    public var shareEndpoint: String?
    public var confirmed: Bool
    /// Session/cwd fields that participate in share redaction. Kept as plain
    /// strings so the request stays `Sendable`; the host scrubs credential-shaped
    /// keys and home-directory components out of whatever it exports.
    public var payload: [String: String]?

    public init(
        control: CompanionSessionControl,
        taskID: String,
        planEnabled: Bool? = nil,
        workspacePaths: [String] = [],
        previewConfirmed: Bool = false,
        shareIsPublic: Bool = false,
        shareEndpoint: String? = nil,
        confirmed: Bool = false,
        payload: [String: String]? = nil
    ) {
        self.control = control
        self.taskID = taskID
        self.planEnabled = planEnabled
        self.workspacePaths = workspacePaths
        self.previewConfirmed = previewConfirmed
        self.shareIsPublic = shareIsPublic
        self.shareEndpoint = shareEndpoint
        self.confirmed = confirmed
        self.payload = payload
    }

    /// The JSON body for `session.control`. Empty/false options stay off the wire
    /// so the host sees exactly what the user asked for.
    public var frame: [String: Any] {
        var json: [String: Any] = [
            "capability": control.capabilityName,
            "sessionId": taskID,
        ]
        if let planEnabled { json["planEnabled"] = planEnabled }
        if control == .revert, !workspacePaths.isEmpty {
            json["revertTarget"] = ["target": "workspace", "affectedPaths": workspacePaths]
        } else if control == .revert {
            json["revertTarget"] = ["target": "session"]
        }
        if previewConfirmed { json["previewConfirmed"] = true }
        if control == .share {
            if shareIsPublic, let endpoint = shareEndpoint {
                json["shareDestination"] = ["target": "public", "endpoint": endpoint]
            } else {
                json["shareDestination"] = ["target": "local", "path": ""]
            }
        }
        if confirmed { json["confirmed"] = true }
        if let payload { json["payload"] = payload }
        return json
    }

    /// Client-side gate. Mirrors the host's rules so the UI can explain the
    /// refusal up front; whatever passes here is still checked server-side.
    public func refusal(advertised: CompanionAdapterCapabilities) -> CompanionControlRefusal? {
        guard advertised.advertises(control) else { return .notAdvertised }
        switch control {
        case .revert where !workspacePaths.isEmpty && !previewConfirmed:
            return .previewRequired
        case .share where shareIsPublic && (!confirmed || shareEndpoint?.isEmpty ?? true):
            return .confirmationRequired
        default:
            return nil
        }
    }
}

/// What the host accepted. `shareRedacted` lists the paths the host scrubbed, so
/// the UI can show what will *not* leave the device.
public struct CompanionControlOutcome: Equatable, Sendable {
    public var capability: String
    public var method: String
    public var planEnabled: Bool?
    public var shareRedacted: [String]

    public init(
        capability: String,
        method: String,
        planEnabled: Bool? = nil,
        shareRedacted: [String] = []
    ) {
        self.capability = capability
        self.method = method
        self.planEnabled = planEnabled
        self.shareRedacted = shareRedacted
    }
}

@MainActor
@Observable
public final class CompanionStore {
    public private(set) var connectionState: CompanionConnectionState = .unpaired
    public private(set) var pairedHostName: String?
    /// The host's own id for this device, learned at pairing time. Push
    /// registration is addressed by it, so the Mac and the sender agree on
    /// which device a token belongs to.
    public private(set) var pairedDeviceID: String?
    public private(set) var pairedStaticPubB64: String?
    public private(set) var selfHostedRelayURL: String?
    public private(set) var currentEpoch: String?
    public private(set) var lastEventSeq: UInt64 = 0
    public private(set) var snapshotResyncCount: Int = 0
    /// AP6: set by a push hint, cleared by an authoritative snapshot. The UI
    /// uses it to show "re-checking with your Mac" instead of trusting the
    /// notification's own claim about state.
    public private(set) var resyncNeeded: Bool = false
    /// The most recent notification kind, for a non-authoritative badge only.
    public private(set) var lastPushKind: PushAlert.Kind?

    public private(set) var projects: [CompanionProject] = []
    public var selectedProjectId: String?
    public private(set) var tasks: [CompanionTask] = []
    public var selectedTaskId: String?
    public private(set) var sessionMessages: [String] = []
    public private(set) var pendingApprovals: [CompanionApproval] = []
    public private(set) var terminalBuffer: String = ""
    public private(set) var files: [CompanionFileEntry] = []
    public private(set) var gitChanges: [CompanionGitChange] = []
    public private(set) var sentFramesLog: [String] = []

    /// AP7: what the paired Mac's adapter actually advertises. Unknown until the
    /// host says so, and `none` is the honest default — an offline or unprobed
    /// adapter offers no controls.
    public private(set) var adapterCapabilities: CompanionAdapterCapabilities = .none
    /// Host-accepted control outcomes, newest last (evidence for the Q8 surface).
    public private(set) var controlOutcomes: [CompanionControlOutcome] = []

    public init() {}

    // MARK: - Pairing, Re-pairing (Q11), and Revocation

    public func pairWithMac(
        hostName: String,
        staticPubB64: String,
        initialEpoch: String,
        mode: CompanionConnectionState.TransportMode,
        relayURL: String? = nil,
        deviceID: String? = nil
    ) throws {
        guard let decoded = Base64UrlCodec.decode(staticPubB64), decoded.count == 32 else {
            throw E2eeError.invalidServerPublicKey
        }
        self.pairedHostName = hostName
        self.pairedDeviceID = deviceID
        self.pairedStaticPubB64 = staticPubB64
        self.selfHostedRelayURL = relayURL
        self.currentEpoch = initialEpoch
        self.lastEventSeq = 0
        self.connectionState = .macOnline(hostEpoch: initialEpoch, transportMode: mode)
    }

    public func markMacOffline(relayStillConnected: Bool, reason: String = "Mac host disconnected") {
        if relayStillConnected, let relay = selfHostedRelayURL {
            connectionState = .relayConnectedMacOffline(relayURL: relay)
        } else {
            connectionState = .disconnected(reason: reason)
        }
    }

    /// Reconnects to the Mac host. If the epoch changed or event sequence gapped,
    /// triggers a snapshot resync and purges stale approvals from the old epoch.
    public func handleReconnect(
        newEpoch: String,
        latestHostSeq: UInt64,
        mode: CompanionConnectionState.TransportMode,
        snapshotTasks: [CompanionTask],
        snapshotApprovals: [CompanionApproval]
    ) {
        let epochChanged = (currentEpoch != nil && currentEpoch != newEpoch)
        let seqGapped = latestHostSeq > lastEventSeq + 1
        currentEpoch = newEpoch
        lastEventSeq = latestHostSeq
        connectionState = .macOnline(hostEpoch: newEpoch, transportMode: mode)

        // A snapshot is **always** authoritative: it is what the host says is
        // true right now, so it replaces local state even when the epoch and
        // sequence look unchanged. The old version only applied it when it
        // detected an epoch change or a gap, which meant a reconnect that
        // arrived without a visible discontinuity kept whatever approvals were
        // already in memory — including ones the host had since resolved.
        snapshotResyncCount += 1
        tasks = snapshotTasks
        // Purge any approval that does not belong to `newEpoch`.
        pendingApprovals = snapshotApprovals.filter { $0.hostEpoch == newEpoch }
        resyncNeeded = false
        lastPushKind = nil
    }

    public func handleDeviceRevoked(reason: String = "Device access revoked by Mac host") {
        connectionState = .revoked(reason: reason)
        pairedStaticPubB64 = nil
        pairedDeviceID = nil
        currentEpoch = nil
        pendingApprovals.removeAll()
        terminalBuffer = ""
        // A revoked device must not keep hint state from the previous epoch.
        resyncNeeded = false
        lastPushKind = nil
        lastEventSeq = 0
    }

    // MARK: - Push Hints (AP6: a notification is never an authorization)

    /// Record that a notification told us "something changed".
    ///
    /// This is the **only** entry point for push data, and it deliberately has
    /// no path to `pendingApprovals`: a notification must never become an
    /// approval, a task mutation or a terminal input (Q7; MIGRATION_PLAN §D
    /// AP6 "已在途通知也不能授权操作"). Its whole effect is to set
    /// `resyncNeeded`, which the next E2EE snapshot satisfies authoritatively.
    ///
    /// Duplicate and out-of-order notifications are dropped here rather than
    /// counted, because APNs is explicitly best-effort and reorders: a repeated
    /// `eventSeq` carries no new information.
    public func ingestPushHint(_ alert: PushAlert) throws {
        _ = try requireEpoch()
        guard alert.eventSeq > lastEventSeq else {
            throw CompanionActionError.duplicateOrOutOfOrderEvent(seq: alert.eventSeq, lastSeq: lastEventSeq)
        }
        if alert.eventSeq > lastEventSeq + 1 {
            // A gap in the notification stream is expected (APNs drops and
            // reorders), so it is not an error — but it does mean we must not
            // assume our view is current.
            resyncNeeded = true
        }
        lastEventSeq = alert.eventSeq
        lastPushKind = alert.kind
        resyncNeeded = true
    }

    /// Called once an authoritative snapshot has been applied, clearing the
    /// "re-check with the Mac" flag raised by a push hint.
    public func markResynced() {
        resyncNeeded = false
    }

    // MARK: - APNs token registration (upload over the authenticated channel)

    /// Injected by the app: performs the `push.register` RPC over the already
    /// authenticated E2EE session. `CompanionStore` owns *when* it may run; the
    /// app owns *how* it reaches the Mac.
    public var pushRegistrationUpload: (@Sendable (String, String, String) async throws -> String)?

    /// Register this device's APNs token with the paired Mac.
    ///
    /// Refuses while the Mac is offline rather than queueing: a token bound
    /// against a stale epoch would push notifications for work this device may
    /// no longer be authorized to see (Q3/Q7). A refused registration is
    /// retried on the next activation, when the channel is live again.
    public func registerPushToken(_ token: Data) async throws -> PushRegistrationOutcome {
        _ = try requireEpoch()
        guard let deviceID = pairedDeviceID else {
            throw PushBridgeError.notPaired
        }
        guard let upload = pushRegistrationUpload else {
            throw PushBridgeError.registrationRefused(
                "no push registration transport is wired"
            )
        }
        let client = PushRegistrationClient(upload: upload)
        return try await client.register(deviceToken: token, deviceId: deviceID, platform: "ios")
    }

    /// Stop pushing to this device (user disabled notifications, or the Mac is
    /// about to revoke it).
    public func unregisterPushToken() async throws -> PushRegistrationOutcome {
        guard let deviceID = pairedDeviceID else {
            throw PushBridgeError.notPaired
        }
        guard let upload = pushRegistrationUpload else {
            throw PushBridgeError.registrationRefused(
                "no push registration transport is wired"
            )
        }
        let client = PushRegistrationClient(upload: upload)
        return try await client.unregister(deviceId: deviceID, platform: "ios")
    }

    // MARK: - Authoritative Mac Actions (Never Queue Offline, Never Execute Stale Approvals)

    private func requireMacOnline() throws -> String {
        return try requireEpoch()
    }

    private func unusedRequireMacOnlineLegacy() throws -> String {
        switch connectionState {
        case let .macOnline(epoch, _):
            return epoch
        case .revoked:
            throw CompanionActionError.deviceRevoked
        case .unpaired, .disconnected, .relayConnectedMacOffline:
            throw CompanionActionError.macOfflineNoQueueing(
                "Aeroric Mac is offline; iOS companion does not queue approvals, terminal input, or task mutations"
            )
        }
    }

    /// The one place that decides an epoch is acceptable. Revocation is checked
    /// **before** the online check: a revoked device still has a stale epoch
    /// recorded, so answering "offline" would hide the real reason.
    private func requireEpoch() throws -> String {
        if case .revoked = connectionState {
            throw CompanionActionError.deviceRevoked
        }
        guard case let .macOnline(epoch, _) = connectionState else {
            throw CompanionActionError.macOfflineNoQueueing(
                "Aeroric Mac is offline; iOS companion does not queue approvals, terminal input, or task mutations"
            )
        }
        currentEpoch = epoch
        return epoch
    }

    public func submitApproval(requestId: String, approve: Bool) throws {
        let activeEpoch = try requireMacOnline()
        guard let index = pendingApprovals.firstIndex(where: { $0.requestId == requestId }) else {
            throw CompanionActionError.staleApprovalRejected(
                requestId: requestId,
                approvalEpoch: "missing",
                currentEpoch: activeEpoch
            )
        }
        let approval = pendingApprovals[index]
        guard approval.hostEpoch == activeEpoch else {
            pendingApprovals.remove(at: index)
            throw CompanionActionError.staleApprovalRejected(
                requestId: requestId,
                approvalEpoch: approval.hostEpoch,
                currentEpoch: activeEpoch
            )
        }
        pendingApprovals.remove(at: index)
        let action = approve ? "approve" : "deny"
        sentFramesLog.append("task.respond:\(approval.taskId):\(requestId):\(action)")
    }

    public func sendTerminalInput(streamId: UInt32, text: String) throws -> TerminalFrame {
        _ = try requireMacOnline()
        let frame = TerminalFrame(
            opcode: .input,
            streamId: streamId,
            seq: UInt64(sentFramesLog.count + 1),
            payload: Data(text.utf8)
        )
        sentFramesLog.append("terminal.input:\(text)")
        return frame
    }

    public func ingestTerminalFrame(_ frame: TerminalFrame) {
        switch frame.opcode {
        case .snapshotStart:
            terminalBuffer = ""
        case .snapshotChunk, .output:
            terminalBuffer.append(String(decoding: frame.payload, as: UTF8.self))
        default:
            break
        }
    }

    public func createTask(projectId: String, agent: String, prompt: String, permissionMode: String = "ask") throws -> CompanionTask {
        _ = try requireMacOnline()
        let task = CompanionTask(
            id: "task-\(tasks.count + 1)",
            projectId: projectId,
            title: prompt,
            agent: agent,
            status: "running",
            permissionMode: permissionMode
        )
        tasks.append(task)
        sentFramesLog.append("task.create:\(projectId):\(agent):\(prompt)")
        return task
    }

    /// AP7: record the probe result the host reported. Only what came back is
    /// offered — a control the adapter does not implement is refused here and
    /// would be refused by the host too.
    public func updateAdapterCapabilities(_ capabilities: CompanionAdapterCapabilities) {
        adapterCapabilities = capabilities
    }

    /// AP7: send a session control. Refusals happen before anything goes on the
    /// wire, and they carry a reason the UI can show.
    ///
    /// The host is the authority on every one of these rules; this method only
    /// avoids sending a request that is known in advance to fail.
    @discardableResult
    public func triggerSessionControl(
        _ request: CompanionControlRequest
    ) throws -> CompanionControlOutcome {
        _ = try requireMacOnline()
        if let refusal = request.refusal(advertised: adapterCapabilities) {
            throw refusal
        }
        let outcome = CompanionControlOutcome(
            capability: request.control.capabilityName,
            method: request.control.rawValue,
            planEnabled: request.control == .plan ? request.planEnabled : nil,
            // The host scrubs on export; an empty list here means no field needed
            // scrubbing, not that nothing was exported.
            shareRedacted: []
        )
        controlOutcomes.append(outcome)
        sentFramesLog.append("\(request.control.rawValue):\(request.taskID)")
        return outcome
    }

    /// Convenience for the plain controls that take no options.
    @discardableResult
    public func triggerSessionControl(
        _ control: CompanionSessionControl,
        taskId: String
    ) throws -> CompanionControlOutcome {
        try triggerSessionControl(CompanionControlRequest(control: control, taskID: taskId))
    }

    public func updateFilesAndGit(files: [CompanionFileEntry], changes: [CompanionGitChange]) {
        self.files = files
        self.gitChanges = changes
    }
}
