import SwiftUI
import AppKit

/// Redesign 01 colour roles, taken from DeepSeek Harness
/// (`packages/client/ui-theme/src/styles/design-platform.css`, neutral-bluish
/// scale; light `:root`, dark `[data-theme=dark]`). Each role is a dynamic
/// NSColor, so light, dark and Increase Contrast resolve through AppKit
/// appearance matching instead of per-view branches.
public enum SessionPalette {
    // Overhaul M1: every role forwards to `AeroricColor`, the single colour
    // vocabulary. The names stay so unmigrated views keep compiling and pick
    // up the new values instead of disagreeing with migrated ones.
    public static let canvas = AeroricColor.canvas
    public static let foundation = AeroricColor.canvas
    public static let sidebar = AeroricColor.sidebar
    public static let surface = AeroricColor.canvas
    public static let input = AeroricColor.layer1
    public static let card = AeroricColor.layer1
    public static let chip = AeroricColor.layer2
    public static let bubble = AeroricColor.userBubble
    public static let text = AeroricColor.text
    public static let secondary = AeroricColor.textSecondary
    public static let muted = AeroricColor.textTertiary
    public static let edge = AeroricColor.separator2
    public static let accent = AeroricColor.accent
    public static let blue = accent
    // Hue accents used inside conversation content (not status roles).
    public static let teal = dynamic(light: 0x16856F, dark: 0x5CC9A7)
    public static let violet = dynamic(light: 0x7A5AD6, dark: 0xA992F2)
    public static let amber = AeroricColor.warning
    public static let tintBlue = dynamic(light: 0xEEF3FD, dark: 0x1E2638)
    public static let tintTeal = dynamic(light: 0xE8F6F2, dark: 0x1A2A26)
    public static let tintAmber = dynamic(light: 0xFDF5E6, dark: 0x2E2719)
    public static let hover = AeroricColor.hover
    public static let selection = AeroricColor.selection
    /// No gutters between panels any more: the columns sit flush.
    public static let gutter = AeroricColor.canvas
    public static let orbit = edge
    public static let shadow = AeroricColor.shadow

    static func dynamic(light: UInt32, dark: UInt32,
                        lightContrast: UInt32? = nil, darkContrast: UInt32? = nil) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let match = appearance.bestMatch(from: [
                .aqua, .darkAqua, .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua,
            ])
            switch match {
            case .accessibilityHighContrastDarkAqua: return rgb(darkContrast ?? dark)
            case .accessibilityHighContrastAqua: return rgb(lightContrast ?? light)
            case .darkAqua: return rgb(dark)
            default: return rgb(light)
            }
        })
    }

    private static func rgb(_ hex: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
                green: CGFloat((hex >> 8) & 0xFF) / 255,
                blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}

/// Redesign v2 radii: soft, capsule-like corners everywhere. Single-line
/// controls (chips, buttons, attachment pills, hover rows) use `Capsule()`
/// directly; these values are for surfaces too tall to be a true capsule.
public enum ShellRadius {
    /// Multi-line inputs (new-session prompt, continuation box).
    public static let composer: CGFloat = 24
    /// Cards and floating panels (capability cards, entries, palette).
    public static let card: CGFloat = 20
    /// Code, diff and tool-output blocks inside the conversation.
    public static let block: CGFloat = 14
    public static let control: CGFloat = card
    public static let chip: CGFloat = block
    /// Redesign v3: sidebar, centre and auxiliary panels are rounded cards.
    public static let panel: CGFloat = 16
    /// Gap between the rounded panels and around the window edge.
    public static let panelGap: CGFloat = 6
}

/// Redesign v3 rounded panel: opaque fill, continuous corners, hairline edge.
public struct RoundedShellPanel<Content: View>: View {
    private let fill: AnyShapeStyle
    private let content: Content
    @Environment(\.colorSchemeContrast) private var contrast

    public init<S: ShapeStyle>(fill: S, @ViewBuilder content: () -> Content) {
        self.fill = AnyShapeStyle(fill)
        self.content = content()
    }

    public var body: some View {
        let shape = RoundedRectangle(cornerRadius: ShellRadius.panel, style: .continuous)
        content
            .background(fill, in: shape)
            .clipShape(shape)
            .overlay {
                shape.strokeBorder(SessionPalette.edge.opacity(contrast == .increased ? 1 : 0.8),
                                   lineWidth: contrast == .increased ? 1 : 0.5)
                    .allowsHitTesting(false)
            }
    }
}

/// Sidebar backdrop: the system sidebar material (Liquid Glass on macOS 26),
/// replaced by an opaque role when Reduce Transparency is on.
public struct SidebarBackdrop: View {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    public init() {}
    public var body: some View {
        if reduceTransparency { SessionPalette.sidebar } else { NativeMaterial.sidebar }
    }
}

/// Kept for source compatibility with surfaces not yet migrated: now a flat
/// opaque fill with no floating card outline.
public struct ShellPanel<Content: View>: View {
    private let fill: Color
    private let content: Content

    public init(fill: Color, cornerRadius: CGFloat = 0, @ViewBuilder content: () -> Content) {
        self.fill = fill
        self.content = content()
    }

    public var body: some View {
        content.background(fill)
    }
}
