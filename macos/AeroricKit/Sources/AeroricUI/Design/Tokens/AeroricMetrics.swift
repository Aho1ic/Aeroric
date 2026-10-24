import SwiftUI

/// Spacing on a 4pt grid (2 and 6 kept for hairline-tight chrome).
public enum AeroricSpace {
    public static let xxs: CGFloat = 2
    public static let xs: CGFloat = 4
    public static let s: CGFloat = 6
    public static let m: CGFloat = 8
    public static let l: CGFloat = 12
    public static let xl: CGFloat = 16
    public static let xxl: CGFloat = 20
    public static let xxxl: CGFloat = 24
    public static let huge: CGFloat = 32
}

/// Corner radii. Always drawn `.continuous`; single-line controls use `Capsule()`.
public enum AeroricRadius {
    public static let xs: CGFloat = 4
    public static let s: CGFloat = 8
    public static let m: CGFloat = 12
    public static let l: CGFloat = 16
    public static let xl: CGFloat = 20
    /// Floating composer and hero cards.
    public static let panel: CGFloat = 28

    public static func shape(_ radius: CGFloat) -> RoundedRectangle {
        RoundedRectangle(cornerRadius: radius, style: .continuous)
    }
}

/// Type scale. Interface text is the system face (SF + PingFang); code and the
/// terminal use JetBrains Mono through `AeroricType.mono`.
public enum AeroricType {
    /// Body-level sizes follow the text-size setting (`AeroricTypeScale`):
    /// secondary = body − 1, caption = body − 2. Headings below are fixed.
    public static var caption: CGFloat { body - 2 }
    public static var secondary: CGFloat { body - 1 }
    public static var body: CGFloat { AeroricTypeScale.shared.textSize }
    /// Code, diffs and inline monospaced text.
    public static var code: CGFloat { AeroricTypeScale.shared.codeSize }
    public static let headline: CGFloat = 15
    public static let title: CGFloat = 17
    public static let largeTitle: CGFloat = 22
    public static let hero: CGFloat = 26

    /// Monospaced face for code, diffs and the terminal: bundled JetBrains Mono
    /// (registered by the app target), falling back to the system mono.
    public static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Font.custom("JetBrainsMono-Regular", size: size, relativeTo: .body).weight(weight)
    }

    /// 11pt uppercase-style section labels (orca/codeg sidebar groups).
    public static func sectionLabel() -> Font { .system(size: caption, weight: .semibold) }
}

/// Motion durations; every call site passes `reduceMotion` so the setting
/// actually changes the output.
public enum AeroricMotion {
    public static let quick: TimeInterval = 0.10
    public static let standard: TimeInterval = 0.20
    public static let slow: TimeInterval = 0.30

    /// Zero when Reduce Motion is on, so transactions built from it are instant.
    public static func duration(_ base: TimeInterval, reduceMotion: Bool) -> TimeInterval {
        reduceMotion ? 0 : base
    }

    public static func animation(_ duration: TimeInterval, reduceMotion: Bool) -> Animation? {
        reduceMotion ? nil : .easeInOut(duration: duration)
    }
}

/// Three elevation levels: hairline → soft → floating. Dark mode relies on
/// the hairline; the soft glow is near-invisible there by design.
public enum AeroricElevation: Sendable {
    case hairline
    case soft
    case floating
}

public extension View {
    func aeroricElevation(_ level: AeroricElevation, in shape: some InsettableShape) -> some View {
        modifier(ElevationModifier(level: level, shape: shape))
    }
}

private struct ElevationModifier<S: InsettableShape>: ViewModifier {
    let level: AeroricElevation
    let shape: S
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        let edge = contrast == .increased ? AeroricColor.separator4 : AeroricColor.separator2
        let width: CGFloat = contrast == .increased ? 1 : 0.5
        switch level {
        case .hairline:
            content.overlay(shape.strokeBorder(edge, lineWidth: width).allowsHitTesting(false))
        case .soft:
            content
                .overlay(shape.strokeBorder(edge, lineWidth: width).allowsHitTesting(false))
                .shadow(color: AeroricColor.shadow.opacity(0.35), radius: 2, y: 1)
                .shadow(color: AeroricColor.shadow.opacity(0.25), radius: 12, y: 4)
        case .floating:
            content
                .overlay(shape.strokeBorder(edge, lineWidth: width).allowsHitTesting(false))
                .shadow(color: AeroricColor.shadow, radius: 24, y: 10)
        }
    }
}
