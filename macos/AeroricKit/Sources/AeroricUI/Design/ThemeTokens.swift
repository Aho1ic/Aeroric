import SwiftUI

// Design tokens for the native macOS client (Task P4-03).
//
// Number layers (spacing / radii / typography / motion) mirror
// `packages/design-system/src/tokens.ts` — that file is the authoring
// source; `ThemePolicyTests` pins the numbers so the two layers cannot
// drift silently. The colour layer never copies RGB values from a
// screenshot: every role resolves through a dynamic system colour
// (spec hard rule "colours come from semantic roles"), so light/dark,
// increased contrast and the user's accent colour are all handled by
// AppKit/SwiftUI instead of per-theme constants here.

// MARK: - Number tokens (mirror of design-system tokens.ts)

public enum DSSpacing {
    public static let values: [CGFloat] = [0, 4, 8, 12, 16, 20, 24]
    public static func v(_ step: Int) -> CGFloat {
        precondition(values.indices.contains(step), "DSSpacing step \(step) out of range")
        return values[step]
    }
}

public enum DSRadius {
    /// tokens.ts `radii.sm`
    public static let sm: CGFloat = 8
    /// tokens.ts `radii.md`
    public static let md: CGFloat = 10
    /// tokens.ts `radii.lg`
    public static let lg: CGFloat = 14
    /// tokens.ts `radii.xl`
    public static let xl: CGFloat = 18
    /// tokens.ts `radii.pill` — full capsule.
    public static let pill: CGFloat = 999
}

public enum DSTypography {
    /// tokens.ts `typography.label`
    public static let label: CGFloat = 11
    /// tokens.ts `typography.meta`
    public static let meta: CGFloat = 12
    /// tokens.ts `typography.body`
    public static let body: CGFloat = 14
    /// tokens.ts `typography.title`
    public static let title: CGFloat = 18
}

/// Typography used by the approved main shell. The bundled JetBrains Mono
/// face is registered by the app target; AppKit/SwiftUI still supplies glyph
/// fallback for Chinese and system symbols. Keeping the constructor here
/// avoids a second font vocabulary in shell, inspector and preview views.
public enum AeroricFont {
    public static let file: CGFloat = 12
    public static let project: CGFloat = 12
    public static let body: CGFloat = 13
    public static let code: CGFloat = 12
    public static let meta: CGFloat = 11
    public static let title: CGFloat = 16

    public static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("JetBrainsMono-Regular", size: size, relativeTo: .body).weight(weight)
    }

    /// Redesign 01: interface text uses the system face (SF + PingFang).
    /// JetBrains Mono (`mono`) stays for code, diffs and the terminal only.
    public static func ui(_ size: CGFloat = body, weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight)
    }
}

public enum DSMotion {
    /// tokens.ts `motion.fast`
    public static let fast: TimeInterval = 0.120
    /// tokens.ts `motion.normal`
    public static let normal: TimeInterval = 0.180
    /// tokens.ts `motion.slow`
    public static let slow: TimeInterval = 0.260
}

// MARK: - Colour roles (forward to AeroricColor)

// Overhaul M1: the `ds*` names forward to `AeroricColor` so the two older
// palettes stop disagreeing. Every role is still a dynamic colour resolved per
// appearance (light, dark, increased contrast).
public extension Color {
    static let dsCanvas = AeroricColor.canvas
    static let dsSurface = AeroricColor.layer1
    /// Opaque text background for terminal and editor panes.
    static let dsTextBackground = AeroricColor.canvas
    static let dsBorder = AeroricColor.separator2
    static let dsText = AeroricColor.text
    static let dsTextSecondary = AeroricColor.textSecondary
    static let dsTextMuted = AeroricColor.textTertiary
    static let dsAccent = AeroricColor.accent
    static let dsSuccess = AeroricColor.success
    static let dsWarning = AeroricColor.warning
    static let dsDanger = AeroricColor.danger
    static let dsFocus = AeroricColor.accent
    static let dsOnAccent = Color.white
}

// MARK: - Motion policy (reduced motion honoured)

public extension DSMotion {
    /// Duration to use for a transition, honouring the system's
    /// accessibility "reduce motion" setting: reduced motion collapses
    /// the animation to a zero-length change instead of a slower one.
    static func duration(_ base: TimeInterval, reduceMotion: Bool) -> TimeInterval {
        reduceMotion ? 0 : base
    }
}
