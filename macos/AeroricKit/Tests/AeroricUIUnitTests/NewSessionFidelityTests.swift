import XCTest
@testable import AeroricUI

/// Values only: no View/hosting/window, defaults, client, file, agent or network.
final class NewSessionFidelityTests: XCTestCase {
    func testConfirmedInputRadiusIsSixteenInBothDensityModes() {
        XCTAssertEqual(NewSessionGeometry.inputRadius(compact: false), 16)
        XCTAssertEqual(NewSessionGeometry.inputRadius(compact: true), 16)
    }

    func testReadableColumnCapExcludesPagePadding() {
        XCTAssertEqual(NewSessionGeometry.pageMaximumWidth, 850)
        for (viewport, compact, expected): (CGFloat, Bool, CGFloat) in [
            (534, true, 500),
            (714, false, 646),
            (900, false, 832),
            (918, false, 850),
            (1182, false, 850),
        ] {
            let column = min(NewSessionGeometry.pageMaximumWidth,
                             viewport - 2 * NewSessionGeometry.pageSidePadding(compact: compact))
            XCTAssertEqual(column, expected, "viewport \(viewport)")
        }
    }

    func testUnchangedInputAndHarnessRequirementsRemainIndependent() {
        XCTAssertEqual(NewSessionGeometry.promptHeight(compact: false), 108)
        XCTAssertEqual(NewSessionGeometry.promptHeight(compact: true), 104)
        XCTAssertEqual(NewSessionGeometry.contextGap, 6)
        XCTAssertEqual(NewSessionGeometry.harnessHeight, 34)
        XCTAssertEqual(NewSessionGeometry.harnessItemHeight, 28)
        XCTAssertEqual(NewSessionGeometry.harnessIconSize, 16)
        XCTAssertEqual(NativePanelGeometry.panelRadius, 18)
        // Redesign 01: flush columns, no floating 25pt cards.
        XCTAssertEqual(NativePanelGeometry.mainShell.panelRadius, 0)
    }
}
