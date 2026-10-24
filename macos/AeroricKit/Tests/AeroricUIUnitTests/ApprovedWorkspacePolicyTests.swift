import XCTest
@testable import AeroricUI

/// Pure values and isolated empty stores only. No clients/refresh calls,
/// windows, defaults, files, agents or network.
final class ApprovedWorkspacePolicyTests: XCTestCase {
    func testOrdinaryBudgetDoesNotDeductBrowserOutline() {
        for width: CGFloat in [1000, 1260] {
            for sidebar: CGFloat in [56, 180, 320, 360] {
                let centre = NativePanelGeometry.workspaceWidth(totalWidth: width, sidebarWidth: sidebar)
                XCTAssertEqual(centre + sidebar + 2 * NativePanelGeometry.panelInset
                               + NativePanelGeometry.panelGap, width)
                XCTAssertGreaterThanOrEqual(centre, 600)
            }
        }
    }

    func testTreeKeyboardOrderFollowsExpandedProjectsAndKeepsIndependentBranches() {
        let items = [
            SidebarItem(id: "all", title: "全部任务", group: .work),
            SidebarItem(id: "a", title: "A", children: [SidebarItem(id: "ta", title: "A task")]),
            SidebarItem(id: "b", title: "B", children: [SidebarItem(id: "tb", title: "B task")]),
        ]
        XCTAssertEqual(SidebarOrdering.rendered(items, expanded: []).map(\.id), ["all", "a", "b"])
        XCTAssertEqual(SidebarOrdering.rendered(items, expanded: ["a", "b"]).map(\.id), ["all", "a", "ta", "b", "tb"])
        XCTAssertEqual(SidebarOrdering.item(id: "tb", in: items)?.title, "B task")
    }

    @MainActor func testGitEmptyRequiresSuccessfulSnapshotAndFailureKeepsOldValues() {
        let store = GitStore()
        XCTAssertEqual(store.changesReadState, .notLoaded)
        XCTAssertNotNil(store.changesReadState.notice)
        store.ingest(changes: [], conflictPaths: [])
        XCTAssertEqual(store.changesReadState, .loaded)
        XCTAssertNil(store.changesReadState.notice)
        store.ingest(commits: [.init(hash: "h", shortHash: "h", author: "fixture",
                                     date: "fixture", message: "retained")])
        store.failRead("fixture failure")
        XCTAssertEqual(store.historyReadState, .failed("fixture failure"))
        XCTAssertEqual(store.commits.first?.message, "retained")
    }

    @MainActor func testNilRemoteClientCannotAdvertiseControlOrSuccessfulEmpty() {
        let store = RemoteStore(client: nil)
        XCTAssertEqual(store.readState, .unavailable)
        XCTAssertFalse(store.canControl)
        XCTAssertNotNil(store.readState.notice)
        XCTAssertEqual(store.devices.count, 0)
    }
}
