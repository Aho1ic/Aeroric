import XCTest
import SwiftUI
@testable import AeroricUI

final class ToolSectionScaffoldTests: XCTestCase {
    func testOnlyComingSoonHidesTheSectionContent() {
        XCTAssertTrue(ToolSectionStatus.wired.showsContent)
        XCTAssertTrue(ToolSectionStatus.partial("说明").showsContent)
        XCTAssertFalse(ToolSectionStatus.comingSoon("说明").showsContent,
                       "an unwired section never renders its empty controls")
    }

    func testNewDecorativeRolesAreDynamic() {
        for role in [AeroricColor.teal, AeroricColor.violet] {
            let light = NSColor(role).usingColorSpace(.sRGB)
            XCTAssertNotNil(light)
        }
    }
}
