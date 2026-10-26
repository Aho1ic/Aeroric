@testable import AeroricUI
import SwiftUI
import XCTest

/// E: where messages sit and when the session-info column shows.
final class SessionLayoutTests: XCTestCase {
    func testTheUsersBubbleHugsTheRightEdgeAndAgentProseTheLeft() {
        XCTAssertEqual(MessageBubble.alignment(role: "user"), .trailing)
        XCTAssertEqual(MessageBubble.alignment(role: "assistant"), .leading)
        XCTAssertEqual(MessageBubble.userWidthRatio, 0.70)
    }

    func testTheInfoColumnFoldsWhenTheWindowIsNarrowOrTheUserSaysSo() {
        XCTAssertTrue(SessionInfoLayout.showsColumn(collapsed: false, centreWidth: 1100))
        XCTAssertFalse(SessionInfoLayout.showsColumn(collapsed: true, centreWidth: 1100))
        XCTAssertFalse(SessionInfoLayout.showsColumn(collapsed: false, centreWidth: 700))
        XCTAssertEqual(SessionInfoLayout.clampWidth(100), 260)
        XCTAssertEqual(SessionInfoLayout.clampWidth(300), 300)
        XCTAssertEqual(SessionInfoLayout.clampWidth(900), 340)
    }
}
