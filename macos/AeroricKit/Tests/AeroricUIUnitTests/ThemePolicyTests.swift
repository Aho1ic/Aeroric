import XCTest
import SwiftUI
@testable import AeroricUI

// Token policy tests. The number scales are the M1 design system
// (`AeroricSpace` / `AeroricRadius` / `AeroricMotion`); the React token
// mirror (`DSSpacing`, `DSRadius`, …) was removed in M4b. Colour policy is
// proven dynamic: every role must resolve differently under light vs dark.

final class ThemePolicyTests: XCTestCase {
    // MARK: Number scales

    func testSpacingScale() {
        XCTAssertEqual([AeroricSpace.xxs, AeroricSpace.xs, AeroricSpace.s, AeroricSpace.m, AeroricSpace.l,
                        AeroricSpace.xl, AeroricSpace.xxl, AeroricSpace.xxxl, AeroricSpace.huge],
                       [2, 4, 6, 8, 12, 16, 20, 24, 32])
    }

    func testRadiusScale() {
        XCTAssertEqual([AeroricRadius.xs, AeroricRadius.s, AeroricRadius.m, AeroricRadius.l, AeroricRadius.xl,
                        AeroricRadius.panel], [4, 8, 12, 16, 20, 28])
    }

    func testMotionScale() {
        XCTAssertEqual(AeroricMotion.quick, 0.10, accuracy: 1e-9)
        XCTAssertEqual(AeroricMotion.standard, 0.20, accuracy: 1e-9)
        XCTAssertEqual(AeroricMotion.slow, 0.30, accuracy: 1e-9)
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

    // MARK: Overhaul M1 colour roles

    private func swatch(_ appearance: NSAppearance.Name, light: AeroricColor.Swatch, dark: AeroricColor.Swatch,
                        lightContrast: AeroricColor.Swatch? = nil,
                        darkContrast: AeroricColor.Swatch? = nil) -> AeroricColor.Swatch {
        AeroricColor.swatch(for: appearance, light: light, dark: dark,
                            lightContrast: lightContrast, darkContrast: darkContrast)
    }

    func testRolesAnswerForAllFourAppearances() {
        XCTAssertEqual(swatch(.aqua, light: .rgb(1), dark: .rgb(2), lightContrast: .rgb(3), darkContrast: .rgb(4)), .rgb(1))
        XCTAssertEqual(swatch(.darkAqua, light: .rgb(1), dark: .rgb(2), lightContrast: .rgb(3), darkContrast: .rgb(4)), .rgb(2))
        XCTAssertEqual(swatch(.accessibilityHighContrastAqua, light: .rgb(1), dark: .rgb(2),
                              lightContrast: .rgb(3), darkContrast: .rgb(4)), .rgb(3))
        XCTAssertEqual(swatch(.accessibilityHighContrastDarkAqua, light: .rgb(1), dark: .rgb(2),
                              lightContrast: .rgb(3), darkContrast: .rgb(4)), .rgb(4))
        // A role without contrast variants falls back to its light/dark value.
        XCTAssertEqual(swatch(.accessibilityHighContrastDarkAqua, light: .rgb(1), dark: .rgb(2)), .rgb(2))
        // Real appearances match onto the four names.
        XCTAssertEqual(NSAppearance(named: .darkAqua)?.bestMatch(from: AeroricColor.appearances), .darkAqua)
        XCTAssertEqual(NSAppearance(named: .aqua)?.bestMatch(from: AeroricColor.appearances), .aqua)
    }

    func testSwatchesResolveToTheirComponents() {
        let accent = AeroricColor.Swatch.rgb(0x4176E6).nsColor
        XCTAssertEqual(accent.redComponent, 0x41 / 255, accuracy: 1e-6)
        XCTAssertEqual(accent.greenComponent, 0x76 / 255, accuracy: 1e-6)
        XCTAssertEqual(accent.blueComponent, 0xE6 / 255, accuracy: 1e-6)
        XCTAssertEqual(AeroricColor.Swatch.white(0.12).nsColor.alphaComponent, 0.12, accuracy: 1e-6)
        XCTAssertEqual(AeroricColor.Swatch.rgba(0x263148, 0.06).nsColor.alphaComponent, 0.06, accuracy: 1e-6)
    }

    // MARK: Overhaul M1 glass policy

    func testGlassPolicyOrder() {
        // Reduce Transparency wins over everything.
        XCTAssertEqual(GlassSurfacePolicy.rendering(preference: .glass, reduceTransparency: true,
                                                    liquidGlassAvailable: true, legacyPreview: false), .opaque)
        // The user's classic preference is opaque too.
        XCTAssertEqual(GlassSurfacePolicy.rendering(preference: .classic, reduceTransparency: false,
                                                    liquidGlassAvailable: true, legacyPreview: false), .opaque)
        // macOS 26+: Liquid Glass.
        XCTAssertEqual(GlassSurfacePolicy.rendering(preference: .glass, reduceTransparency: false,
                                                    liquidGlassAvailable: true, legacyPreview: false), .liquidGlass)
        // macOS 14–25, and the DEBUG preview of it: system material.
        XCTAssertEqual(GlassSurfacePolicy.rendering(preference: .glass, reduceTransparency: false,
                                                    liquidGlassAvailable: false, legacyPreview: false), .material)
        XCTAssertEqual(GlassSurfacePolicy.rendering(preference: .glass, reduceTransparency: false,
                                                    liquidGlassAvailable: true, legacyPreview: true), .material)
    }

    func testLegacyPreviewIsDebugOnly() {
        #if DEBUG
        XCTAssertTrue(LegacySurfacePreview.isAvailable)
        #else
        XCTAssertFalse(LegacySurfacePreview.isAvailable)
        XCTAssertFalse(LegacySurfacePreview.launchEnabled)
        #endif
    }

    // MARK: Overhaul M1 indicators

    func testWireStatusesMapToActivityStates() {
        XCTAssertEqual(ActivityState(wireStatus: "pending"), .running)
        XCTAssertEqual(ActivityState(wireStatus: "running"), .running)
        XCTAssertEqual(ActivityState(wireStatus: "detached"), .running)
        XCTAssertEqual(ActivityState(wireStatus: "input_required"), .waiting)
        XCTAssertEqual(ActivityState(wireStatus: "done"), .done)
        XCTAssertEqual(ActivityState(wireStatus: "failed"), .failed)
        XCTAssertEqual(ActivityState(wireStatus: "cancelled"), .idle)
        XCTAssertEqual(ActivityState(wireStatus: nil), .idle)
        XCTAssertEqual(ActivityState(wireStatus: "something-new"), .idle)
    }

    func testRelativeTimeBuckets() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-10), now: now), "刚刚")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-5 * 60), now: now), "5 分钟")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-2 * 3600), now: now), "2 小时")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(-3 * 86_400), now: now), "3 天")
        XCTAssertEqual(RelativeTime.short(now.addingTimeInterval(60), now: now), "刚刚", "future clamps to now")
    }

    func testAgentGlyphsNeverFallBackToEmpty() {
        for agent in ["claude", "claude_gpt55", "codex", "dsh", "omp", "custom-x", "", nil] as [String?] {
            XCTAssertFalse(AgentGlyph.symbol(for: agent).isEmpty)
            XCTAssertFalse(AgentGlyph.displayName(for: agent).isEmpty)
        }
    }

    // MARK: Motion policy

    func testReduceMotionCollapsesDurations() {
        XCTAssertEqual(AeroricMotion.duration(AeroricMotion.slow, reduceMotion: true), 0)
        XCTAssertEqual(AeroricMotion.duration(AeroricMotion.quick, reduceMotion: false), AeroricMotion.quick)
        XCTAssertNil(AeroricMotion.animation(AeroricMotion.slow, reduceMotion: true))
    }
}
