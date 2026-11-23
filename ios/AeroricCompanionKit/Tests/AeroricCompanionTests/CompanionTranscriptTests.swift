@testable import AeroricMobileTransport
import Foundation
import XCTest

/// D4 (2026-11-23): a task's page reads the latest messages of its session
/// from the Mac, and reads them again when the Mac says the task changed.
final class CompanionTranscriptTests: XCTestCase {
    private let mac = ServerHandshakeResponder()

    private func json(_ object: [String: Any]) -> String {
        String(decoding: try! JSONSerialization.data(withJSONObject: object), as: UTF8.self)
    }

    @MainActor
    private func eventually(_ what: String, _ condition: @MainActor () -> Bool) async {
        for _ in 0..<300 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }

    private func answer(_ texts: [String], total: Int? = nil) -> [String: Any] {
        [
            "available": true,
            "messages": texts.enumerated().map { index, text in
                ["role": index.isMultiple(of: 2) ? "user" : "assistant", "messageId": "m\(index)",
                 "content": [["type": "text", "text": text]]]
            },
            "total": total ?? texts.count,
            "omitted": (total ?? texts.count) - texts.count,
        ]
    }

    // MARK: - Reading the Mac's answer

    func testTheMacsAnswerBecomesMessagesWithTheirLines() {
        let state = CompanionTranscriptState.reading(taskId: "t1", answer: json([
            "available": true, "total": 52, "omitted": 50,
            "messages": [
                ["role": "user", "content": [["type": "text", "text": "修一下"]]],
                ["role": "assistant", "messageId": "a1", "content": [
                    ["type": "thinking", "thinking": "想想"],
                    ["type": "tool_use", "id": "c1", "name": "Bash", "input": "ls", "truncated": true],
                    ["type": "tool_result", "id": "c1", "output": "denied", "failed": true],
                    ["type": "tool_result", "id": "c2", "output": "ok"],
                    ["type": "attachment", "name": "a.png", "mediaType": "image/png", "truncated": true],
                    ["type": "hologram"],
                    ["type": "text", "text": "好了"],
                ]],
            ],
        ]))
        guard case let .shown(transcript, refreshing) = state else { return XCTFail("\(state)") }
        XCTAssertFalse(refreshing)
        XCTAssertEqual(transcript.total, 52)
        XCTAssertEqual(transcript.omitted, 50)
        XCTAssertEqual(transcript.messages.map(\.id), ["#0", "a1"])
        XCTAssertEqual(transcript.messages[1].lines, [
            CompanionMessageLine(kind: .thinking, text: "想想"),
            CompanionMessageLine(kind: .tool, text: "Bash ls", truncated: true),
            CompanionMessageLine(kind: .failedToolResult, text: "denied"),
            CompanionMessageLine(kind: .toolResult, text: "ok"),
            CompanionMessageLine(kind: .attachment, text: "a.png", truncated: true),
            CompanionMessageLine(kind: .text, text: "好了"),
        ], "a block the phone does not know is left out")
    }

    func testNothingToShowSaysWhy() {
        let reason = { (object: [String: Any]) -> String? in
            if case let .unavailable(_, reason) = CompanionTranscriptState.reading(taskId: "t1", answer: self.json(object)) {
                return reason
            }
            return nil
        }
        XCTAssertEqual(reason(["available": false, "reason": "no_session", "messages": []]), "这个任务还没有会话记录")
        XCTAssertEqual(reason(["available": false, "reason": "unreadable", "detail": "line 3"]), "会话记录读不出来：line 3")
        XCTAssertEqual(reason(["available": false, "reason": "ssh"]), "这个项目在远程主机（SSH）上，手机还看不了它的会话")
        XCTAssertEqual(CompanionTranscriptState.reading(taskId: "t1", answer: "not json"),
                       .failed(taskId: "t1", message: "Mac 的回答读不懂"))
    }

    // MARK: - The store

