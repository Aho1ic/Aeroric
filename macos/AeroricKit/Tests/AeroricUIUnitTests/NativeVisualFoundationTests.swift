import AppKit
import SwiftUI
import XCTest
@testable import AeroricUI

final class NativeVisualFoundationTests: XCTestCase {
    func testGeometryReusesExistingTokens() {
        XCTAssertEqual(NativePanelGeometry.panelRadius, AeroricRadius.xl)
        XCTAssertEqual(NativePanelGeometry.panelInset, AeroricSpace.m)
        XCTAssertEqual(NativePanelGeometry.panelGap, AeroricSpace.m)
        XCTAssertEqual(NativePanelGeometry.navigationRowHeight, 32)
        XCTAssertEqual(NativePanelGeometry.statusBarHeight, 32)
    }

    func testRegularReviewBudgetAccountsForEveryPoint() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 1260, sidebarWidth: 320, inspectorWidth: 320, inspectorVisible: true
        )
        XCTAssertEqual(budget.innerWidth, 1258)
        XCTAssertEqual(budget.workspaceWidth, 586)
        XCTAssertEqual(budget.gaps, 2)
        assertConserved(budget)
    }

    func testCompactBudgetIsOnlyAReviewFixture() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 1000, sidebarWidth: 284, inspectorWidth: 273, inspectorVisible: true
        )
        XCTAssertEqual(budget.workspaceWidth, 409)
        XCTAssertEqual(budget.summary, "1000 → 284/409/273")
        assertConserved(budget)
    }

    func testMinimumSampleHasNoInvisibleColumnGap() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 400, sidebarWidth: 0, inspectorWidth: 0, inspectorVisible: false
        )
        XCTAssertEqual(budget.workspaceWidth, 382)
        XCTAssertEqual(budget.gaps, 0)
        assertConserved(budget)
    }

    func testClosedInspectorConsumesNeitherWidthNorExtraGap() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 1260, sidebarWidth: 320, inspectorWidth: 320, inspectorVisible: false
        )
        XCTAssertEqual(budget.inspectorWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 914)
        XCTAssertEqual(budget.gaps, 1)
        assertConserved(budget)
    }

    func testZeroWidthInspectorDoesNotAddAGap() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 1000, sidebarWidth: 284, inspectorWidth: 0, inspectorVisible: true
        )
        XCTAssertEqual(budget.gaps, 1)
        assertConserved(budget)
    }

    func testInspectorWithoutSidebarNeedsOnlyOneGap() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 1000, sidebarWidth: 0, inspectorWidth: 273, inspectorVisible: true
        )
        XCTAssertEqual(budget.gaps, 1)
        XCTAssertEqual(budget.workspaceWidth, 701)
        assertConserved(budget)
    }

    func testImpossibleBudgetNeverProducesNegativeContentWidth() {
        let budget = NativePanelGeometry.budget(
            totalWidth: 12, sidebarWidth: -4, inspectorWidth: 320, inspectorVisible: true
        )
        XCTAssertEqual(budget.sidebarWidth, 0)
        XCTAssertEqual(budget.workspaceWidth, 0)
    }

    func testSurfacePolicyNeverLetsAnOverrideDisableSystemAccessibility() {
        for preference in SurfacePreference.allCases {
            for system in [false, true] {
                for override: Bool? in [nil, false, true] {
                    let result = SurfaceChoice.resolve(
                        preference: preference, reduceTransparency: system,
                        reduceTransparencyOverride: override
                    )
                    XCTAssertEqual(result, system || override == true ? .classic : preference)
                }
            }
        }
    }

    func testOriginalSurfaceFunctionSignatureRemainsUsable() {
        let resolve: (SurfacePreference, Bool) -> SurfacePreference = SurfaceChoice.resolve
        XCTAssertEqual(resolve(.glass, false), .glass)
        XCTAssertEqual(resolve(.glass, true), .classic)
    }

    func testIncreasedContrastStrengthensTheSameSemanticBoundary() {
        let standard = NativePanelGeometry.outline(increasedContrast: false)
        let increased = NativePanelGeometry.outline(increasedContrast: true)
        XCTAssertGreaterThan(increased.width, standard.width)
        XCTAssertGreaterThan(increased.opacity, standard.opacity)
        XCTAssertEqual(increased.width, 1)
        XCTAssertEqual(increased.opacity, 1)
    }

    func testReducedMotionRemovesTheReviewAnimationDuration() {
        XCTAssertEqual(AeroricMotion.duration(AeroricMotion.standard, reduceMotion: true), 0)
        XCTAssertEqual(AeroricMotion.duration(AeroricMotion.standard, reduceMotion: false), AeroricMotion.standard)
    }

    @MainActor
    func testLegacyGlassPanelConstructionAndResolvedInitializerStillCompose() {
        let content = VStack {
            GlassPanel(preference: .glass) { Text("environment") }
            GlassPanel(preference: .glass, reduceTransparency: false) { Text("legacy false") }
            GlassPanel(preference: .glass, reduceTransparency: true) { Text("legacy true") }
            GlassPanel(resolved: .classic) { Text("resolved") }
        }
        let host = NSHostingView(rootView: content.frame(width: 320, height: 180))
        XCTAssertEqual(host.fittingSize.width, 320)
        XCTAssertEqual(host.fittingSize.height, 180)
    }

    @MainActor
    func testIconStyleKeepsSystemPrimitiveButtonSemantics() {
        func requireNativeActivation<S: PrimitiveButtonStyle>(_ style: S) {}
        requireNativeActivation(NativeIconButtonStyle())
        let icon = NativeIconButton(systemImage: "plus", label: "添加样本",
                                    identifier: "unit.review.add") {}
        let host = NSHostingView(rootView: icon)
        XCTAssertGreaterThanOrEqual(host.fittingSize.width, NativePanelGeometry.iconButtonSize)
        XCTAssertGreaterThanOrEqual(host.fittingSize.height, NativePanelGeometry.iconButtonSize)
    }

    @MainActor
    func testSharedFoundationAndReadableInputsComposeWithoutAWindowOrClient() {
        for preference in SurfacePreference.allCases {
            let content = GlassPanel(preference: preference) {
                VStack {
                    TextField("单行", text: .constant("input"))
                    TextEditor(text: .constant("多行\ninput"))
                    NativeSegmentedPicker(
                        selection: .constant(0), groupTitle: "样本选择",
                        segments: [.init(value: 0, title: "一"), .init(value: 1, title: "二")]
                    )
                }.padding(NativePanelGeometry.panelInset)
            }
            .background(NativePanelFoundation(preference: preference))
            .frame(width: 382, height: 400)
            let host = NSHostingView(rootView: content)
            XCTAssertEqual(host.fittingSize.width, 382)
            XCTAssertEqual(host.fittingSize.height, 400)
        }
    }

    #if DEBUG
    @MainActor
    func testDebugReviewUsesTheSharedComponentsWithoutStartingAnApplication() {
        // This is assembly coverage, NOT real-window/AX acceptance.
        let host = NSHostingView(rootView: NativeVisualFoundationReviewView())
        XCTAssertGreaterThanOrEqual(host.fittingSize.width, 400)
        let minimum = NativePanelGeometry.reviewSample(id: "minimum")
        let content = NSWindow.contentRect(
            forFrameRect: NSRect(x: 0, y: 0, width: minimum.totalWidth, height: minimum.totalHeight),
            styleMask: [.titled, .closable, .miniaturizable, .resizable]
        )
        XCTAssertLessThanOrEqual(
            host.fittingSize.height, content.height,
            "400×600 describes the outer window, not a 600pt content minimum plus a titlebar"
        )
    }

    func testReviewWindowPlacementFitsTheWholeSampleAndKeepsItsDisplay() {
        let laptop = CGRect(x: 0, y: 81, width: 1470, height: 842)
        let external = CGRect(x: -399, y: 956, width: 2560, height: 1440)
        let regular = CGSize(width: 1260, height: 860)
        let frame = ReviewWindowPlacement.frame(
            size: regular, visibleFrames: [laptop, external], preferred: laptop
        )
        XCTAssertEqual(frame, CGRect(x: 251, y: 1246, width: 1260, height: 860))
        XCTAssertTrue(frame.map(external.contains) ?? false)
        let small = ReviewWindowPlacement.frame(
            size: CGSize(width: 400, height: 600),
            visibleFrames: [laptop, external], preferred: external
        )
        XCTAssertEqual(small, CGRect(x: 681, y: 1376, width: 400, height: 600))
        XCTAssertNil(ReviewWindowPlacement.frame(
            size: regular, visibleFrames: [laptop], preferred: laptop
        ), "an undersized display must not silently count as the 1260×860 sample")
    }

    func testAllApprovedReviewSampleBudgets() {
        XCTAssertEqual(NativePanelGeometry.reviewSamples.map(\.budget.summary), [
            "1260 → 320/586/320", "1000 → 284/409/273", "400 → 382",
        ])
        XCTAssertEqual(NativePanelGeometry.reviewSample(id: "unknown"),
                       NativePanelGeometry.defaultReviewSample)
    }

    func testReviewEntryRequiresEveryExplicitOptIn() {
        XCTAssertTrue(NativeVisualReviewEntry.isActive(environment: reviewEnvironment))
        for key in [NativeVisualReviewEntry.flagKey, NativeVisualReviewEntry.environmentKey,
                    NativeVisualReviewEntry.dataDirectoryKey] {
            var environment = reviewEnvironment
            environment.removeValue(forKey: key)
            XCTAssertFalse(NativeVisualReviewEntry.isActive(environment: environment), key)
        }
    }

    func testReviewEntryRejectsWrongEnvironmentAndNonExactFlag() {
        for value in ["", "default", "production", "test"] {
            var environment = reviewEnvironment
            environment[NativeVisualReviewEntry.environmentKey] = value
            XCTAssertFalse(NativeVisualReviewEntry.isActive(environment: environment), value)
        }
        for value in ["", "0", "true", " 1"] {
            var environment = reviewEnvironment
            environment[NativeVisualReviewEntry.flagKey] = value
            XCTAssertFalse(NativeVisualReviewEntry.isActive(environment: environment), value)
        }
    }

    func testReviewEntryRejectsMissingBlankAndProductionDirectories() {
        for path in ["", " ", NSHomeDirectory() + "/.aeroric",
                     NSHomeDirectory() + "/Library/Application Support/Aeroric",
                     "Library/Caches/review"] {
            var environment = reviewEnvironment
            environment[NativeVisualReviewEntry.dataDirectoryKey] = path
            XCTAssertFalse(NativeVisualReviewEntry.isActive(environment: environment), path)
        }
    }

    func testCachesRootItselfIsNotAnIsolatedDataDirectory() {
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            NSHomeDirectory() + "/Library/Caches"
        ))
    }

    func testDirectoryBoundaryRejectsSiblingPrefixAndTraversal() {
        let home = "/tmp/aeroric-visual-policy-home"
        for path in [home + "/Library/Caches-neighbour/review",
                     home + "/Library/Caches/../Application Support/Aeroric"] {
            XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(path, home: home))
        }
        XCTAssertTrue(NativeVisualReviewEntry.isIsolatedDataDirectory(
            home + "/Library/Caches/review-unique", home: home
        ))
        // A sandboxed XCTest runner has a different NSHomeDirectory than
        // the account used by the unsandboxed app. An explicit account home
        // preserves the boundary; neither a container home nor production
        // storage may accidentally become an accepted substitute.
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            home + "/Library/Caches/review-unique",
            home: home + "/Library/Containers/uitests/Data"
        ))
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            home + "/Library/Application Support/Aeroric", home: home
        ))
    }

    func testSymlinkCannotEscapeCachesBoundary() throws {
        let fixture = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-visual-boundary-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: fixture) }
        let caches = fixture.appendingPathComponent("home/Library/Caches")
        let outside = fixture.appendingPathComponent("outside")
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        let link = caches.appendingPathComponent("linked-review")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: outside)
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            link.appendingPathComponent("not-yet-created").path,
            home: fixture.appendingPathComponent("home").path
        ))
    }

    func testExistingFileCannotServeAsIsolatedDirectory() throws {
        let fixture = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-visual-file-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: fixture) }
        let caches = fixture.appendingPathComponent("Library/Caches")
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        let file = caches.appendingPathComponent("not-a-directory")
        try Data().write(to: file)
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(file.path, home: fixture.path))
    }

    func testTraversalCannotHideASymlinkBeforeBoundaryValidation() throws {
        let fixture = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-visual-traversal-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: fixture) }
        let caches = fixture.appendingPathComponent("Library/Caches")
        let outside = fixture.appendingPathComponent("outside/nested")
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        let link = caches.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: outside)
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            caches.path + "/link/../escaped-review", home: fixture.path
        ), "Normalizing '..' first must not hide an escaping symlink in the original path")
    }

    func testAFileAncestorCannotBecomeAnIsolatedDirectory() throws {
        let fixture = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-visual-ancestor-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: fixture) }
        let caches = fixture.appendingPathComponent("Library/Caches")
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        let file = caches.appendingPathComponent("file")
        try Data().write(to: file)
        XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(
            file.appendingPathComponent("review-child").path, home: fixture.path
        ))
    }

    @MainActor
    func testInactiveVisualLifecycleIsInertWithoutAWindow() {
        let lifecycle = NativeVisualReviewWindowLifecycle(environment: [:])
        XCTAssertNil(lifecycle.windowIdentifier)
        XCTAssertEqual(lifecycle.windowTitle, "")
        XCTAssertFalse(lifecycle.isWindowCreated)
        lifecycle.showReviewWindow()
        XCTAssertFalse(lifecycle.isWindowCreated)
    }

    @MainActor
    func testValidVisualLifecycleConstructsOnlyItsPolicyUntilLaunch() {
        let lifecycle = NativeVisualReviewWindowLifecycle(environment: reviewEnvironment)
        XCTAssertEqual(lifecycle.windowIdentifier?.rawValue, "native-visual-review")
        XCTAssertEqual(lifecycle.windowTitle, "原生视觉基础审阅")
        XCTAssertFalse(lifecycle.isWindowCreated, "Unit test never opens an actual review window")
    }

    private var reviewEnvironment: [String: String] {
        [
            NativeVisualReviewEntry.flagKey: "1",
            NativeVisualReviewEntry.environmentKey: "ui-test",
            NativeVisualReviewEntry.dataDirectoryKey:
                NSHomeDirectory() + "/Library/Caches/aeroric-native/stage01-unit-\(UUID().uuidString)",
        ]
    }
    #endif

    private func assertConserved(
        _ budget: NativePanelGeometry.Budget, file: StaticString = #filePath, line: UInt = #line
    ) {
        XCTAssertEqual(
            budget.sidebarWidth + budget.workspaceWidth + budget.inspectorWidth
                + CGFloat(budget.gaps) * NativePanelGeometry.panelGap
                + 2 * NativePanelGeometry.panelInset + 2 * NativePanelGeometry.outlineInset,
            budget.totalWidth, file: file, line: line
        )
    }
}
