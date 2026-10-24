import SwiftUI

// Overhaul M1 controls: capsule rows, capsule buttons, capsule pickers, key
// chips and the "coming soon" badge. One implementation each, so hover and
// selection look the same everywhere instead of being hand-drawn per view.

/// Selectable row with the codeg-style capsule: nothing at rest, a soft fill
/// on hover, an accent-tinted fill when selected.
public struct HoverCapsuleRow<Label: View>: View {
    let selected: Bool
    let height: CGFloat
    let horizontalPadding: CGFloat
    let action: () -> Void
    let label: () -> Label
    @State private var hovered = false
    @Environment(\.isEnabled) private var isEnabled

    public init(selected: Bool, height: CGFloat = 30, horizontalPadding: CGFloat = AeroricSpace.m,
                action: @escaping () -> Void, @ViewBuilder label: @escaping () -> Label) {
        self.selected = selected
        self.height = height
        self.horizontalPadding = horizontalPadding
        self.action = action
        self.label = label
    }

    public var body: some View {
        Button(action: action) {
            label()
                .padding(.horizontal, horizontalPadding)
                .frame(maxWidth: .infinity, minHeight: height, maxHeight: height, alignment: .leading)
                .background {
                    if selected || (hovered && isEnabled) {
                        Capsule(style: .continuous)
                            .fill(selected ? AeroricColor.selection : AeroricColor.hover)
                    }
                }
                .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Capsule button family. `ghost` is the default chrome button; `primary` is
/// near-black / near-white (the accent is decoration only).
public struct AeroricCapsuleButtonStyle: ButtonStyle {
    public enum Kind: Sendable { case ghost, tinted, primary, destructive }
    let kind: Kind
    let compact: Bool

    public init(_ kind: Kind = .ghost, compact: Bool = false) {
        self.kind = kind
        self.compact = compact
    }

    public func makeBody(configuration: Configuration) -> some View {
        CapsuleButtonBody(configuration: configuration, kind: kind, compact: compact)
    }

    private struct CapsuleButtonBody: View {
        let configuration: Configuration
        let kind: Kind
        let compact: Bool
        @State private var hovered = false
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .font(.system(size: compact ? AeroricType.secondary : AeroricType.body, weight: .medium))
                .foregroundStyle(foreground)
                .padding(.horizontal, compact ? AeroricSpace.m : AeroricSpace.l)
                .frame(height: compact ? 24 : 30)
                .background(Capsule(style: .continuous).fill(fill))
                .overlay {
                    if kind == .tinted {
                        Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 0.5)
                    }
                }
                .opacity(isEnabled ? 1 : 0.4)
                .contentShape(Capsule(style: .continuous))
                .onHover { hovered = $0 }
        }

        private var foreground: Color {
            switch kind {
            case .ghost, .tinted: AeroricColor.text
            case .primary: AeroricColor.onPrimary
            case .destructive: AeroricColor.danger
            }
        }

        private var fill: Color {
            let active = isEnabled && (hovered || configuration.isPressed)
            switch kind {
            case .ghost: return configuration.isPressed ? AeroricColor.pressed : (active ? AeroricColor.hover : .clear)
            case .tinted: return active ? AeroricColor.layer3 : AeroricColor.layer2
            case .primary: return AeroricColor.primary.opacity(configuration.isPressed ? 0.8 : 1)
            case .destructive: return active ? AeroricColor.danger.opacity(0.14) : AeroricColor.danger.opacity(0.08)
            }
        }
    }
}

/// Picker whose closed state is a capsule "● 标题 ⌄" (Agents-Anywhere /
/// codeg composer footer). The options are supplied by the caller; M2 fills in
/// the composer's permission / model / effort menus.
public struct CapsulePicker<MenuContent: View>: View {
    let title: String
    let systemImage: String?
    let dot: Color?
    let menu: () -> MenuContent
    @State private var hovered = false

    public init(title: String, systemImage: String? = nil, dot: Color? = nil,
                @ViewBuilder menu: @escaping () -> MenuContent) {
        self.title = title
        self.systemImage = systemImage
        self.dot = dot
        self.menu = menu
    }

    public var body: some View {
        Menu(content: menu) {
            HStack(spacing: AeroricSpace.s) {
                if let dot {
                    Circle().fill(dot).frame(width: 6, height: 6)
                }
                if let systemImage {
                    Image(systemName: systemImage).imageScale(.small)
                }
                Text(title).lineLimit(1)
                Image(systemName: "chevron.down")
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(AeroricColor.textSecondary)
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 24)
            .background(Capsule(style: .continuous).fill(hovered ? AeroricColor.hover : .clear))
            .contentShape(Capsule(style: .continuous))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .onHover { hovered = $0 }
    }
}

/// Keyboard shortcut chip ("⌘K").
public struct KeyChip: View {
    let keys: String
    public init(_ keys: String) { self.keys = keys }

    public var body: some View {
        Text(keys)
            .font(.system(size: AeroricType.caption, weight: .medium).monospacedDigit())
            .foregroundStyle(AeroricColor.textTertiary)
            .padding(.horizontal, 5)
            .frame(height: 18)
            .background(AeroricRadius.shape(AeroricRadius.xs).fill(AeroricColor.layer2))
            .accessibilityLabel("快捷键 \(keys)")
    }
}

/// Q8: an entry whose capability is not wired yet says so instead of acting.
public struct ComingSoonBadge: View {
    public init() {}

    public var body: some View {
        Text("即将支持")
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(AeroricColor.textTertiary)
            .padding(.horizontal, 6)
            .frame(height: 16)
            .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
    }
}
