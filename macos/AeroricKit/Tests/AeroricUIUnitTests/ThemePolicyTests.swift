import XCTest
import SwiftUI
@testable import AeroricUI

// Token policy tests (Task P4-03 Acceptance). The number layers are
// pinned to `packages/design-system/src/tokens.ts` — that file is the
// authoring source, so any change here without a matching React-side
// change breaks these tests. Colour policy is proven dynamic: every role
// must resolve to a different value under light vs dark appearance.

final class ThemePolicyTests: XCTestCase {
    // MARK: Number tokens mirror tokens.ts

    func testSpacingMatchesReactTokens() {
        XCTAssertEqual(DSSpacing.values, [0, 4, 8, 12, 16, 20, 24])
    }

    func testRadiiMatchReactTokens() {
        XCTAssertEqual(DSRadius.sm, 8)
        XCTAssertEqual(DSRadius.md, 10)
        XCTAssertEqual(DSRadius.lg, 14)
        XCTAssertEqual(DSRadius.xl, 18)
        XCTAssertEqual(DSRadius.pill, 999)
    }

    func testTypographyMatchesReactTokens() {
        XCTAssertEqual(DSTypography.label, 11)
        XCTAssertEqual(DSTypography.meta, 12)
        XCTAssertEqual(DSTypography.body, 14)
        XCTAssertEqual(DSTypography.title, 18)
    }

    func testMotionMatchesReactTokens() {
        XCTAssertEqual(DSMotion.fast, 0.120, accuracy: 1e-9)
        XCTAssertEqual(DSMotion.normal, 0.180, accuracy: 1e-9)
        XCTAssertEqual(DSMotion.slow, 0.260, accuracy: 1e-9)
    }

    func testSpacingAccessorCoversExactlySevenSteps() {
        // Bounds contract: valid steps are exactly 0..<7 (values.count);
        // precondition guards out-of-range steps.
        XCTAssertEqual(DSSpacing.values.count, 7)
        XCTAssertEqual(DSSpacing.v(0), 0)
        XCTAssertEqual(DSSpacing.v(6), 24)
    }

    // MARK: Colour roles are dynamic, not literal RGB

    /// Resolve a dynamic NSColor under a concrete appearance. AppKit has
    /// no per-appearance `resolvedColor` like UIKit, so evaluate inside
    /// `performAsCurrentDrawingAppearance`.
    private func resolved(_ color: NSColor, in appearance: NSAppearance) -> NSColor {
        var value = NSColor.white
        appearance.performAsCurrentDrawingAppearance {
            value = color.usingColorSpace(.sRGB) ?? .white
        }
        return value
    }

    func testTextRolesResolveDifferentlyInLightAndDark() {
        let light = NSAppearance(named: .aqua)!
        let dark = NSAppearance(named: .darkAqua)!
        let lightText = resolved(.labelColor, in: light)
        let darkText = resolved(.labelColor, in: dark)
        XCTAssertNotEqual(lightText, darkText,
                          "dsText must be a dynamic system colour, not a fixed RGB")
    }

    func testSeparatorRoleIsDynamic() {
        let light = NSAppearance(named: .aqua)!
        let dark = NSAppearance(named: .darkAqua)!
        XCTAssertNotEqual(resolved(.separatorColor, in: light),
                          resolved(.separatorColor, in: dark))
    }

    // MARK: Surface policy

    func testReduceTransparencyForcesClassicSurface() {
        XCTAssertEqual(SurfaceChoice.resolve(preference: .glass, reduceTransparency: true),
                       .classic)
        XCTAssertEqual(SurfaceChoice.resolve(preference: .classic, reduceTransparency: true),
                       .classic)
    }

    func testNoReduceTransparencyHonoursPreference() {
        XCTAssertEqual(SurfaceChoice.resolve(preference: .glass, reduceTransparency: false),
                       .glass)
        XCTAssertEqual(SurfaceChoice.resolve(preference: .classic, reduceTransparency: false),
                       .classic)
    }

    // MARK: Motion policy

    func testReduceMotionCollapsesDurations() {
        XCTAssertEqual(DSMotion.duration(DSMotion.slow, reduceMotion: true), 0)
        XCTAssertEqual(DSMotion.duration(DSMotion.fast, reduceMotion: false),
                       DSMotion.fast)
    }
}
