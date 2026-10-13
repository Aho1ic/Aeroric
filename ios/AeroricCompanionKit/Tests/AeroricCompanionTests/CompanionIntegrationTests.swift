@testable import AeroricMobileTransport
@testable import AeroricMobileUI
import XCTest

final class CompanionIntegrationTests: XCTestCase {
    // MARK: - 1. E2EE v2 Handshake, Static Key Pinning & Replay Protection

    func testE2eeV2HandshakeConfirmPinningFrameEncryptionAndReplayRejection() throws {
        let server = ServerHandshakeResponder()
        let pendingClient = try PendingClientHandshake.start(serverStaticPubBase64Url: server.publicBase64Url)
        let (ackJSON, serverSession) = try server.respond(helloJSON: pendingClient.helloJSON)
        let clientSession = try pendingClient.finish(ackJSON: ackJSON)

        // Control frame client -> server
        let reqPlain = Data("{\"v\":3,\"type\":\"request\",\"id\":\"1\",\"method\":\"projects.list\",\"params\":{}}".utf8)
        let encryptedCtrl = try clientSession.encryptFrame(kind: .control, plaintext: reqPlain)
        let (decKind, decPlain) = try serverSession.decryptFrame(encryptedCtrl)
        XCTAssertEqual(decKind, .control)
        XCTAssertEqual(decPlain, reqPlain)

        // Replaying the exact same frame must be rejected by strict monotonic sequence check!
        XCTAssertThrowsError(try serverSession.decryptFrame(encryptedCtrl)) { error in
            XCTAssertEqual(error as? E2eeError, .outOfOrderFrame(expected: 1, received: 0))
        }

        // Terminal frame server -> client
        let termFrame = TerminalFrame(
            opcode: .output,
            streamId: 3,
            seq: 1,
            payload: Data("hello from Mac PTY\r\n".utf8)
        ).encode()
        let encryptedTerm = try serverSession.encryptFrame(kind: .terminal, plaintext: termFrame)
        let (termKind, termPlain) = try clientSession.decryptFrame(encryptedTerm)
        XCTAssertEqual(termKind, .terminal)
        XCTAssertEqual(TerminalFrame.decode(termPlain)?.opcode, .output)

        // MITM / wrong static server key must fail confirm verification!
        let attackerServer = ServerHandshakeResponder()
        let victimPending = try PendingClientHandshake.start(serverStaticPubBase64Url: server.publicBase64Url)
        let (attackerAck, _) = try attackerServer.respond(helloJSON: victimPending.helloJSON)
        XCTAssertThrowsError(try victimPending.finish(ackJSON: attackerAck)) { error in
            XCTAssertEqual(error as? E2eeError, .hostIdentityVerificationFailed)
        }
    }

    // MARK: - 2. Binary Terminal Frame & RPC v2/v3 Codec

    func testTerminalBinaryFrameRoundTripAndImePayload() throws {
        for opcode in TerminalOpcode.allCases {
            let payload = Data("中文 IME 输入测试 · \(opcode.rawValue)".utf8)
            let frame = TerminalFrame(opcode: opcode, streamId: 42, seq: 99, payload: payload)
            let encoded = frame.encode()
            XCTAssertEqual(encoded.count, TerminalFrame.headerBytes + payload.count)
            let decoded = TerminalFrame.decode(encoded)
            XCTAssertEqual(decoded, frame)
        }

        // Oversized payload (> 64 KiB) is rejected
        let oversized = TerminalFrame(
            opcode: .output,
            streamId: 1,
            seq: 1,
            payload: Data(repeating: 0x41, count: TerminalFrame.maxPayloadBytes + 1)
        ).encode()
        XCTAssertNil(TerminalFrame.decode(oversized))

        // RPC v2 & v3 envelope encoding/decoding
        let v3Req = try RemoteRpcCodec.encodeRequest(version: .v3, id: "req-7", method: "git.changes", params: ["projectId": "p1"])
        XCTAssertTrue(v3Req.contains("\"v\":3"))
        let pushEnv = RemoteRpcCodec.decodeEnvelope("{\"v\":3,\"type\":\"push\",\"event\":\"session.appended\",\"seq\":12,\"data\":{\"text\":\"ok\"}}")
        if case let .push(event, seq, _) = pushEnv {
            XCTAssertEqual(event, "session.appended")
            XCTAssertEqual(seq, 12)
        } else {
            XCTFail("Expected decoded push envelope")
        }
    }

