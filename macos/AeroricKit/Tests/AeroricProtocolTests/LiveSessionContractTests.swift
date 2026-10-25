import XCTest

@testable import AeroricProtocol

/// Reads the committed `contracts/native/v1/fixtures/session-live.json` that the
/// Rust DTOs generate, so both sides agree on the same bytes.
final class LiveSessionContractTests: XCTestCase {
    private static let repoRoot: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url
    }()

    private func fixture() throws -> JSONValue {
        let url = Self.repoRoot.appendingPathComponent("contracts/native/v1/fixtures/session-live.json")
        return JSONValue.from(try JSONSerialization.jsonObject(with: Data(contentsOf: url)))
    }

    func testEveryUpdateKindInTheFixtureDecodesAsKnown() throws {
        let batch = try LiveUpdatesBatch.decode(try fixture()["updates"] ?? .null)
        XCTAssertEqual(batch.updates.count, 16)
        XCTAssertEqual(batch.latestSeq, 16)
        XCTAssertNil(batch.gapOldestAvailable)
        XCTAssertEqual(batch.updates.map(\.seq), Array(1...16))
        XCTAssertEqual(batch.updates[0], LiveSessionUpdate(taskId: "task-1", seq: 1, atMs: 1_761_350_400_000,
                                                           kind: .turnStarted(turnId: "u1")))
        let kinds = batch.updates.map(\.kind)
        XCTAssertTrue(kinds.contains(.thoughtChunk(id: "r1", text: "先看目录", summary: true)))
        XCTAssertTrue(kinds.contains(.permissionRequest(id: "7", toolCallId: "c1", kind: "command", summary: "ls -la",
                                                        options: ["allowOnce", "allowForSession", "deny"])))
        XCTAssertTrue(kinds.contains(.toolCallUpdate(id: "c1", status: .completed, outputDelta: nil,
                                                     result: .object(["exitCode": .number("0")]))))
        XCTAssertTrue(kinds.contains(.plan([LivePlanEntry(text: "读代码", status: "completed"),
                                            LivePlanEntry(text: "改代码", status: "inProgress")])))
        XCTAssertTrue(kinds.contains(.usage(contextUsed: 1200, contextLimit: 200_000, inputTokens: 1500, outputTokens: 300)))
        XCTAssertTrue(kinds.contains(.unknown(kind: "item/futureThing", payload: .object(["x": .number("1")]))))
        XCTAssertEqual(kinds.last, .turnEnded(turnId: "u1", reason: .completed, error: nil))
        // Only the fixture's deliberate `unknown` row is unknown: every other kind is understood.
        let unknown = kinds.filter { if case .unknown = $0 { true } else { false } }
        XCTAssertEqual(unknown.count, 1)
    }

    func testGapAndStateDecode() throws {
        let fixture = try fixture()
        let gap = try LiveUpdatesBatch.decode(fixture["gap"] ?? .null)
        XCTAssertEqual(gap.updates, [])
        XCTAssertEqual(gap.latestSeq, 4200)
        XCTAssertEqual(gap.gapOldestAvailable, 2201)
        let state = try LiveSessionState.decode(fixture["state"] ?? .null)
        XCTAssertTrue(state.live)
        XCTAssertTrue(state.structured)
        XCTAssertEqual(state.family, "codex")
        XCTAssertEqual(state.sessionId, "th-1")
        XCTAssertEqual(state.pending, [LivePendingPermission(id: "7", kind: "command", summary: "ls -la", toolCallId: "c1",
                                                             options: ["allowOnce", "allowForSession", "deny"])])
        XCTAssertEqual(state.latestSeq, 16)
        XCTAssertEqual(state.contextLimit, 200_000)
    }

    func testAnUnknownOrMalformedUpdateIsKeptNotFatal() throws {
        let newer: JSONValue = .object([
            "taskId": .string("t"), "seq": .number("9"), "atMs": .number("1"), "type": .string("someNewerKind"), "a": .bool(true),
        ])
        let update = try LiveSessionUpdate.decode(newer)
        guard case let .unknown(kind, payload) = update.kind else { return XCTFail("kept as unknown") }
        XCTAssertEqual(kind, "someNewerKind")
        XCTAssertEqual(payload["a"], .bool(true))
        let malformed: JSONValue = .object([
            "taskId": .string("t"), "seq": .number("10"), "atMs": .number("1"), "type": .string("turnEnded"),
            "turnId": .string("u"), "reason": .string("exploded"),
        ])
        guard case .unknown(kind: "turnEnded", _) = try LiveSessionUpdate.decode(malformed).kind else {
            return XCTFail("a known kind it cannot read is kept as unknown")
        }
        XCTAssertThrowsError(try LiveSessionUpdate.decode(.object(["type": .string("turnStarted")])))
    }

    func testRequestParamsMatchTheRustFixture() throws {
        let requests = try fixture()["requests"] ?? .null
        XCTAssertEqual(LiveSessionRequest.prompt(taskId: "task-1", text: "继续"), requests[LiveSessionContract.promptMethod])
        XCTAssertEqual(LiveSessionRequest.interrupt(taskId: "task-1"), requests[LiveSessionContract.interruptMethod])
        XCTAssertEqual(LiveSessionRequest.respond(taskId: "task-1", requestId: "7", decision: .deny, reason: "别删"),
                       requests[LiveSessionContract.respondMethod])
        XCTAssertEqual(LiveSessionRequest.configure(taskId: "task-1", model: "gpt-5", permission: .fullAccess, effort: "high"),
                       requests[LiveSessionContract.configureMethod])
        XCTAssertEqual(LiveSessionRequest.command(taskId: "task-1", command: .compact), requests[LiveSessionContract.commandMethod])
        XCTAssertEqual(LiveSessionRequest.close(taskId: "task-1"), requests[LiveSessionContract.closeMethod])
        XCTAssertEqual(LiveSessionRequest.state(taskId: "task-1"), requests[LiveSessionContract.stateMethod])
        XCTAssertEqual(LiveSessionRequest.updates(taskId: "task-1", after: 12, limit: 500), requests[LiveSessionContract.updatesMethod])
        XCTAssertEqual(requests.objectValue?.count, 8)
    }
}
