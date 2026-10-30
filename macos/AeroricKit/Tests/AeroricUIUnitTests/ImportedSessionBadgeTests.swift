import XCTest
@testable import AeroricUI

final class ImportedSessionBadgeTests: XCTestCase {
    func testTheBadgeNamesTheHarnessAndWhenTheSessionLastMoved() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let earlierToday = UInt64((now.timeIntervalSince1970 - 60) * 1000)
        let badge = ImportedSessionPane.badge(agent: "codex", lastAtMs: earlierToday, now: now)
        XCTAssertTrue(badge.hasPrefix("从 \(AgentGlyph.displayName(for: "codex")) 导入 · "), badge)
        XCTAssertTrue(badge.hasSuffix(TranscriptTurns.clock(earlierToday, now: now)), badge)
    }

    func testAnImportedSessionGroupsIntoTurnsWithTheWorkFolded() {
        let messages = [
            SessionMessage(role: "user", content: [.text(text: "修按钮颜色")], messageId: "u1"),
            SessionMessage(role: "assistant", content: [.text(text: "我先看看")], messageId: "a1"),
            SessionMessage(role: "assistant", content: [.text(text: "改好了")], messageId: "a2"),
            SessionMessage(role: "user", content: [.text(text: "深色也看看")], messageId: "u2"),
            SessionMessage(role: "assistant", content: [.text(text: "没问题")], messageId: "a3"),
        ]
        let turns = TranscriptTurns.group(TranscriptBuilder.items(from: messages, taskRunning: false))
        XCTAssertEqual(turns.count, 2)
        XCTAssertEqual(TranscriptTurns.text(of: turns[0].answer), "改好了")
        XCTAssertEqual(turns[0].process.count, 1)
        XCTAssertEqual(TranscriptTurns.text(of: turns[1].answer), "没问题")
    }
}
