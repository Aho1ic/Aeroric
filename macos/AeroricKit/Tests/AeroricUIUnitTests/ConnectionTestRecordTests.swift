import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// C5: a connection test is kept with the exit it took; once the exit
/// changes the model row says 需要重测 and why.
final class ConnectionTestRecordTests: XCTestCase {
    func testKeptTestsDecodeAndSayWhenTheyNoLongerHold() throws {
        let snapshot = try AgentConnectionsClient.decodeSnapshot(.object([
            "document": .object(["version": .number("1"), "connections": .array([]), "pools": .array([]),
                                 "configs": .array([])]),
            "credentials": .object(["c1": .string("stored")]),
            "tests": .array([
                .object([
                    "connectionId": .string("c1"), "model": .string("m1"), "atMs": .number("5"),
                    "status": .number("200"), "elapsedMs": .number("1200"), "excerpt": .string("pong"),
                    "passed": .bool(true),
                    "egress": .object(["baseUrl": .string("https://api.example/v1"),
                                       "protocol": .string("chat_completions"),
                                       "proxy": .string("http://127.0.0.1:7890")]),
                ]),
                .object([
                    "connectionId": .string("c1"), "model": .string("m2"), "status": .number("0"),
                    "excerpt": .string("Global proxy is not configured"), "passed": .bool(false),
                    "egress": .object(["baseUrl": .string("https://api.example/v1"),
                                       "protocol": .string("chat_completions")]),
                ]),
                .object([
                    "connectionId": .string("c1"), "model": .string("m3"), "status": .number("200"),
                    "passed": .bool(true), "stale": .string("配置已变，需要重测（凭据换过）"),
                    "egress": .object([:]),
                ]),
            ]),
        ]))
        XCTAssertEqual(snapshot.tests.count, 3)
        let passed = try XCTUnwrap(snapshot.test(connectionId: "c1", model: "m1"))
        XCTAssertEqual(AgentConfigText.lastTest(passed).text, "上次通过")
        let help = AgentConfigText.lastTestHelp(passed)
        XCTAssertTrue(help.contains("HTTP 200 · 1.2 秒"), help)
        XCTAssertTrue(help.contains("经代理 http://127.0.0.1:7890"), help)
        XCTAssertTrue(help.contains("https://api.example/v1"), help)

        let failed = try XCTUnwrap(snapshot.test(connectionId: "c1", model: "m2"))
        XCTAssertEqual(AgentConfigText.lastTest(failed).text, "上次失败")
        let failedHelp = AgentConfigText.lastTestHelp(failed)
        XCTAssertTrue(failedHelp.contains("没有回应") && failedHelp.contains("直连"), failedHelp)
        XCTAssertTrue(failedHelp.contains("Global proxy is not configured"), failedHelp)

        let stale = try XCTUnwrap(snapshot.test(connectionId: "c1", model: "m3"))
        XCTAssertEqual(AgentConfigText.lastTest(stale).text, "需要重测")
        XCTAssertTrue(AgentConfigText.lastTestHelp(stale).hasPrefix("配置已变，需要重测（凭据换过）"))
        XCTAssertNil(snapshot.test(connectionId: "c2", model: "m1"))
    }
}
