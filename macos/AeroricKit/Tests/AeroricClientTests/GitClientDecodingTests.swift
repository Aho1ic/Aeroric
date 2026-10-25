import AeroricProtocol
import XCTest
@testable import AeroricClient

/// The read-only Git replies, shaped as the host sends them.
final class GitClientDecodingTests: XCTestCase {
    func testStatusBranchesAndLogDecode() throws {
        let status = try GitClient.decodeStatus(.array([
            .object(["path": .string("src/calc.py"), "status": .string("M"), "staged": .bool(false)]),
        ]))
        XCTAssertEqual(status, [GitClient.FileStatus(path: "src/calc.py", status: "M", staged: false)])
        let branches = try GitClient.decodeBranches(.array([
            .object(["name": .string("master"), "current": .bool(true), "remote": .null]),
        ]))
        XCTAssertEqual(branches, [GitClient.Branch(name: "master", current: true, remote: nil)])
        let log = try GitClient.decodeLog(.array([
            .object(["hash": .string("966bb0435d"), "short_hash": .string("966bb04"), "author": .string("smoke"),
                     "date": .string("2026-10-25"), "message": .string("smoke: initial"), "refs": .array([])]),
        ]))
        XCTAssertEqual(log.first?.shortHash, "966bb04")
        XCTAssertEqual(log.first?.message, "smoke: initial")
    }

    func testAMalformedStatusRowIsRejected() {
        XCTAssertThrowsError(try GitClient.decodeStatus(.array([.object(["path": .string("x")])])))
    }
}
