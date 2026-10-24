import SwiftUI
import AppKit

/// Redesign 01 colour roles, taken from DeepSeek Harness
/// (`packages/client/ui-theme/src/styles/design-platform.css`, neutral-bluish
/// scale; light `:root`, dark `[data-theme=dark]`). Each role is a dynamic
/// NSColor, so light, dark and Increase Contrast resolve through AppKit
/// appearance matching instead of per-view branches.
public enum SessionPalette {
    /// `bg-base`: main content and inspector.
    public static let canvas = dynamic(light: 0xFFFFFF, dark: 0x151517)
    public static let foundation = dynamic(light: 0xF5F6F7, dark: 0x151517)
    /// Sidebar fallback when transparency is reduced (neutral-bluish-50 / 900).
    public static let sidebar = dynamic(light: 0xF9FAFB, dark: 0x1B1B1C)
    public static let surface = dynamic(light: 0xFFFFFF, dark: 0x151517)
    /// `bg-layer-1`: composer, cards.
    public static let input = dynamic(light: 0xFFFFFF, dark: 0x232324)
    public static let card = dynamic(light: 0xF9FAFB, dark: 0x1F1F20)
    /// Hover / chip fill (`bg-module-platform`).
    public static let chip = dynamic(light: 0xF1F3F5, dark: 0x2C2C2E)
    /// User message bubble.
    public static let bubble = dynamic(light: 0xF1F3F5, dark: 0x2C2C2E)
    public static let text = dynamic(light: 0x0F1115, dark: 0xF9FAFB)
    public static let secondary = dynamic(light: 0x61666B, dark: 0xCFD3D6,
                                          lightContrast: 0x353638, darkContrast: 0xE9ECF2)
    public static let muted = dynamic(light: 0x81858C, dark: 0xADB2B8,
                                      lightContrast: 0x43454A, darkContrast: 0xCFD3D6)
    public static let edge = dynamic(light: 0xE5E7EB, dark: 0x2E2F31,
                                     lightContrast: 0x81858C, darkContrast: 0x81858C)
    /// `brand-primary-new-color` (deepseek-500 / 450).
    public static let accent = dynamic(light: 0x4176E6, dark: 0x5686FE)
    public static let blue = accent
    public static let teal = dynamic(light: 0x16856F, dark: 0x5CC9A7)
    public static let violet = dynamic(light: 0x7A5AD6, dark: 0xA992F2)
    public static let amber = dynamic(light: 0xB7791F, dark: 0xF0B85A)
    public static let tintBlue = dynamic(light: 0xEEF3FD, dark: 0x1E2638)
    public static let tintTeal = dynamic(light: 0xE8F6F2, dark: 0x1A2A26)
    public static let tintAmber = dynamic(light: 0xFDF5E6, dark: 0x2E2719)
    /// codeg `hover:bg-accent`: ghost-button and row hover capsule.
    public static let hover = dynamic(light: 0xEEF0F3, dark: 0x2A2A2C)
    /// Selected row / open-menu capsule (one step stronger than hover).
    public static let selection = dynamic(light: 0xE6E9EE, dark: 0x323235,
                                          lightContrast: 0xCFD3D6, darkContrast: 0x4A4C50)
    /// Window foundation visible in the gaps between the rounded panels.
    public static let gutter = dynamic(light: 0xF1F2F4, dark: 0x0F0F10)
    public static let orbit = edge
    public static let shadow = Color.black.opacity(0.08)

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
