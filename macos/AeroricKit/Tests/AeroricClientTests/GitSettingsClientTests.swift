import XCTest

@testable import AeroricClient
@testable import AeroricProtocol

/// 2026-11-24 设置 › Git 与工作副本: reading `gitSettings.get` and `copies.list`.
final class GitSettingsClientTests: XCTestCase {
    func testSettingsReadWithDefaultsForWhatIsMissing() {
        XCTAssertEqual(GitSettings.decode(.object([:])), GitSettings(commitGuidance: "", clearedCopyKeepDays: 30))
        let read = GitSettings.decode(.object([
            "commitGuidance": .string("用中文写"), "clearedCopyKeepDays": .number("0"),
        ]))
        XCTAssertEqual(read, GitSettings(commitGuidance: "用中文写", clearedCopyKeepDays: 0))
        XCTAssertEqual(GitSettings.decode(read.json), read, "what is sent reads back the same")
    }

    func testCopiesKeepOnlyRowsWithAnIdAndAPath() {
        let copies = IsolatedCopy.decodeList(.object(["copies": .array([
            .object(["taskId": .string("t1"), "title": .string("Fix"), "path": .string("/p/.aeroric/worktrees/t1"),
                     "origin": .string("/p"), "createdMs": .number("1795500000000"), "clearedMs": .null]),
            .object(["taskId": .string("t2"), "path": .string("/p/.aeroric/worktrees/t2"),
                     "clearedMs": .number("1795500000001")]),
            .object(["path": .string("/no-id")]),
        ])]))
        XCTAssertEqual(copies.map(\.taskID), ["t1", "t2"])
        XCTAssertNil(copies[0].clearedMs, "null: still has work in it")
        XCTAssertEqual(copies[1].clearedMs, 1_795_500_000_001)
        XCTAssertNil(copies[1].title)
    }
}
