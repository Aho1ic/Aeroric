import XCTest
import SwiftUI
@testable import AeroricUI

/// Arithmetic only: no hosting/window, defaults, client, agent or file I/O.
/// Overhaul M1 rules: flush columns, sidebar 260 (220–420), 56pt rail below
/// 1000, inspector 320 by default (260–520), centre protected at 480 — the
/// inspector shrinks, then hides.
final class MainShellGeometryTests: XCTestCase {
    private let shell = NativePanelGeometry.mainShell

    func testOverhaulConstants() {
        XCTAssertEqual(shell.defaultSidebarWidth, 260)
        XCTAssertEqual(shell.minimumSidebarWidth, 220)
        XCTAssertEqual(shell.maximumSidebarWidth, 420)
        XCTAssertEqual(shell.sidebarAutoCollapseWidth, 1000)
        XCTAssertEqual(shell.defaultInspectorWidth, 320)
        XCTAssertEqual(shell.minimumInspectorWidth, 260)
        XCTAssertEqual(shell.maximumInspectorWidth, 520)
        XCTAssertEqual(shell.minimumCentreWidth, 480)
        XCTAssertEqual(shell.minimumWindowWidth, 900)
        XCTAssertEqual(shell.minimumWindowHeight, 600)
        XCTAssertEqual(shell.columnGap, 0)
        XCTAssertEqual(shell.panelRadius, 0)
        XCTAssertEqual(shell.statusBarHeight, 24)
        XCTAssertEqual(shell.terminalBarHeight, 32)
        XCTAssertEqual(shell.terminalInitialFraction, 0.28, accuracy: 0.0001)
    }

    func testReferenceWidths() {
        for (width, left, centre, right): (CGFloat, CGFloat, CGFloat, CGFloat) in [
            (1280, 260, 700, 320),
            (1260, 260, 680, 320),
            (1000, 260, 480, 260),
            (990, 56, 614, 320),
            (900, 56, 524, 320),
        ] {
            let budget = NativePanelGeometry.mainShellBudget(totalWidth: width, preferredSidebarWidth: 260)
            XCTAssertEqual(budget.sidebarWidth, left, "width \(width)")
            XCTAssertEqual(budget.workspaceWidth, centre, "width \(width)")
            XCTAssertEqual(budget.inspectorWidth, right, "width \(width)")
        }
    }

    func testNarrowWindowFallsBackToRailWithoutChangingPreference() {
        let narrow = NativePanelGeometry.mainShellBudget(totalWidth: 990, preferredSidebarWidth: 360)
        XCTAssertTrue(narrow.sidebarAutoCollapsed)
        XCTAssertEqual(narrow.sidebarWidth, ProjectSidebar.collapsedWidth)
        let wide = NativePanelGeometry.mainShellBudget(totalWidth: 1400, preferredSidebarWidth: 360)
        XCTAssertFalse(wide.sidebarAutoCollapsed)
        XCTAssertEqual(wide.sidebarWidth, 360, "the stored drag preference returns on widening")
    }

    func testPreferencesAreClampedToTheirRanges() {
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 1600, preferredSidebarWidth: 100).sidebarWidth, 220)
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 1600, preferredSidebarWidth: 900).sidebarWidth, 420)
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 2000, preferredSidebarWidth: 260, preferredInspectorWidth: 900).inspectorWidth, 520)
        XCTAssertEqual(NativePanelGeometry.mainShellBudget(
            totalWidth: 2000, preferredSidebarWidth: 260, preferredInspectorWidth: 100).inspectorWidth, 260)
    }

    func testInspectorHidesRatherThanSqueezingTheCentre() {
        // 1100 with a 420 sidebar leaves 200pt beside a 480 centre: hide.
        let budget = NativePanelGeometry.mainShellBudget(totalWidth: 1100, preferredSidebarWidth: 420)
        XCTAssertEqual(budget.inspectorWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 680)
    }

    func testClosedInspectorHasNoPhantomWidth() {
        let budget = NativePanelGeometry.mainShellBudget(
            totalWidth: 1260, preferredSidebarWidth: 260, inspectorVisible: false)
        XCTAssertEqual(budget.inspectorWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 1000)
        XCTAssertEqual(budget.contentWidth, budget.workspaceWidth)
    }

    func testEveryAllowedWidthPreservesInvariants() {
        for width in stride(from: CGFloat(900), through: 2000, by: 20) {
            for preference: CGFloat in [220, 260, 340, 420] {
                for visible in [false, true] {
                    for collapsed in [false, true] {
                        let budget = NativePanelGeometry.mainShellBudget(
                            totalWidth: width, preferredSidebarWidth: preference,
                            inspectorVisible: visible, sidebarCollapsed: collapsed)
                        let label = "w\(width) p\(preference) v\(visible) c\(collapsed)"
                        XCTAssertEqual(budget.sidebarWidth + budget.contentWidth, width, label)
                        XCTAssertGreaterThanOrEqual(budget.workspaceWidth, shell.minimumCentreWidth, label)
                        XCTAssertTrue(budget.inspectorWidth == 0
                                      || (shell.minimumInspectorWidth...shell.maximumInspectorWidth)
                                          .contains(budget.inspectorWidth), label)
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
