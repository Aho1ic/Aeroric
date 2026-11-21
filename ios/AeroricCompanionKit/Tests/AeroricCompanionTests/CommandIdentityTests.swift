@testable import AeroricMobileTransport
@testable import AeroricMobileUI
import XCTest

/// D4 (2026-11-21): commands carry a stable id and the host start they were
/// decided on; a gap or a stale refusal means resync before trusting anything.
final class CommandIdentityTests: XCTestCase {
    @MainActor
    private func online(_ epoch: String = "epoch-1") throws -> CompanionStore {
        let store = CompanionStore()
        try store.pairWithMac(hostName: "Mac", staticPubB64: ServerHandshakeResponder().publicBase64Url,
                              initialEpoch: epoch, mode: .directLAN)
        return store
    }

    @MainActor
    func testACommandKeepsItsIdWhenSentAgainToTheSameHostStart() throws {
        let store = try online()
        _ = try store.createTask(projectId: "p1", agent: "claude", prompt: "修个 bug")
        XCTAssertEqual(store.outbox.count, 1)
        let sent = store.outbox[0]
        XCTAssertEqual(sent.method, "task.create")
        XCTAssertEqual(sent.hostEpoch, "epoch-1")

        // On the wire: the params, the id and the host start.
        let wire = try RemoteRpcCodec.encodeCommand(version: .v3, id: "7", command: sent)
        let frame = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(wire.utf8)) as? [String: Any])
        let params = try XCTUnwrap(frame["params"] as? [String: Any])
        XCTAssertEqual(params["commandId"] as? String, sent.commandId)
        XCTAssertEqual(params["hostEpoch"] as? String, "epoch-1")
        XCTAssertEqual(params["prompt"] as? String, "修个 bug")

        // The connection dropped before the answer; back on the same start,
        // it goes again three times with the same id.
        store.markMacOffline(relayStillConnected: false)
        XCTAssertThrowsError(try store.commandsToResend(), "nothing is sent while offline")
        store.handleReconnect(newEpoch: "epoch-1", latestHostSeq: 0, mode: .directLAN,
                              snapshotTasks: store.tasks, snapshotApprovals: [])
        for _ in 0..<3 {
            XCTAssertEqual(try store.commandsToResend().map(\.commandId), [sent.commandId])
        }
        store.applyCommandReply(commandId: sent.commandId)
        XCTAssertTrue(store.outbox.isEmpty)
        XCTAssertFalse(store.resyncNeeded)
    }

    /// The Mac restarted: unanswered commands are not resent; a refusal for
    /// an earlier start asks for a resync, and the banner says so.
    @MainActor
    func testARestartedMacGetsNoOldCommands() throws {
        let store = try online()
        store.handleReconnect(newEpoch: "epoch-1", latestHostSeq: 0, mode: .directLAN, snapshotTasks: [],
                              snapshotApprovals: [CompanionApproval(requestId: "r1", taskId: "t1", summary: "删文件",
                                                                    hostEpoch: "epoch-1")])
        try store.submitApproval(requestId: "r1", approve: true)
        let approval = try XCTUnwrap(store.outbox.first)
        XCTAssertEqual(approval.method, "task.respond")
        XCTAssertEqual(approval.params["action"], "approve")

        store.handleReconnect(newEpoch: "epoch-2", latestHostSeq: 0, mode: .directLAN,
                              snapshotTasks: [], snapshotApprovals: [])
        XCTAssertTrue(store.outbox.isEmpty, "decided on the last start: not sent again")
        XCTAssertEqual(store.droppedCommands.map(\.commandId), [approval.commandId])
        XCTAssertTrue(try store.commandsToResend().isEmpty)

        _ = try store.createTask(projectId: "p1", agent: "codex", prompt: "x")
        let created = store.outbox[0]
        XCTAssertEqual(created.hostEpoch, "epoch-2")
        store.applyCommandReply(commandId: created.commandId,
                                failure: "\(staleHostEpochPrefix): 宿主已经重启")
        XCTAssertTrue(store.resyncNeeded)
        let banner = CompanionStatusBanner(state: store.connectionState, resyncing: store.resyncNeeded)
        XCTAssertEqual(banner.statusText, "正在和 Mac 重新同步，之前显示的状态可能已过期")
        XCTAssertEqual(CompanionStatusBanner(state: store.connectionState).statusText.hasPrefix("Mac 在线"), true)
    }

    /// Events: a repeat is ignored, a gap or an overflow means resync.
    @MainActor
    func testAnEventGapOrOverflowMeansResync() throws {
        let store = try online()
        store.ingestHostEvent(seq: 1)
        store.ingestHostEvent(seq: 2)
        store.ingestHostEvent(seq: 2)
        XCTAssertFalse(store.resyncNeeded)
        XCTAssertEqual(store.lastEventSeq, 2)
        store.ingestHostEvent(seq: 5)
        XCTAssertTrue(store.resyncNeeded, "3 and 4 were missed")
        store.markResynced()
        store.ingestEventsReset()
        XCTAssertTrue(store.resyncNeeded)

        store.handleDeviceRevoked()
        XCTAssertTrue(store.outbox.isEmpty)
        XCTAssertTrue(store.droppedCommands.isEmpty)
    }
}
