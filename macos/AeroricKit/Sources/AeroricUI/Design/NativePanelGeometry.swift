import SwiftUI

// Shared panel geometry for the native redesign (stage 01).
//
// Numbers only — no view code, no new design system. The shape values are
// the approved stage-01 scope (`docs/native/frontend-redesign-stage-01.md`
// §2 + review-round-07): an 18pt continuous panel radius, an 8pt inset and
// an 8pt gap between panels, on the existing 4pt token scale. Radii come
// from `DSRadius`, spacing from `DSSpacing`; nothing here redefines either.
//
// The width budgets are REVIEW SAMPLES. They reproduce the measured browser
// fixtures so the shared geometry can be checked against them:
//
//   1260 → 320 / 586 / 320      (regular, both side columns visible)
//   1000 → 284 / 409 / 273      (compact sample)
//    400 → centre 382           (minimum sample, no side columns)
//
// They are **not** an approved compression floor, resize range or
// persistence algorithm, and they say nothing about whether the real
// workspace surfaces support a 400pt window.

public enum NativePanelGeometry {

    /// The main product shell variant. It is intentionally separate from the
    /// legacy review geometry below: older review surfaces and their unit
    /// tests keep the 18/8/8 values, while the approved shell uses the
    /// tighter 25/7/6 composition.
    public struct MainShell: Equatable, Sendable {
        public let panelRadius: CGFloat = 0
        public let horizontalInset: CGFloat = 0
        public let bottomInset: CGFloat = 0
        public let columnGap: CGFloat = 0
        /// Redesign 01 (DSH `columns.ts`): 280 default, 264–420 drag range.
        public let defaultSidebarWidth: CGFloat = 280
        public let minimumSidebarWidth: CGFloat = 264
        public let maximumSidebarWidth: CGFloat = 420
        /// Below this window width the sidebar falls back to the icon rail.
        public let sidebarAutoCollapseWidth: CGFloat = 1024
        /// Inspector: never narrower than 300; first opening takes 0.45.
        public let minimumInspectorWidth: CGFloat = 300
        public let initialInspectorFraction: CGFloat = 0.45
        public let maximumInspectorFraction: CGFloat = 0.7
        /// The centre is protected: the inspector shrinks, then hides.
        public let minimumCentreWidth: CGFloat = 400
        public let minimumWindowWidth: CGFloat = 860
        public let minimumWindowHeight: CGFloat = 560
        public let defaultWindowWidth: CGFloat = 1280
        public let defaultWindowHeight: CGFloat = 820
        public let projectRowHeight: CGFloat = 28
        public let resourceRowHeight: CGFloat = 24
        public let statusBarHeight: CGFloat = 24
        public let terminalBarHeight: CGFloat = 32
        public let terminalInitialFraction: CGFloat = 0.28

        public init() {}
    }

    public static let mainShell = MainShell()

    /// Window-level budget for the three-column shell. Automatic compression
    /// never mutates the user's stored drag preferences.
    public struct MainShellBudget: Equatable, Sendable {
        public let totalWidth: CGFloat
        public let sidebarWidth: CGFloat
        public let workspaceWidth: CGFloat
        public let inspectorWidth: CGFloat
        public let gap: CGFloat
        /// True when the rail is shown because the window is narrow, not
        /// because the user collapsed it.
        public let sidebarAutoCollapsed: Bool

        public var contentWidth: CGFloat {
            workspaceWidth + (inspectorWidth > 0 ? gap + inspectorWidth : 0)
        }
    }