    // MARK: - 3. Direct LAN & Self-Hosted Relay + Mac Offline Refusal (No Queueing)

    @MainActor
    func testDirectAndSelfHostedRelayOperationChainAndMacOfflineNoQueueing() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()

        try store.pairWithMac(
            hostName: "Studio-Mac",
            staticPubB64: server.publicBase64Url,
            initialEpoch: "epoch-100",
            mode: .selfHostedRelay,
            relayURL: "wss://relay.user-owned.example/ws"
        )
        XCTAssertTrue(store.connectionState.isMacOnline)

        // Create task, ingest approval & terminal output, send IME input, trigger Phase 9 control
        let task = try store.createTask(projectId: "p1", agent: "claude", prompt: "Fix bug")
        // AP6: a push hint is a *hint*. It must not fabricate an approval — the
        // approval has to come from the authoritative snapshot over E2EE.
        try store.ingestPushHint(PushAlert(kind: .approvalPending, eventSeq: 1))
        XCTAssertTrue(store.pendingApprovals.isEmpty, "a notification must never create an approval")
        XCTAssertTrue(store.resyncNeeded, "the hint must ask for an authoritative re-check")

        // The re-check arrives as a snapshot; only then is there an approval.
        store.handleReconnect(
            newEpoch: "epoch-100",
            latestHostSeq: 1,
            mode: .selfHostedRelay,
            snapshotTasks: [task],
            snapshotApprovals: [
                CompanionApproval(requestId: "req-1", taskId: task.id, summary: "Run bash test", hostEpoch: "epoch-100")
            ]
        )
        XCTAssertEqual(store.pendingApprovals.count, 1)
        store.markResynced()
        XCTAssertFalse(store.resyncNeeded)

        try store.submitApproval(requestId: "req-1", approve: true)
        XCTAssertTrue(store.pendingApprovals.isEmpty)

        store.ingestTerminalFrame(TerminalFrame(opcode: .snapshotStart, streamId: 1, seq: 1, payload: Data()))
        store.ingestTerminalFrame(TerminalFrame(opcode: .output, streamId: 1, seq: 2, payload: Data(" Cargo test ok\n".utf8)))
        XCTAssertEqual(store.terminalBuffer, " Cargo test ok\n")

        _ = try store.sendTerminalInput(streamId: 1, text: "ls -la\r")
        try store.triggerSessionControl(.compact, taskId: task.id)

        let logCountWhileOnline = store.sentFramesLog.count
        XCTAssertEqual(logCountWhileOnline, 4)

        // Now Mac goes offline while self-hosted relay is still connected:
        // Relay is NOT a compute host — all mutations/approvals/terminal inputs must be refused and NOT queued!
        store.markMacOffline(relayStillConnected: true)
        XCTAssertFalse(store.connectionState.isMacOnline)
        XCTAssertEqual(
            store.connectionState,
            .relayConnectedMacOffline(relayURL: "wss://relay.user-owned.example/ws")
        )

