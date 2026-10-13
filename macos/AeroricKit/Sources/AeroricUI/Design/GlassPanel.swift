import SwiftUI

// Glass / classic content surfaces (Task P4-03 Action 3).
//
// `glass/classic` is an appearance preference and nothing else — it is
// separate from any interaction mode (AGENTS.md native selection rule;
// ADR-001 §4). The system "reduce transparency" accessibility setting
// always wins over the user's preference: readability beats ambience.

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
}

/// A panel shell that resolves to either a SwiftUI Material (glass) or an
/// opaque system surface (classic / reduce transparency).
///
/// Text-dense panes (terminal, editor) must NOT stack translucent layers
/// under every row: those panes sit on `Color.dsTextBackground`, which is
/// opaque by construction. GlassPanel is for chrome-level shells —
/// sidebars, inspector panels, floating cards.
public struct GlassPanel<Content: View>: View {
    private let choice: SurfacePreference
    private let content: Content

    public init(preference: SurfacePreference,
                reduceTransparency: Bool = false,
                @ViewBuilder content: () -> Content) {
        self.choice = SurfaceChoice.resolve(preference: preference,
                                            reduceTransparency: reduceTransparency)
        self.content = content()
    }

    /// Build from a resolved choice directly (tests / previews).
    init(resolved: SurfacePreference, @ViewBuilder content: () -> Content) {
        self.choice = resolved
        self.content = content()
    }

    public var body: some View {
        Group {
            switch choice {
            case .glass:
                if #available(macOS 26.0, *) {
                    content
                        .glassEffect(.regular, in: RoundedRectangle(cornerRadius: DSRadius.lg, style: .continuous))
                } else {
                    content
                        .background(.thinMaterial, ignoresSafeAreaEdges: [])
                }
            case .classic:
                content
                    .background(Color.dsSurface, ignoresSafeAreaEdges: [])
            }
        }
        // Hairline edge keeps glass panels legible against arbitrary
        // window content in both light and dark appearances.
        .overlay {
            RoundedRectangle(cornerRadius: DSRadius.lg, style: .continuous)
                .strokeBorder(Color.dsBorder.opacity(0.6), lineWidth: 0.5)
        }
        .clipShape(RoundedRectangle(cornerRadius: DSRadius.lg, style: .continuous))
    }
}
