@testable import AeroricMobileTransport
import Foundation
import XCTest

/// D4 (2026-11-23): a task page sends a message to its task and stops its
/// current turn, as commands (`commandId`, `hostEpoch`), and says what became
/// of each.
final class CompanionTaskActionTests: XCTestCase {
    private let mac = ServerHandshakeResponder()

    @MainActor
    private func eventually(_ what: String, _ condition: @MainActor () -> Bool) async {
        for _ in 0..<300 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }

    private let task = CompanionTask(id: "t1", projectId: "p1", title: "x", agent: "codex", status: "running")

    @MainActor
    func testAMessageAndAStopAreCommandsThatSayHowTheyEnded() throws {
        let store = CompanionStore()
        XCTAssertThrowsError(try store.sendMessage(taskId: "t1", text: "hi")) { error in
            guard case .macOfflineNoQueueing = error as? CompanionActionError else { return XCTFail("\(error)") }
        }
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 0, mode: .directLAN, snapshotTasks: [task], snapshotApprovals: [])
        XCTAssertThrowsError(try store.sendMessage(taskId: "t1", text: "  \n")) {
            XCTAssertEqual($0 as? CompanionActionError, .emptyMessage)
        }
        XCTAssertThrowsError(try store.stopTask(taskId: "t9")) {
            XCTAssertEqual($0 as? CompanionActionError, .unknownTask("t9"))
        }
        XCTAssertTrue(store.outbox.isEmpty)

        try store.sendMessage(taskId: "t1", text: " 继续 \n")
        let message = try XCTUnwrap(store.outbox.last)
        XCTAssertEqual(message.method, "task.input")
        XCTAssertEqual(message.params, ["projectId": "p1", "taskId": "t1", "text": "继续"])
        XCTAssertEqual(store.taskActions["t1"]?.note, "正在发给 Mac…")
        store.applyCommandReply(commandId: message.commandId)
        XCTAssertEqual(store.taskActions["t1"]?.state, .done)
        XCTAssertEqual(store.taskActions["t1"]?.note, "Mac 收下了这条消息")

        try store.stopTask(taskId: "t1")
        let stop = try XCTUnwrap(store.outbox.last)
        XCTAssertEqual(stop.method, "task.interrupt")
        XCTAssertEqual(store.taskActions["t1"]?.kind, .stop)
        store.applyCommandReply(commandId: stop.commandId, failure: "暂停所有 Agent 已打开")
        XCTAssertEqual(store.taskActions["t1"]?.note, "没能停止：暂停所有 Agent 已打开")

        try store.sendMessage(taskId: "t1", text: "再来")
        store.handleReconnect(newEpoch: "e2", latestHostSeq: 0, mode: .directLAN, snapshotTasks: [task], snapshotApprovals: [])
        XCTAssertEqual(store.taskActions["t1"]?.state, .failed("Mac 重启了，没有送出；看过最新状态再发"))

        store.unpair()
        XCTAssertTrue(store.taskActions.isEmpty)
    }

    @MainActor
    func testTheMacTakesTheMessageAndSaysWhyItCouldNotStop() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: CompanionCredentials(
            hostName: "Studio Mac", hostId: "host-1", publicKey: mac.publicBase64Url,
            endpoints: ["ws://192.168.1.33:8765"], deviceId: "dev-1", deviceToken: "token-1"))
        var config = FakeMac.Config()
        config.tasks = ["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex", "status": "running"]]]
        config.refuseInterrupt = "this task has no open native session"
        let fake = FakeMac(responder: mac, config: config)
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { _ in fake }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline && !store.tasks.isEmpty }

        try store.sendMessage(taskId: "t1", text: "改完跑一下测试")
        await eventually("taken") { store.taskActions["t1"]?.state == .done }
        XCTAssertEqual(fake.receivedCommands.map(\.method), ["task.input"])
        XCTAssertEqual(fake.receivedParams.last?["text"], "改完跑一下测试")
        XCTAssertNotNil(fake.receivedParams.last?["commandId"])

        try store.stopTask(taskId: "t1")
        await eventually("refused") { store.taskActions["t1"]?.state.isFailure == true }
        XCTAssertEqual(store.taskActions["t1"]?.note, "没能停止：this task has no open native session")
        XCTAssertTrue(store.outbox.isEmpty)
        await connector.stop()
    }
}