        XCTAssertThrowsError(try store.sendTerminalInput(streamId: 1, text: "queued?")) { error in
            guard case .macOfflineNoQueueing = error as? CompanionActionError else {
                XCTFail("Expected macOfflineNoQueueing, got \(error)")
                return
            }
        }
        XCTAssertThrowsError(try store.createTask(projectId: "p1", agent: "codex", prompt: "offline task"))
        XCTAssertThrowsError(try store.submitApproval(requestId: "req-1", approve: true))
        XCTAssertEqual(store.sentFramesLog.count, logCountWhileOnline, "Offline actions must never be queued")
    }

    // MARK: - 4. Reconnect Epoch/Snapshot Resync, Stale Approval Rejection & Revocation

    @MainActor
    func testReconnectEpochResyncStaleApprovalRejectionAndDeviceRevocation() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()
        try store.pairWithMac(
            hostName: "MacBook-Pro",
            staticPubB64: server.publicBase64Url,
            initialEpoch: "epoch-A",
            mode: .directLAN
        )

        // Seq 1 arrives; duplicate seq 1 is rejected, and neither creates an approval.
        try store.ingestPushHint(PushAlert(kind: .approvalPending, eventSeq: 1))
        XCTAssertThrowsError(try store.ingestPushHint(PushAlert(kind: .approvalPending, eventSeq: 1))) { error in
            XCTAssertEqual(error as? CompanionActionError, .duplicateOrOutOfOrderEvent(seq: 1, lastSeq: 1))
        }
        XCTAssertTrue(store.pendingApprovals.isEmpty, "notifications never become approvals")

        // Reconnect after Mac host restarted into `epoch-B`:
        // Snapshot resync runs and purges the stale `epoch-A` approval!
        store.handleReconnect(
            newEpoch: "epoch-B",
            latestHostSeq: 10,
            mode: .directLAN,
            snapshotTasks: [CompanionTask(id: "t1", projectId: "p1", title: "Resynced", agent: "omp", status: "running")],
            snapshotApprovals: [
                CompanionApproval(requestId: "req-1", taskId: "t1", summary: "Stale from epoch-A", hostEpoch: "epoch-A"),
                CompanionApproval(requestId: "req-10", taskId: "t1", summary: "Fresh in epoch-B", hostEpoch: "epoch-B"),
            ]
        )
        XCTAssertEqual(store.snapshotResyncCount, 1)
        XCTAssertEqual(store.pendingApprovals.map(\.requestId), ["req-10"], "Stale epoch-A approval must be purged")

        // Attempting to answer the purged stale approval is rejected!
        XCTAssertThrowsError(try store.submitApproval(requestId: "req-1", approve: true)) { error in
            XCTAssertEqual(
                error as? CompanionActionError,
                .staleApprovalRejected(requestId: "req-1", approvalEpoch: "missing", currentEpoch: "epoch-B")
            )
        }
        // Answering the fresh epoch-B approval succeeds!
        try store.submitApproval(requestId: "req-10", approve: true)

        // Device revocation clears credentials and blocks actions until Q11 re-pairing.
        store.handleDeviceRevoked()
        XCTAssertEqual(store.connectionState, .revoked(reason: "Device access revoked by Mac host"))
        XCTAssertThrowsError(try store.sendTerminalInput(streamId: 1, text: "ls")) { error in
            XCTAssertEqual(error as? CompanionActionError, .deviceRevoked)
        }

        // Q11 re-pairing restores online state cleanly.
        try store.pairWithMac(
            hostName: "MacBook-Pro",
            staticPubB64: server.publicBase64Url,
            initialEpoch: "epoch-C",
            mode: .directLAN
        )
        XCTAssertTrue(store.connectionState.isMacOnline)
    }

    // MARK: - 5. Adaptive iPhone / iPad Views & Liquid Glass

    @MainActor
    func testAdaptiveIPhoneAndIPadCompanionViewsAndLiquidGlass() {
        let store = CompanionStore()
        store.updateFilesAndGit(
            files: [CompanionFileEntry(name: "main.swift", path: "/repo/main.swift", isDirectory: false)],
            changes: [CompanionGitChange(path: "main.swift", status: "M", staged: false)]
        )
        let iphoneView = CompanionRootView(store: store, isIPadLayout: false, reduceTransparency: false)
        let ipadView = CompanionRootView(store: store, isIPadLayout: true, reduceTransparency: true)
        XCTAssertNotNil(iphoneView.body)
        XCTAssertNotNil(ipadView.body)
        XCTAssertEqual(CompanionTerminalPane.accessoryKeys.count, 5)
    }
}
