import XCTest

@testable import AeroricClient
@testable import AeroricProtocol

/// 2026-11-24 设置 › 存储: reading `storage.usage`.
final class StorageClientTests: XCTestCase {
    func testTheHostsAnswerIsReadByKind() throws {
        let answer = JSONValue.object([
            "dataDir": .string("/Users/me/Library/Application Support/com.aeroric.native"),
            "totalBytes": .number("1500000000"),
            "unreadable": .number("2"),
            "groups": .array([
                .object([
                    "id": .string("sessions"), "title": .string("会话与 Agent"), "bytes": .number("1200000000"),
                    "entries": .array([
                        .object(["path": .string("/d/agent-homes"), "bytes": .number("1100000000")]),
                        .object(["bytes": .number("5")]),
                    ]),
                ]),
                .object(["title": .string("没有 id")]),
                .object(["id": .string("logs"), "bytes": .number("300000000")]),
            ]),
        ])
        let usage = try XCTUnwrap(StorageUsage.decode(answer))
        XCTAssertEqual(usage.totalBytes, 1_500_000_000, "past Int32")
        XCTAssertEqual(usage.unreadable, 2)
        XCTAssertEqual(usage.groups.map(\.id), ["sessions", "logs"], "a group without an id is dropped")
        XCTAssertEqual(usage.groups[0].entries.map(\.path), ["/d/agent-homes"], "an entry without a path is dropped")
        XCTAssertEqual(usage.groups[1].title, "logs", "a missing title reads as the id")
        XCTAssertNil(StorageUsage.decode(.object(["groups": .array([])])), "no dataDir: not an answer")
    }
}