    public static func mainShellBudget(
        totalWidth: CGFloat,
        preferredSidebarWidth: CGFloat,
        inspectorVisible: Bool = true,
        sidebarCollapsed: Bool = false,
        preferredInspectorWidth: CGFloat? = nil
    ) -> MainShellBudget {
        let shell = mainShell
        let auto = !sidebarCollapsed && totalWidth < shell.sidebarAutoCollapseWidth
        let sidebar = (sidebarCollapsed || auto) ? ProjectSidebar.collapsedWidth : min(
            shell.maximumSidebarWidth, max(shell.minimumSidebarWidth, preferredSidebarWidth)
        )
        var inspector: CGFloat = 0
        if inspectorVisible {
            let wanted = preferredInspectorWidth ?? (totalWidth * shell.initialInspectorFraction).rounded()
            let ceiling = totalWidth * shell.maximumInspectorFraction
            let available = totalWidth - sidebar - shell.minimumCentreWidth
            let target = min(ceiling, max(shell.minimumInspectorWidth, wanted))
            let fitted = min(target, available)
            inspector = fitted >= shell.minimumInspectorWidth ? fitted.rounded(.down) : 0
        }
        let workspace = max(0, totalWidth - sidebar - inspector)
        return MainShellBudget(
            totalWidth: totalWidth, sidebarWidth: sidebar, workspaceWidth: workspace,
            inspectorWidth: inspector, gap: shell.columnGap, sidebarAutoCollapsed: auto
        )
    }

    // MARK: - Shape and chrome numbers

    /// Panel corner radius: the existing `xl` role (18), continuous.
    public static let panelRadius: CGFloat = DSRadius.xl
    /// Inset between the window edge and the panel band, and the panel's
    /// own content padding (`DSSpacing` step 2 = 8).
    public static let panelInset: CGFloat = DSSpacing.v(2)
    /// Gap between adjacent panels (`DSSpacing` step 2 = 8).
    public static let panelGap: CGFloat = DSSpacing.v(2)
    /// The review fixture draws a 1pt window outline; the sample's inner
    /// width is the total width minus this on both sides (1260 → 1258).
    /// The real window has no such outline — it is a fixture budget only.
    public static let outlineInset: CGFloat = 1
    /// Navigation row height (frozen stage-01 density decision).
    public static let navigationRowHeight: CGFloat = 32
    /// Bottom status bar height (frozen stage-01 decision).
    public static let statusBarHeight: CGFloat = 32
    /// Bare icon button hit target (v10 measured 28×28).
    public static let iconButtonSize: CGFloat = 28
    /// Hover feedback fill for a bare icon button. Not a permanent tray:
    /// it exists only while the pointer is over an enabled button.
    public static let iconHoverFillOpacity: Double = 0.08
    /// Disabled buttons keep their hit geometry and dim the glyph.
    public static let disabledOpacity: Double = 0.35
    /// Pressed feedback for a bare icon button.
    public static let pressedOpacity: Double = 0.6

    /// Weak semantic edge around a panel. The v10 refinement replaced the
    /// touching full-strength outlines with this subdued one so the seams
    /// between panels do not read as isolated high-contrast pockets.
    public struct Outline: Equatable, Sendable {
        public let opacity: Double
        public let width: CGFloat
    }

    public static let standardOutline = Outline(opacity: 0.6, width: 0.5)
    /// Increased contrast strengthens the necessary boundary instead of
    /// changing colours (spec: "增强对比提高必要边界").
    public static let increasedContrastOutline = Outline(opacity: 1.0, width: 1.0)

    public static func outline(increasedContrast: Bool) -> Outline {
        increasedContrast ? increasedContrastOutline : standardOutline
    }

    /// Ordinary window budget: no browser-fixture outline deduction.
    /// The existing 1000pt minimum and 180...360pt sidebar range keep this
    /// positive; callers must not use a clamped zero to claim narrow support.
    public static func workspaceWidth(totalWidth: CGFloat, sidebarWidth: CGFloat) -> CGFloat {
        totalWidth - 2 * panelInset - sidebarWidth - panelGap
    }

    // MARK: - Width budget

    /// Column widths for one total width. Pure arithmetic so the budget is
    /// unit-testable without instantiating a view.
    ///
    /// `inner = total − 2 × outlineInset`;
    /// `workspace = inner − 2 × panelInset − Σ visible side columns − gaps × panelGap`
    /// where `gaps` is one per pair of adjacent panels (2 with the inspector
    /// open, 1 without it).
    public struct Budget: Equatable, Sendable {
        public let totalWidth: CGFloat
        public let innerWidth: CGFloat
        public let sidebarWidth: CGFloat
        public let workspaceWidth: CGFloat
        public let inspectorWidth: CGFloat
        public let inspectorVisible: Bool
        public let gaps: Int

