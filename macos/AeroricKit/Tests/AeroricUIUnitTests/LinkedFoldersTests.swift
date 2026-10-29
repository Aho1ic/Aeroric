@testable import AeroricUI
import XCTest

/// Linked folders in the project's file tree and on the session page (2026-10-29).
final class LinkedFoldersTests: XCTestCase {
    func testTreePathsMapToTheLinkedFolderOnTheHost() {
        let node = LinkedFolderPath.node(folder: "/Users/me/shared")
        XCTAssertEqual(LinkedFolderPath.request(projectID: "p1", path: node).projectID, "p1#linked:/Users/me/shared")
        XCTAssertEqual(LinkedFolderPath.request(projectID: "p1", path: node).path, "")
        let child = LinkedFolderPath.child(of: node, hostPath: "proto/mail.proto")
        XCTAssertEqual(LinkedFolderPath.request(projectID: "p1", path: child).path, "proto/mail.proto")
        // Project paths pass through untouched.
        XCTAssertEqual(LinkedFolderPath.request(projectID: "p1", path: "src/a.swift").projectID, "p1")
        XCTAssertEqual(LinkedFolderPath.child(of: "src", hostPath: "src/a.swift"), "src/a.swift")
    }

    func testDeepSeekHarnessIsToldItCannotSeeThem() {
        XCTAssertNil(LinkedFolderReach.hint(count: 2, agent: "claude"))
        XCTAssertNil(LinkedFolderReach.hint(count: 0, agent: "dsh"))
        XCTAssertNotNil(LinkedFolderReach.hint(count: 2, agent: "dsh"))
        XCTAssertEqual(LinkedFolderReach.text(count: 2, agent: "codex"), "2 个，下一次启动时带上")
    }
}
