import XCTest
import AeroricProtocol
@testable import AeroricClient

final class ImportsClientDecodingTests: XCTestCase {
    func testStatusDecodesPerAgentRows() {
        let status = ImportsClient.decodeStatus(.object([
            "agents": .array([
                .object(["agent": .string("claude"), "granted": .bool(true), "root": .string("/h/.claude"),
                         "grantedAtMs": .number("5"), "sessions": .number("3")]),
                .object(["granted": .bool(true)]),
            ]),
            "lastScanAtMs": .number("10"), "lastError": .null,
        ]))
        XCTAssertEqual(status.agents.count, 1, "rows without an agent are dropped")
        XCTAssertEqual(status.agents[0].sessions, 3)
        XCTAssertEqual(status.agents[0].grantedAtMs, 5)
        XCTAssertEqual(status.lastScanAtMs, 10)
        XCTAssertNil(status.lastError)
    }

    func testEstimateSessionsAndDetection() {
        let estimate = ImportsClient.decodeEstimate(.object([
            "agent": .string("codex"), "root": .string("/h/.codex"),
            "estimate": .object(["rootExists": .bool(true), "sessions": .number("2"), "bytes": .number("2048")]),
        ]))
        XCTAssertEqual(estimate?.sessions, 2)
        XCTAssertEqual(estimate?.bytes, 2048)
        XCTAssertNil(ImportsClient.decodeEstimate(.object([:])))

        let sessions = ImportsClient.decodeSessions(.object(["sessions": .array([
            .object(["id": .string("imp_1"), "agent": .string("claude"), "title": .string("修复"),
                     "lastAtMs": .number("7"), "projectId": .string("p1"), "cwd": .string("/w")]),
            .object(["id": .string("imp_2"), "agent": .string("codex"), "tooLarge": .bool(true)]),
        ])]))
        XCTAssertEqual(sessions.map(\.id), ["imp_1", "imp_2"])
        XCTAssertEqual(sessions[1].title, "imp_2", "a missing title falls back to the id")
        XCTAssertTrue(sessions[1].tooLarge)
        XCTAssertNil(sessions[1].projectId)

        let detected = ImportsClient.decodeDetected(.object(["agents": .array([
            .object(["agent": .string("claude"), "installed": .bool(true), "path": .string("/bin/claude")]),
            .object(["agent": .string("omp"), "installed": .bool(false)]),
        ])]))
        XCTAssertEqual(detected.filter(\.installed).map(\.agent), ["claude"])
    }
}
