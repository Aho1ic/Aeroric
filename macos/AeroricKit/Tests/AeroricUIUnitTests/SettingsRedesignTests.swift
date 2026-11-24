@testable import AeroricUI
import XCTest

/// 1124 settings redesign (note `2026-11-24-settings-redesign`).
final class SettingsRedesignTests: XCTestCase {
    func testEveryPageHasAGroupTitleAndSubtitle() {
        for pane in SettingsPane.allCases {
            XCTAssertFalse(pane.title.isEmpty)
            XCTAssertFalse(pane.subtitle.isEmpty, pane.rawValue)
        }
        let order = SettingsPane.allCases.map(\.group)
        XCTAssertEqual(order, order.sorted { SettingsGroup.allCases.firstIndex(of: $0)! < SettingsGroup.allCases.firstIndex(of: $1)! },
                       "pages are listed group by group")
        XCTAssertEqual(SettingsPane.remote.group, .integrations)
        XCTAssertNil(SettingsGroup.about.title)
    }

    func testSearchFindsRowsByTheirWords() {
        let all = SettingsPane.allCases
        XCTAssertEqual(SettingsIndex.search("睡眠", in: all).map(\.anchor), ["运行任务时阻止 Mac 睡眠"])
        XCTAssertEqual(SettingsIndex.search("深色", in: all).first?.pane, .appearance)
        XCTAssertTrue(SettingsIndex.search("手机 配对", in: all).contains { $0.pane == .remote })
        XCTAssertTrue(SettingsIndex.search("通知", in: all).allSatisfy { $0.pane == .notifications })
        XCTAssertTrue(SettingsIndex.search("MCP", in: all).contains { $0.anchor == "MCP 服务器" })
        XCTAssertTrue(SettingsIndex.search("   ", in: all).isEmpty)
        XCTAssertTrue(SettingsIndex.search("没有这种设置", in: all).isEmpty)
        XCTAssertTrue(SettingsIndex.search("手机", in: all.filter { $0 != .remote }).isEmpty,
                      "a hidden page is not searched")
        let ids = SettingsIndex.entries.map(\.id)
        XCTAssertEqual(ids.count, Set(ids).count, "anchors are unique per page")
    }

    func testFinishedNotificationsOnlyForWorkThatEndedInTheBackground() {
        XCTAssertTrue(FinishedNotificationGate.shouldNotify(previous: "running", current: "done", appActive: false))
        XCTAssertTrue(FinishedNotificationGate.shouldNotify(previous: "input_required", current: "failed", appActive: false))
        XCTAssertFalse(FinishedNotificationGate.shouldNotify(previous: "running", current: "done", appActive: true))
        XCTAssertFalse(FinishedNotificationGate.shouldNotify(previous: "done", current: "done", appActive: false))
        XCTAssertFalse(FinishedNotificationGate.shouldNotify(previous: "running", current: "interrupted", appActive: false))
        XCTAssertFalse(FinishedNotificationGate.shouldNotify(previous: nil, current: "done", appActive: false))
    }

    func testShortcutsListTheMenuKeys() {
        let keys = ShortcutsSettingsPane.groups.flatMap(\.items).map(\.keys)
        XCTAssertTrue(keys.contains("⌘N"))
        XCTAssertTrue(keys.contains("⌘K"))
        XCTAssertTrue(keys.contains("⌥↩"))
    }
}
