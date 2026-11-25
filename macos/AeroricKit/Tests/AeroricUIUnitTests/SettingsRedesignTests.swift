import AeroricClient
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
        XCTAssertEqual(SettingsIndex.search("写文件", in: all).map(\.anchor), ["允许手机改文件"])
        XCTAssertEqual(SettingsIndex.search("磁盘", in: all).map(\.pane), [.storage])
        XCTAssertEqual(SettingsPane.storage.group, .data)
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

    /// 2026-11-24 存储: rows name a folder under the data folder by its
    /// own name, and others in full with the home folder as ~.
    func testStorageRowsNameTheirFolders() {
        let data = "/Users/me/Library/Application Support/com.aeroric.native"
        XCTAssertEqual(StorageSettingsPane.name(of: data + "/agent-homes", in: data), "agent-homes")
        XCTAssertEqual(StorageSettingsPane.name(of: data + "/agent-homes", in: data + "/"), "agent-homes")
        XCTAssertEqual(StorageSettingsPane.name(of: NSHomeDirectory() + "/code/app/.aeroric/worktrees", in: data),
                       "~/code/app/.aeroric/worktrees")
        XCTAssertEqual(StorageSettingsPane.name(of: "/opt/x/.aeroric/worktrees", in: data), "/opt/x/.aeroric/worktrees")
    }

    /// 2026-11-24 工作副本: each copy says what will happen to it.
    func testACopySaysWhetherItWillBeRemoved() {
        let now = Date(timeIntervalSince1970: 1_795_500_000)
        func copy(cleared: TimeInterval?) -> IsolatedCopy {
            IsolatedCopy(taskID: "t", title: nil, path: "/p/.aeroric/worktrees/t", origin: "/code/app",
                         createdMs: 1_795_000_000_000, clearedMs: cleared.map { Int64($0 * 1000) })
        }
        let working = WorktreeSettingsPane.detail(copy(cleared: nil), keepDays: 30, now: now)
        XCTAssertTrue(working.hasPrefix("app · 创建于 "), working)
        XCTAssertTrue(working.hasSuffix("还有改动没带回"), working)
        let emptied = WorktreeSettingsPane.detail(copy(cleared: 1_795_500_000 - 86_400 * 3), keepDays: 30, now: now)
        XCTAssertTrue(emptied.hasSuffix("已清空，约 27 天后删除"), emptied)
        let due = WorktreeSettingsPane.detail(copy(cleared: 1_795_500_000 - 86_400 * 40), keepDays: 30, now: now)
        XCTAssertTrue(due.hasSuffix("已清空，下次宿主启动时删除"), due)
        let kept = WorktreeSettingsPane.detail(copy(cleared: 1_795_000_000), keepDays: 0, now: now)
        XCTAssertTrue(kept.hasSuffix("已清空，不自动删"), kept)
        XCTAssertTrue(WorktreeSettingsPane.keepChoices.contains { $0.0 == 30 }, "the default is a choice")
        XCTAssertEqual(SettingsIndex.search("自动删除", in: SettingsPane.allCases).map(\.anchor), ["没有改动的副本保留多久"])
    }

    func testShortcutsListTheMenuKeys() {
        let keys = ShortcutsSettingsPane.groups.flatMap(\.items).map(\.keys)
        XCTAssertTrue(keys.contains("⌘N"))
        XCTAssertTrue(keys.contains("⌘K"))
        XCTAssertTrue(keys.contains("⌥↩"))
    }
}
