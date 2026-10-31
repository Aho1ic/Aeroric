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
        XCTAssertEqual(batch.updates.count, 27)
        XCTAssertEqual(batch.latestSeq, 27)
        XCTAssertNil(batch.gapOldestAvailable)
        XCTAssertEqual(batch.updates.map(\.seq), Array(1...27))
        let handoff = batch.updates.compactMap { update -> (String, LiveCheckpoint)? in
            if case let .handoff(reason, checkpoint) = update.kind { return (reason, checkpoint) } else { return nil }
        }.first
        XCTAssertEqual(handoff?.0, "sourceMove")
        XCTAssertEqual(handoff?.1.goal?.text, "修复搜索客户端超时")
        XCTAssertEqual(handoff?.1.unknown.map(\.summary), ["cargo test search_timeout"])
        XCTAssertEqual(handoff?.1.needsAttention, true)
        XCTAssertTrue(batch.updates.map(\.kind).contains(.clarification(LiveClarification(
            turnId: "u0", requirements: ["超时可以配置"],
            acceptance: [LiveAcceptance(text: "cargo test search_timeout 通过", check: "cargo test search_timeout")]))))
        XCTAssertTrue(batch.updates.map(\.kind).contains(.branchReturned(LiveBranchResult(
            taskId: "task-2", title: "查 reqwest 超时语义", summary: "reqwest 的 timeout 覆盖整个请求，连接超时另设",
            returnedMs: 1_761_350_400_500))))
        XCTAssertTrue(batch.updates.map(\.kind).contains(.review(LiveReviewCard(
            reviewId: "rv-1", round: 1, state: "waitingCoder", turnId: "u1", reviewerTaskId: "task-3",
            reviewerLabel: "Claude Code · sonnet",
            findings: [LiveReviewFinding(id: "r1", severity: "blocker", text: "重试没有上限，可能死循环",
                                         file: "src/client.rs", line: 88, decision: "adopt")],
            summary: "超时逻辑可行，重试上限缺测试"))))
        let delegation = batch.updates.compactMap { update -> LiveDelegationCard? in
            if case let .delegation(card) = update.kind { return card }
            return nil
        }.first
        XCTAssertEqual(delegation?.delegationId, "dg-1")
        XCTAssertEqual(delegation?.origin, "session")
        XCTAssertEqual(delegation?.contract.agent, .other)
        XCTAssertEqual(delegation?.contract.contextMode, "isolated")
        XCTAssertEqual(delegation?.contract.contextRefs, ["src/client.rs"])
        XCTAssertEqual(delegation?.contract.budgetMinutes, 30)
        XCTAssertEqual(delegation?.acceptance, [LiveDelegationAcceptance(id: "a1", text: "给出可运行的示例", state: "open")])
        XCTAssertEqual(delegation?.tokens, 1200)
        XCTAssertEqual(delegation?.sent, true)
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
        XCTAssertTrue(kinds.contains(.subagent(id: "s1", title: "检索", status: "running", detail: "读取 src/main.rs")))
        XCTAssertTrue(kinds.contains(.autoApproved(id: "8", toolCallId: "c2", kind: "bash", summary: "npm run lint", subject: "bash")))
        XCTAssertTrue(kinds.contains(.question(LivePendingQuestion(id: "q1", style: "select", title: "用哪个测试框架？",
                                                                  options: ["vitest", "jest"], timeoutMs: 30_000))))
        XCTAssertTrue(kinds.contains(.questionResolved(id: "q1", outcome: "answered")))
        XCTAssertTrue(kinds.contains(.notice(level: "info", message: "上下文已自动压缩")))
        XCTAssertTrue(kinds.contains(.turnChanges(turnId: "u1", files: [
            LiveFileChange(path: "src/calc.py", additions: 3, deletions: 1),
            LiveFileChange(path: "docs/diagram.png", additions: nil, deletions: nil),
        ], additions: 3, deletions: 1, truncated: false)))
        XCTAssertTrue(kinds.contains(.autoCheck(turnId: "u1", state: "failed", items: [
            LiveAutoCheckItem(kind: "tests", profile: "python", label: "Pytest", state: "failed",
                              summary: "2 项中 1 项失败", failures: ["test_add: assert 3 == 4"],
                              outputTail: "1 failed, 1 passed"),
        ])))
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
        XCTAssertNil(state.resumeBlocked)
        var blocked = state
        blocked.resumeBlocked = "x"
        var fields = fixture["state"]?.objectValue ?? [:]
        fields["resumeBlocked"] = .string("x")
        XCTAssertEqual(try LiveSessionState.decode(.object(fields)), blocked)
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
        XCTAssertEqual(LiveSessionRequest.answer(taskId: "task-1", questionId: "q1", value: "vitest"),
                       requests[LiveSessionContract.answerMethod])
        XCTAssertEqual(LiveSessionRequest.revokeGrant(taskId: "task-1", subject: "bash"),
                       requests[LiveSessionContract.revokeGrantMethod])
        XCTAssertEqual(requests.objectValue?.count, 10)
    }
}
