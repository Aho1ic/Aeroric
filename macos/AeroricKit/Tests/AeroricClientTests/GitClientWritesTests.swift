import XCTest
import AeroricProtocol
@testable import AeroricClient

final class GitClientWritesTests: XCTestCase {
    func testStashEntriesDecode() throws {
        let value = JSONValue.array([
            .object(["index": .number("0"), "name": .string("stash@{0}"), "commit": .string("abc"),
                     "date": .string("2026-10-30"), "message": .string("WIP on main: 修按钮")]),
        ])
        let entries = try GitClient.decodeStash(value)
        XCTAssertEqual(entries, [.init(index: 0, name: "stash@{0}", message: "WIP on main: 修按钮")])
    }

    func testRemoteCountsDecodeAndAMissingUpstreamIsNil() {
        let counts = GitClient.decodeCounts(.object(["ahead": .number("2"), "behind": .number("0"), "branch": .string("main")]))
        XCTAssertEqual(counts, .init(ahead: 2, behind: 0))
        XCTAssertNil(GitClient.decodeCounts(.null))
    }
}
