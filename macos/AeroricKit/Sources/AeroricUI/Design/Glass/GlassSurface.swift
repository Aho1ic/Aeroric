import SwiftUI

/// Where a glass surface sits. The role picks the tint and the fallback
/// material; reading content (conversation, code, diff, terminal) never uses
/// glass and has no role here.
public enum GlassRole: Sendable, CaseIterable {
    case sidebar
    case toolbar
    case floatingCard
    case popover
    case tabStrip
}

/// How a glass surface is actually drawn on this system, for this user.
public enum GlassRendering: Equatable, Sendable {
    /// macOS 26+ Liquid Glass (`glassEffect`).
    case liquidGlass
    /// System material (`NSVisualEffectView` / SwiftUI `Material`), the
    /// macOS 14–25 path and the DEBUG legacy preview.
    case material
    /// Opaque fill: Reduce Transparency, or the user's classic preference.
    case opaque
}

public enum GlassSurfacePolicy {
    /// Reduce Transparency always wins, then the user's classic preference,
    /// then the OS capability (or the DEBUG preview that pretends it is absent).
    public static func rendering(preference: SurfacePreference, reduceTransparency: Bool,
                                 liquidGlassAvailable: Bool, legacyPreview: Bool) -> GlassRendering {
        if SurfaceChoice.resolve(preference: preference, reduceTransparency: reduceTransparency) == .classic {
            return .opaque
        }
        return liquidGlassAvailable && !legacyPreview ? .liquidGlass : .material
    }

    public static var liquidGlassAvailable: Bool {
        if #available(macOS 26, *) { return true }
        return false
    }
}

/// DEBUG-only "preview the older-system look" switch, so the macOS 14–25
/// fallback can be reviewed on a newer Mac. Release builds never read it.
public enum LegacySurfacePreview {
    public static let environmentVariable = "AERORIC_PREVIEW_LEGACY_SURFACE"
    public static let storageKey = "debug.previewLegacySurface"

    public static var isAvailable: Bool {
        #if DEBUG
        true
        #else
        false
        #endif
    }

    /// Launch-time default: the environment variable, DEBUG only.
    public static var launchEnabled: Bool {
        isAvailable && ProcessInfo.processInfo.environment[environmentVariable] == "1"
    }
}

private struct LegacySurfacePreviewKey: EnvironmentKey {
    static let defaultValue = LegacySurfacePreview.launchEnabled
}

public extension EnvironmentValues {
    var legacySurfacePreview: Bool {
        get { self[LegacySurfacePreviewKey.self] }
        set { self[LegacySurfacePreviewKey.self] = newValue }
    }
}

public extension View {
    /// Draw this view's background as a glass surface for `role`.
    func glassSurface(_ role: GlassRole, in shape: some InsettableShape = Rectangle()) -> some View {
        modifier(GlassSurfaceModifier(role: role, shape: shape))
    }
}

struct GlassSurfaceModifier<S: InsettableShape>: ViewModifier {
    let role: GlassRole
    let shape: S
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.legacySurfacePreview) private var legacyPreview
    @Environment(\.colorSchemeContrast) private var contrast
    @AppStorage("settings.surfacePreference") private var surfaceRawValue = SurfacePreference.glass.rawValue

    private var rendering: GlassRendering {
        GlassSurfacePolicy.rendering(
            preference: SurfacePreference(rawValue: surfaceRawValue) ?? .glass,
            reduceTransparency: reduceTransparency,
            liquidGlassAvailable: GlassSurfacePolicy.liquidGlassAvailable,
            legacyPreview: legacyPreview
        )
    }

    func body(content: Content) -> some View {
        Group {
            switch rendering {
            case .opaque:
                content.background(opaqueFill, in: shape)
            case .material:
                content.background { materialBackground }
            case .liquidGlass:
                if #available(macOS 26, *) {
                    content.glassEffect(.regular.tint(tint), in: shape)
                } else {
                    content.background { materialBackground }
                }
            }
        }
        .overlay {
            if drawsEdge {
                shape.strokeBorder(contrast == .increased ? AeroricColor.separator4 : AeroricColor.separator2,
                                   lineWidth: contrast == .increased ? 1 : 0.5)
                    .allowsHitTesting(false)
            }
        }
    }

    /// Full-height columns are separated by the layout's own hairline.
    private var drawsEdge: Bool { role != .sidebar && role != .toolbar }

    @ViewBuilder private var materialBackground: some View {
        switch role {
        case .sidebar:
            ZStack {
                NativeMaterial.sidebar
                AeroricColor.sidebar.opacity(0.4)
            }
        case .toolbar, .tabStrip:
            shape.fill(.bar).overlay(shape.fill(tint))
        case .floatingCard:
            shape.fill(.regularMaterial).overlay(shape.fill(tint))
        case .popover:
            shape.fill(.thickMaterial).overlay(shape.fill(tint))
        }
    }

    private var tint: Color {
        switch role {
        case .sidebar: AeroricColor.sidebar.opacity(0.4)
        case .toolbar, .tabStrip: AeroricColor.canvas.opacity(0.2)
        case .floatingCard: AeroricColor.layer1.opacity(0.35)
        case .popover: AeroricColor.layer1.opacity(0.5)
        }
    }

    private var opaqueFill: Color {
        switch role {
        case .sidebar: AeroricColor.sidebar
        case .toolbar, .tabStrip: AeroricColor.canvas
        case .floatingCard, .popover: AeroricColor.layer1
        }
    }
}
