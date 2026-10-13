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
public enum CompanionSessionControl: String, CaseIterable, Equatable, Sendable {
    case plan = "session.plan"
    case compact = "session.compact"
    case fork = "session.fork"
    case revert = "session.revert"
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

    public func triggerSessionControl(_ control: CompanionSessionControl, taskId: String) throws {
        _ = try requireMacOnline()
        sentFramesLog.append("\(control.rawValue):\(taskId)")
    }

    public func updateFilesAndGit(files: [CompanionFileEntry], changes: [CompanionGitChange]) {
        self.files = files
        self.gitChanges = changes
    }
}