    @MainActor
    func testTheStoreAsksForTheOpenTaskAndIgnoresALateAnswer() {
        let store = CompanionStore()
        var asked = 0
        store.onTranscriptWanted = { asked += 1 }
        store.openTask("t1")
        XCTAssertEqual(store.transcript, .loading(taskId: "t1"))
        XCTAssertEqual(asked, 1)

        store.applyTranscript(taskId: "t2", answer: json(answer(["x"])))
        XCTAssertEqual(store.transcript, .loading(taskId: "t1"), "an answer for a task no longer open")
        store.applyTranscript(taskId: "t1", answer: json(answer(["hi"])))
        guard case let .shown(transcript, false) = store.transcript else { return XCTFail() }
        XCTAssertEqual(transcript.messages.first?.lines.first?.text, "hi")

        store.transcriptFailed(taskId: "t1", message: "断了")
        guard case .shown = store.transcript else { return XCTFail("a shown session stays when a read fails") }

        store.closeTask("t2")
        XCTAssertEqual(store.transcript.taskId, "t1", "leaving another task changes nothing")
        store.closeTask("t1")
        XCTAssertEqual(store.transcript, .closed)
        store.transcriptFailed(taskId: "t1", message: "late")
        XCTAssertEqual(store.transcript, .closed)
    }

    @MainActor
    func testASnapshotRereadsTheOpenTaskOrSaysItIsGone() {
        let store = CompanionStore()
        var asked = 0
        store.onTranscriptWanted = { asked += 1 }
        let task = CompanionTask(id: "t1", projectId: "p1", title: "x", agent: "codex", status: "running",
                                 permissionMode: "ask")
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 0, mode: .directLAN, snapshotTasks: [task], snapshotApprovals: [])
        XCTAssertEqual(asked, 0, "nothing open, nothing read")

        store.openTask("t1")
        store.applyTranscript(taskId: "t1", answer: json(answer(["hi"])))
        store.handleReconnect(newEpoch: "e1", latestHostSeq: 1, mode: .directLAN, snapshotTasks: [task], snapshotApprovals: [])
        XCTAssertEqual(asked, 2)
        guard case .shown(_, true) = store.transcript else { return XCTFail("\(store.transcript)") }

        store.handleReconnect(newEpoch: "e1", latestHostSeq: 2, mode: .directLAN, snapshotTasks: [], snapshotApprovals: [])
        XCTAssertEqual(store.transcript, .unavailable(taskId: "t1", reason: "这个任务在 Mac 上已经没有了"))
        XCTAssertEqual(asked, 2)

        store.unpair()
        XCTAssertEqual(store.transcript, .closed)
    }

    // MARK: - Over the connection

    @MainActor
    func testTheOpenTaskIsReadAndReadAgainWhenTheMacSaysItChanged() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: CompanionCredentials(
            hostName: "Studio Mac", hostId: "host-1", publicKey: mac.publicBase64Url,
            endpoints: ["ws://192.168.1.33:8765"], deviceId: "dev-1", deviceToken: "token-1"))
        var config = FakeMac.Config()
        config.tasks = ["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex", "status": "running"]]]
        config.transcripts = ["t1": answer(["修一下"], total: 41)]
        let fake = FakeMac(responder: mac, config: config)
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { _ in fake }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }

        store.openTask("t1")
        await eventually("shown") { if case .shown(_, false) = store.transcript { true } else { false } }
        guard case let .shown(first, _) = store.transcript else { return XCTFail() }
        XCTAssertEqual(first.messages.map { $0.lines.first?.text }, ["修一下"])
        XCTAssertEqual(first.omitted, 40)
        XCTAssertEqual(fake.receivedTranscriptReads.first?.taskId, "t1")
        XCTAssertEqual(fake.receivedTranscriptReads.first?.limit, CompanionConnector.transcriptLimit)

        fake.setTranscript("t1", answer(["修一下", "改好了"]))
        fake.push(seq: 1, event: "task-changed")
        await eventually("read again") {
            if case let .shown(transcript, false) = store.transcript { transcript.messages.count == 2 } else { false }
        }

        store.closeTask("t1")
        let reads = fake.receivedTranscriptReads.count
        fake.push(seq: 2, event: "task-changed")
        await eventually("resynced") { store.lastEventSeq >= 2 && !store.resyncNeeded }
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(fake.receivedTranscriptReads.count, reads, "a closed page is not read")
        await connector.stop()
    }
}
