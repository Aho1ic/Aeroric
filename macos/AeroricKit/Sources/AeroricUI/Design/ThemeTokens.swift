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

public enum DSMotion {
    /// tokens.ts `motion.fast`
    public static let fast: TimeInterval = 0.120
    /// tokens.ts `motion.normal`
    public static let normal: TimeInterval = 0.180
    /// tokens.ts `motion.slow`
    public static let slow: TimeInterval = 0.260
}

// MARK: - Colour roles (dynamic system colours only)

public extension Color {
    /// Window / canvas base (`windowBackgroundColor`).
    static let dsCanvas = Color(nsColor: .windowBackgroundColor)
    /// Elevated control surface (`controlBackgroundColor`).
    static let dsSurface = Color(nsColor: .controlBackgroundColor)
    /// Readable text background for terminal & editor panes
    /// (`textBackgroundColor`) — deliberately opaque, never a stacked
    /// translucent layer under every text row.
    static let dsTextBackground = Color(nsColor: .textBackgroundColor)
    /// Hairline separators (`separatorColor`).
    static let dsBorder = Color(nsColor: .separatorColor)
    /// Primary text (`labelColor`).
    static let dsText = Color(nsColor: .labelColor)
    /// Secondary text (`secondaryLabelColor`).
    static let dsTextSecondary = Color(nsColor: .secondaryLabelColor)
    /// Muted text (`tertiaryLabelColor`).
    static let dsTextMuted = Color(nsColor: .tertiaryLabelColor)
    /// Accent (`Color.accentColor` — follows the system accent preference).
    static let dsAccent = Color.accentColor
    /// Positive state (`systemGreen`).
    static let dsSuccess = Color(nsColor: .systemGreen)
    /// Warning state (`systemOrange`).
    static let dsWarning = Color(nsColor: .systemOrange)
    /// Destructive state (`systemRed`).
    static let dsDanger = Color(nsColor: .systemRed)
    /// Focus ring (`controlAccentColor` — tracks the accent beyond Color.accentColor).
    static let dsFocus = Color(nsColor: .controlAccentColor)
    /// Text on top of accent surfaces (`controlAccentColor`'s readable pair
    /// is produced by the system for labels on accent backgrounds).
    static let dsOnAccent = Color(nsColor: .alternateSelectedControlTextColor)
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