        /// Readable form for the review window's AX leaf, e.g.
        /// `1260 → 320/586/320`. Zero-width columns are omitted.
        public var summary: String {
            var parts: [String] = []
            if sidebarWidth > 0 { parts.append(Self.format(sidebarWidth)) }
            parts.append(Self.format(workspaceWidth))
            if inspectorVisible, inspectorWidth > 0 { parts.append(Self.format(inspectorWidth)) }
            return "\(Self.format(totalWidth)) → \(parts.joined(separator: "/"))"
        }

        static func format(_ value: CGFloat) -> String {
            String(Int(value.rounded()))
        }
    }

    public static func budget(totalWidth: CGFloat,
                              sidebarWidth: CGFloat,
                              inspectorWidth: CGFloat,
                              inspectorVisible: Bool) -> Budget {
        let inner = max(0, totalWidth - 2 * outlineInset)
        let sidebar = max(0, sidebarWidth)
        let inspector = inspectorVisible ? max(0, inspectorWidth) : 0
        // The centre always exists. Only an actually visible neighbour
        // adds a gap; the 400pt centre-only sample has no gaps at all.
        let gaps = (sidebar > 0 ? 1 : 0) + (inspector > 0 ? 1 : 0)
        let sideColumns = sidebar + inspector
        let workspace = max(0, inner - 2 * panelInset - sideColumns - CGFloat(gaps) * panelGap)
        return Budget(
            totalWidth: totalWidth,
            innerWidth: inner,
            sidebarWidth: sidebar,
            workspaceWidth: workspace,
            inspectorWidth: inspector,
            inspectorVisible: inspectorVisible,
            gaps: gaps
        )
    }

    #if DEBUG
    // MARK: - Review samples (never shipped in ReleaseLocal)

    /// One review fixture size. These are specimens for the shared geometry,
    /// not an approved window range: see the file header.
    public struct ReviewSample: Equatable, Sendable {
        public let id: String
        public let title: String
        public let totalWidth: CGFloat
        public let totalHeight: CGFloat
        public let sidebarWidth: CGFloat
        public let inspectorWidth: CGFloat
        public let inspectorVisible: Bool

        public var budget: Budget {
            NativePanelGeometry.budget(
                totalWidth: totalWidth,
                sidebarWidth: sidebarWidth,
                inspectorWidth: inspectorWidth,
                inspectorVisible: inspectorVisible
            )
        }
    }

    public static let reviewSamples: [ReviewSample] = [
        ReviewSample(id: "regular", title: "1260×860", totalWidth: 1260, totalHeight: 860,
                     sidebarWidth: 320, inspectorWidth: 320, inspectorVisible: true),
        ReviewSample(id: "compact", title: "1000×640", totalWidth: 1000, totalHeight: 640,
                     sidebarWidth: 284, inspectorWidth: 273, inspectorVisible: true),
        ReviewSample(id: "minimum", title: "400×600", totalWidth: 400, totalHeight: 600,
                     sidebarWidth: 0, inspectorWidth: 0, inspectorVisible: false),
    ]

    public static let defaultReviewSample = reviewSamples[0]

    public static func reviewSample(id: String) -> ReviewSample {
        reviewSamples.first { $0.id == id } ?? defaultReviewSample
    }
    #endif
}

/// The root is the only window-width owner. Ready content consumes this
/// budget for its centre + inspector envelope; it must not measure a second
/// three-column layout or force the centre alone through a 600pt floor.
private struct MainShellBudgetKey: EnvironmentKey {
    static let defaultValue: NativePanelGeometry.MainShellBudget? = nil
}

private struct MainShellNavigationEpochKey: EnvironmentKey {
    static let defaultValue: UInt64 = 0
}

public extension EnvironmentValues {
    var mainShellBudget: NativePanelGeometry.MainShellBudget? {
        get { self[MainShellBudgetKey.self] }
        set { self[MainShellBudgetKey.self] = newValue }
    }
    var mainShellNavigationEpoch: UInt64 {
        get { self[MainShellNavigationEpochKey.self] }
        set { self[MainShellNavigationEpochKey.self] = newValue }
    }
}
