import AppKit
import SwiftUI

/// The single colour vocabulary of the redesign (overhaul M1).
///
/// Roles, not hues: each one is a dynamic `NSColor` resolved per appearance
/// (light, dark, and both increased-contrast variants), so a view never
/// branches on the colour scheme. Values start from the cool neutral scale the
/// user picked (DeepSeek Harness light/dark relation). This is the only colour
/// vocabulary; the old `SessionPalette` / `Color.ds*` forwarders were removed.
public enum AeroricColor {
    // MARK: Surfaces

    /// Opaque reading area: conversation, code, diff, terminal surroundings.
    public static let canvas = dynamic(light: .rgb(0xFFFFFF), dark: .rgb(0x151517))
    /// Tint laid over the sidebar glass; also the sidebar fill when
    /// transparency is reduced.
    public static let sidebar = dynamic(light: .rgb(0xF5F6F8), dark: .rgb(0x1B1B1C))
    /// First raised layer: cards, inspector content, inputs at rest.
    public static let layer1 = dynamic(light: .rgb(0xF9FAFB), dark: .rgb(0x232324))
    /// Second layer: chips, hover-level fills inside cards, user bubble (dark).
    public static let layer2 = dynamic(light: .rgb(0xF1F3F5), dark: .rgb(0x2C2C2E))
    /// Third layer: pressed chips, popover rows.
    public static let layer3 = dynamic(light: .rgb(0xE9ECEF), dark: .rgb(0x353638))
    /// User message bubble.
    public static let userBubble = dynamic(light: .rgb(0xEDF3FE), dark: .rgb(0x2C2C2E))

    // MARK: Text

    // 2026-10-29: dark text dimmed twice at the owner's request (was F2F3F5 /
    // B8BCC2 / 8B9097, ~17:1 on the canvas, read as glaring): body 10.5:1,
    // secondary 6.3:1, tertiary 4.5:1 (the floor for small text). The canvas
    // is unchanged and the increased-contrast variants keep their values.
    public static let text = dynamic(light: .rgb(0x0F1115), dark: .rgb(0xC2C5CA),
                                     lightContrast: .rgb(0x000000), darkContrast: .rgb(0xFFFFFF))
    public static let textSecondary = dynamic(light: .rgb(0x61666B), dark: .rgb(0x94989E),
                                              lightContrast: .rgb(0x353638), darkContrast: .rgb(0xE9ECF2))
    public static let textTertiary = dynamic(light: .rgb(0x8A8F96), dark: .rgb(0x7A7F85),
                                             lightContrast: .rgb(0x43454A), darkContrast: .rgb(0xCFD3D6))

    // MARK: Lines (alpha ladder, never solid greys)

    public static let separator1 = dynamic(light: .black(0.04), dark: .white(0.06),
                                           lightContrast: .black(0.20), darkContrast: .white(0.24))
    public static let separator2 = dynamic(light: .black(0.10), dark: .white(0.12),
                                           lightContrast: .black(0.35), darkContrast: .white(0.40))
    public static let separator3 = dynamic(light: .black(0.12), dark: .white(0.16),
                                           lightContrast: .black(0.45), darkContrast: .white(0.50))
    public static let separator4 = dynamic(light: .black(0.16), dark: .white(0.20),
                                           lightContrast: .black(0.55), darkContrast: .white(0.60))

    // MARK: Interaction

    /// Capsule hover fill (codeg rows, ghost buttons).
    public static let hover = dynamic(light: .rgba(0x263148, 0.06), dark: .white(0.08),
                                      lightContrast: .rgba(0x263148, 0.14), darkContrast: .white(0.16))
    public static let pressed = dynamic(light: .rgba(0x263148, 0.10), dark: .white(0.12),
                                        lightContrast: .rgba(0x263148, 0.20), darkContrast: .white(0.22))
    /// Selected row / open-menu capsule: accent-tinted, one step stronger than hover.
    public static let selection = dynamic(light: .rgba(0x4176E6, 0.12), dark: .rgba(0x7AAAFF, 0.16),
                                          lightContrast: .rgba(0x4176E6, 0.24), darkContrast: .rgba(0x7AAAFF, 0.30))
    /// Focus, links, selected glyphs. Decoration only — primary buttons use `primary`.
    public static let accent = dynamic(light: .rgb(0x4176E6), dark: .rgb(0x7AAAFF),
                                       lightContrast: .rgb(0x2A5BC4), darkContrast: .rgb(0xA6C6FF))
    /// Primary button fill: near-black in light, near-white in dark.
    public static let primary = dynamic(light: .rgb(0x17181B), dark: .rgb(0xE9EBEE))
    public static let onPrimary = dynamic(light: .rgb(0xFFFFFF), dark: .rgb(0x151517))
    /// Decorative hues for capability / category marks (never status). Tints
    /// are `.opacity(0.12)` of these.
    public static let teal = dynamic(light: .rgb(0x16856F), dark: .rgb(0x5CC9A7))
    public static let violet = dynamic(light: .rgb(0x7A5AD6), dark: .rgb(0xA992F2))

    // MARK: Status (fixed meaning across the app)

    public static let success = dynamic(light: .rgb(0x16A34A), dark: .rgb(0x22C55E))
    public static let warning = dynamic(light: .rgb(0xD97706), dark: .rgb(0xF59E0B))
    public static let danger = dynamic(light: .rgb(0xEC1313), dark: .rgb(0xF25A5A))
    /// Running / in progress: a calm blue-grey, not the accent.
    public static let running = dynamic(light: .rgb(0x5B7BA8), dark: .rgb(0x8FA9CF))

    // MARK: Elevation

    public static let shadow = dynamic(light: .black(0.10), dark: .black(0.45))

    // MARK: Resolution

    public enum Swatch: Equatable, Sendable {
        case rgb(UInt32)
        case rgba(UInt32, CGFloat)
        case white(CGFloat)
        case black(CGFloat)

        var nsColor: NSColor {
            switch self {
            case let .rgb(hex): return Self.color(hex, alpha: 1)
            case let .rgba(hex, alpha): return Self.color(hex, alpha: alpha)
            case let .white(alpha): return NSColor(srgbRed: 1, green: 1, blue: 1, alpha: alpha)
            case let .black(alpha): return NSColor(srgbRed: 0, green: 0, blue: 0, alpha: alpha)
            }
        }

        private static func color(_ hex: UInt32, alpha: CGFloat) -> NSColor {
            NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
                    green: CGFloat((hex >> 8) & 0xFF) / 255,
                    blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
        }
    }

    static let appearances: [NSAppearance.Name] = [
        .aqua, .darkAqua, .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua,
    ]

    /// The four appearances a role answers for. Takes the matched appearance
    /// name (the system supplies the high-contrast names when Increase
    /// Contrast is on); a role without contrast values falls back.
    static func swatch(for match: NSAppearance.Name?, light: Swatch, dark: Swatch,
                       lightContrast: Swatch?, darkContrast: Swatch?) -> Swatch {
        switch match {
        case .accessibilityHighContrastDarkAqua: return darkContrast ?? dark
        case .accessibilityHighContrastAqua: return lightContrast ?? light
        case .darkAqua: return dark
        default: return light
        }
    }

    public static func dynamic(light: Swatch, dark: Swatch,
                               lightContrast: Swatch? = nil, darkContrast: Swatch? = nil) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            swatch(for: appearance.bestMatch(from: appearances), light: light, dark: dark,
                   lightContrast: lightContrast, darkContrast: darkContrast).nsColor
        })
    }
}
