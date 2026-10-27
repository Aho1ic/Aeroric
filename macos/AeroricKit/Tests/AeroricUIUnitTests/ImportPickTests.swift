import AeroricClient
import AeroricProtocol
import XCTest
@testable import AeroricUI

/// The import picker's pure state (2026-10-27): grouping by working
/// directory, the tri-state project tick, search and the footer counts.
final class ImportPickTests: XCTestCase {
    private func row(_ id: String, cwd: String?, at: UInt64, project: String? = nil,
                     selected: Bool = true) -> ImportsClient.PreviewSession {
        .init(session: .init(id: id, agent: "claude", cwd: cwd, title: "会话 \(id)", lastAtMs: at,
                             projectId: project, tooLarge: false),
              selected: selected)
    }

    private var sample: ImportPick {
        ImportPick(rows: [
            row("a1", cwd: "/w/alpha", at: 10),
            row("a2", cwd: "/w/alpha", at: 30),
            row("b1", cwd: "/w/beta", at: 20, project: "p-beta"),
            row("x1", cwd: nil, at: 99, selected: false),
        ])
    }

    func testGroupsByDirectoryNewestFirstWithUnknownLast() {
        let groups = sample.groups
        XCTAssertEqual(groups.map(\.id), ["/w/alpha", "/w/beta", ""])
        XCTAssertEqual(groups[0].title, "alpha")
        XCTAssertEqual(groups[0].sessions.map(\.id), ["a2", "a1"])
        XCTAssertTrue(groups[0].isNew)
        XCTAssertFalse(groups[1].isNew, "已有项目的目录不算新项目")
        XCTAssertFalse(groups[2].isNew)
    }

    func testStartsFromTheHostsSelection() {
        XCTAssertEqual(sample.selected, ["a1", "a2", "b1"])
        XCTAssertEqual(sample.selectedCount, 3)
        XCTAssertEqual(sample.newProjectCount, 1)
    }

    func testProjectTickClearsThenFills() {
        var pick = sample
        let alpha = pick.groups[0]
        XCTAssertEqual(pick.tick(alpha), .all)
        pick.toggle("a1")
        XCTAssertEqual(pick.tick(alpha), .some)
        pick.toggle(group: alpha)
        XCTAssertEqual(pick.tick(alpha), .none, "部分勾选时点项目勾 = 全部取消")
        XCTAssertEqual(pick.newProjectCount, 0)
        pick.toggle(group: alpha)
        XCTAssertEqual(pick.tick(alpha), .all)
    }

    func testSearchKeepsMatchingProjectsWholeAndNarrowsOthers() {
        var pick = sample
        pick.query = "alp"
        XCTAssertEqual(pick.visibleGroups.map(\.id), ["/w/alpha"])
        XCTAssertEqual(pick.visibleGroups[0].sessions.count, 2)
        pick.query = "x1"
        XCTAssertEqual(pick.visibleGroups.map(\.id), [""])
        pick.selectAll()
        XCTAssertTrue(pick.selected.contains("x1"))
        pick.selectNone()
        XCTAssertEqual(pick.selected, ["a1", "a2", "b1"], "全不选只作用于搜索结果")
    }

    func testDecodesPreviewAndArchivedFlag() {
        let reply = JSONValue.object(["sessions": .array([
            .object(["id": .string("s1"), "agent": .string("codex"), "title": .string("t"),
                     "lastAtMs": .number("5"), "selected": .bool(false), "archived": .bool(true)]),
        ])])
        let rows = ImportsClient.decodePreview(reply)
        XCTAssertEqual(rows.count, 1)
        XCTAssertFalse(rows[0].selected)
        XCTAssertTrue(rows[0].session.archived)
    }
}
