import SwiftUI

// Glass / classic content surfaces (Task P4-03 Action 3; stage-01 revision).
//
// `glass/classic` is an appearance preference and nothing else — it is
// separate from any interaction mode (AGENTS.md native selection rule;
// ADR-001 §4). The system "reduce transparency" accessibility setting
// always wins over the user's preference: readability beats ambience.
//
// Stage-01 change (e3836e62:docs/native/frontend-redesign-stage-01.md §3): the panel
// no longer wraps itself in `glassEffect`. Liquid Glass is for interactive
// controls and navigation chrome, not for a whole content panel — the panel
// uses a real SwiftUI `Material` (ADR-001 §3 "content panel" row) and keeps
// the 18pt continuous shape from `NativePanelGeometry`. The previous
// macOS-26-only `glassEffect(.regular, in: RoundedRectangle)` branch is
// removed, so a panel renders the same way on every supported system.

/// Appearance preference for panel shells. Independent from
/// `interactionMode`; only this enum may choose materials.
public enum SurfacePreference: String, Sendable, CaseIterable {
    case glass
    case classic
}

/// Resolved material choice for one panel. Pure function so the policy is
/// unit-testable without instantiating SwiftUI views.
public enum SurfaceChoice {
    /// Reduce transparency always forces the classic (opaque) surface.
    public static func resolve(preference: SurfacePreference,
                               reduceTransparency: Bool) -> SurfacePreference {
        reduceTransparency ? .classic : preference
    }

    public static func resolve(preference: SurfacePreference,
                               reduceTransparency: Bool,
                               reduceTransparencyOverride: Bool?) -> SurfacePreference {
        // Compatibility/test arguments may strengthen accessibility, never
        // disable a real system preference.
        resolve(preference: preference,
                reduceTransparency: reduceTransparency || reduceTransparencyOverride == true)
    }
}

/// One continuous base behind panels AND their gutters. Keep it separate
/// from panel outlines: no patching individual corner pockets, and no
/// whole-window Liquid Glass. Classic and Reduce Transparency are opaque.
public struct NativePanelFoundation: View {
    private let preference: SurfacePreference
    private let reduceTransparency: Bool?
    @Environment(\.accessibilityReduceTransparency) private var systemReduceTransparency

    public init(preference: SurfacePreference, reduceTransparency: Bool? = nil) {
        self.preference = preference
        self.reduceTransparency = reduceTransparency
    }

    public var body: some View {
        switch SurfaceChoice.resolve(
            preference: preference,
            reduceTransparency: systemReduceTransparency,
            reduceTransparencyOverride: reduceTransparency
        ) {
        case .glass:
            NativeMaterial.underWindowBackground
        case .classic:
            AeroricColor.canvas
        }
    }
}

/// A panel shell that resolves to either a SwiftUI Material (glass) or an
/// opaque system surface (classic / reduce transparency).
///
/// Text-dense panes (terminal, editor) must NOT stack translucent layers
/// under every row: those panes sit on `AeroricColor.layer1`, which is
/// opaque by construction. GlassPanel is for chrome-level shells —
/// sidebars, inspector panels, floating cards.
///
/// The view reads the system preferences itself (`accessibilityReduceTransparency`,
/// `colorSchemeContrast`) rather than trusting a caller to pass them in, so
/// the accessibility behaviour cannot be lost by a call site that forgets.
/// The `reduceTransparency` parameter stays for source compatibility and for
/// tests/previews that enable the fallback; false never overrides the system.
public struct GlassPanel<Content: View>: View {
    private enum Source {
        /// Resolve at render time from the live environment.
        case preference(SurfacePreference, transparencyOverride: Bool?, contrastOverride: Bool?)
        /// Already decided by the caller (tests / previews).
        case resolved(SurfacePreference)
    }

    private let source: Source
    private let content: Content
    private let cornerRadius: CGFloat

    @Environment(\.accessibilityReduceTransparency) private var systemReduceTransparency
    @Environment(\.colorSchemeContrast) private var colorSchemeContrast

    /// - Parameters:
    ///   - preference: the user's `glass/classic` appearance preference.
    ///   - reduceTransparency: `nil` (the default) reads the system
    ///     accessibility setting; true can demonstrate the fallback in a
    ///     test/preview. False cannot defeat the real system preference.
    ///   - increaseContrast: same contract as `reduceTransparency` — `nil`
    ///     reads `\.colorSchemeContrast`. macOS has no per-app override for
    ///     either accessibility preference, so a sample that must show the
    ///     effect without changing the user's global setting pins it here.
    public init(preference: SurfacePreference,
                reduceTransparency: Bool? = nil,
                increaseContrast: Bool? = nil,
                cornerRadius: CGFloat = NativePanelGeometry.panelRadius,
                @ViewBuilder content: () -> Content) {
        self.source = .preference(preference,
                                  transparencyOverride: reduceTransparency,
                                  contrastOverride: increaseContrast)
        self.content = content()
        self.cornerRadius = cornerRadius
    }

    /// Build from a resolved choice directly (tests / previews).
    init(resolved: SurfacePreference,
         cornerRadius: CGFloat = NativePanelGeometry.panelRadius,
         @ViewBuilder content: () -> Content) {
        self.source = .resolved(resolved)
        self.content = content()
        self.cornerRadius = cornerRadius
    }

    private var resolvedChoice: SurfacePreference {
        switch source {
        case let .preference(preference, transparencyOverride, _):
            return SurfaceChoice.resolve(
                preference: preference,
                reduceTransparency: systemReduceTransparency,
                reduceTransparencyOverride: transparencyOverride
            )
        case let .resolved(choice):
            return SurfaceChoice.resolve(preference: choice,
                                         reduceTransparency: systemReduceTransparency)
        }
    }

    private var outline: NativePanelGeometry.Outline {
        let increased: Bool
        switch source {
        case let .preference(_, _, contrastOverride):
            increased = contrastOverride == true || colorSchemeContrast == .increased
        case .resolved:
            increased = colorSchemeContrast == .increased
        }
        return NativePanelGeometry.outline(increasedContrast: increased)
    }

    public var body: some View {
        let shape = RoundedRectangle(
            cornerRadius: cornerRadius, style: .continuous
        )
        Group {
            switch resolvedChoice {
            case .glass:
                content
                    .background(.thinMaterial, in: shape)
            case .classic:
                content
                    .background(AeroricColor.canvas, in: shape)
            }
        }
        // Hairline edge keeps panels legible against arbitrary window
        // content in both light and dark appearances. Subdued by default so
        // the seam between two panels does not read as a hard pocket;
        // increased contrast strengthens it.
        .overlay {
            shape
                .strokeBorder(AeroricColor.separator2.opacity(outline.opacity), lineWidth: outline.width)
                .allowsHitTesting(false)
        }
        // Shape the background, not child text, focus rings, scrollbars or
        // IME candidates. Callers retain their content's safe inner padding.
    }
}
