import AeroricProtocol
@testable import AeroricClient
import XCTest

/// Values only. No UDS/host, project/agent files, processes or windows.
final class ProjectResourcesClientTests: XCTestCase {
    func testStatusPreservesSamePathWithDifferentSources() throws {
        let changes = try ProjectResourcesClient.decodeChanges(.array([
            .object(["path": .string("a.swift"), "status": .string("M"), "staged": .bool(true)]),
            .object(["path": .string("a.swift"), "status": .string("M"), "staged": .bool(false)]),
        ]))
        XCTAssertEqual(Set(changes.map(\.id)).count, 2)
    }
    func testMalformedRowCannotBecomeSuccessfulEmpty() {
        for result: JSONValue in [.null, .object([:]), .array([.object(["path": .string("x")])])] {
            XCTAssertThrowsError(try ProjectResourcesClient.decodeChanges(result))
        }
        XCTAssertEqual(try ProjectResourcesClient.decodeChanges(.array([])), [])
        XCTAssertThrowsError(try ProjectResourcesClient.decodeBranches(.array([.null])))
    }
    func testDiffDecoderPreservesCompleteText() throws {
        let text = String(repeating: "中", count: 2000)
        XCTAssertEqual(try ProjectResourcesClient.decodeDiff(.string(text)), text)
        let large = String(repeating: "a", count: 204800)
        XCTAssertEqual(try ProjectResourcesClient.decodeDiff(.string(large)), large)
        XCTAssertThrowsError(try ProjectResourcesClient.decodeDiff(.number("0")))
    }
    func testDiffParametersCarryLiteralPathAndSource() throws {
        for path in [":(top)secret", "*.swift", "a[1].swift", "a?.swift", "f.swift"] {
            let params = ProjectResourcesClient.diffParameters(projectPath: "/fixture", path: path, staged: true)
            XCTAssertEqual(params["filePath"]?.stringValue, path)
            XCTAssertEqual(params["staged"]?.boolValue, true)
        }
        let unstaged = ProjectResourcesClient.diffParameters(projectPath: "/fixture", path: "f", staged: false)
        XCTAssertEqual(unstaged["staged"]?.boolValue, false)
    }
    func testMalformedDirectoryCannotSilentlyDropRows() throws {
        XCTAssertThrowsError(try FilesClient.decodeDirectory(.object([
            "entries": .array([.object(["name": .string("missing-bools"), "path": .string("f")])]),
        ])))
        XCTAssertEqual(try FilesClient.decodeDirectory(.object(["entries": .array([])])), [])
    }
}
