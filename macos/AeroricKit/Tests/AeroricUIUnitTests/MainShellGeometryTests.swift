import XCTest
import SwiftUI
@testable import AeroricUI

/// Arithmetic only: no hosting/window, defaults, client, agent or file I/O.
/// Redesign 01 rules (DSH `columns.ts`): sidebar 280 (264–420), 56pt rail
/// below 1024, inspector ≥300 and 0.45 of the window on first open, centre
/// protected at 400 — the inspector shrinks, then hides.
final class MainShellGeometryTests: XCTestCase {
    private let shell = NativePanelGeometry.mainShell

    func testRedesignConstants() {
        XCTAssertEqual(shell.defaultSidebarWidth, 280)
        XCTAssertEqual(shell.minimumSidebarWidth, 264)
        XCTAssertEqual(shell.maximumSidebarWidth, 420)
        XCTAssertEqual(shell.sidebarAutoCollapseWidth, 1024)
        XCTAssertEqual(shell.minimumInspectorWidth, 300)
        XCTAssertEqual(shell.minimumCentreWidth, 400)
        XCTAssertEqual(shell.minimumWindowWidth, 860)
        XCTAssertEqual(shell.minimumWindowHeight, 560)
        XCTAssertEqual(shell.statusBarHeight, 24)
        XCTAssertEqual(shell.terminalBarHeight, 32)
        XCTAssertEqual(shell.terminalInitialFraction, 0.28, accuracy: 0.0001)
    }

    func testReferenceWidths() {
        for (width, left, centre, right): (CGFloat, CGFloat, CGFloat, CGFloat) in [
            (1280, 280, 424, 576),
            (1260, 280, 413, 567),
            (1024, 280, 400, 344),
            (1000, 56, 494, 450),
            (860, 56, 417, 387),
        ] {
            let budget = NativePanelGeometry.mainShellBudget(totalWidth: width, preferredSidebarWidth: 280)
            XCTAssertEqual(budget.sidebarWidth, left, "width \(width)")
            XCTAssertEqual(budget.workspaceWidth, centre, "width \(width)")
            XCTAssertEqual(budget.inspectorWidth, right, "width \(width)")
        }
    }

    func testNarrowWindowFallsBackToRailWithoutChangingPreference() {
        let narrow = NativePanelGeometry.mainShellBudget(totalWidth: 1000, preferredSidebarWidth: 360)
        XCTAssertTrue(narrow.sidebarAutoCollapsed)
        XCTAssertEqual(narrow.sidebarWidth, ProjectSidebar.collapsedWidth)
        let wide = NativePanelGeometry.mainShellBudget(totalWidth: 1400, preferredSidebarWidth: 360)
        XCTAssertFalse(wide.sidebarAutoCollapsed)
        XCTAssertEqual(wide.sidebarWidth, 360, "the stored drag preference returns on widening")
    }

    func testSidebarPreferenceIsClampedToDragRange() {
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 1600, preferredSidebarWidth: 100).sidebarWidth, 264)
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 1600, preferredSidebarWidth: 900).sidebarWidth, 420)
    }

    func testInspectorHidesRatherThanSqueezingTheCentre() {
        // 1024 with a 420 sidebar leaves 204pt: below the 300 floor, so hide.
        let budget = NativePanelGeometry.mainShellBudget(totalWidth: 1024, preferredSidebarWidth: 420)
        XCTAssertEqual(budget.inspectorWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 604)
    }

    func testClosedInspectorHasNoPhantomWidth() {
        let budget = NativePanelGeometry.mainShellBudget(
            totalWidth: 1260, preferredSidebarWidth: 280, inspectorVisible: false)
        XCTAssertEqual(budget.inspectorWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 980)
        XCTAssertEqual(budget.contentWidth, budget.workspaceWidth)
    }

    func testEveryAllowedWidthPreservesInvariants() {
        for width in stride(from: CGFloat(860), through: 2000, by: 20) {
            for preference: CGFloat in [264, 280, 340, 420] {
                for visible in [false, true] {
                    for collapsed in [false, true] {
                        let budget = NativePanelGeometry.mainShellBudget(
                            totalWidth: width, preferredSidebarWidth: preference,
                            inspectorVisible: visible, sidebarCollapsed: collapsed)
                        let label = "w\(width) p\(preference) v\(visible) c\(collapsed)"
                        XCTAssertEqual(budget.sidebarWidth + budget.contentWidth, width, label)
                        XCTAssertGreaterThanOrEqual(budget.workspaceWidth, shell.minimumCentreWidth, label)
                        XCTAssertTrue(budget.inspectorWidth == 0
                                      || budget.inspectorWidth >= shell.minimumInspectorWidth, label)
                        XCTAssertLessThanOrEqual(budget.inspectorWidth, width * shell.maximumInspectorFraction, label)
                        if collapsed { XCTAssertEqual(budget.sidebarWidth, 56, label) }
                        if !visible { XCTAssertEqual(budget.inspectorWidth, 0, label) }
                    }
                }
            }
        }
    }

    func testTerminalDockKeepsConversationVisible() {
        typealias Split = ConversationTerminalSplit<EmptyView, EmptyView>
        XCTAssertEqual(Split.terminalHeight(total: 600, fraction: 0.28), 168)
        XCTAssertEqual(Split.terminalHeight(total: 600, fraction: 0.95), 440, "conversation keeps 160pt")
        XCTAssertEqual(Split.terminalHeight(total: 600, fraction: 0.01), 96)
    }
}
