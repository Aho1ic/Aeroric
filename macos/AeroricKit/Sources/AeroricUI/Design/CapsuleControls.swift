import SwiftUI
import AppKit

// Redesign v3 capsule controls, modelled on codeg's composer
// (`components/chat/session-config-selector.tsx`, `ui/dropdown-menu.tsx`,
// `chat/agent-selector.tsx`). Drawn in SwiftUI on the user's explicit
// request to match codeg; each control keeps a real Button, a readable
// accessibility label and the selected trait.

/// codeg `Button variant="ghost" size="xs"`: borderless at rest; a capsule
/// highlight appears only while hovered, or while its menu is open.
public struct GhostCapsuleButtonStyle: ButtonStyle {
    private let isActive: Bool
    private let height: CGFloat

    public init(isActive: Bool = false, height: CGFloat = 26) {
        self.isActive = isActive
        self.height = height
    }

    public func makeBody(configuration: Configuration) -> some View {
        GhostCapsuleBody(label: configuration.label, isPressed: configuration.isPressed,
                         isActive: isActive, height: height)
    }
}

private struct GhostCapsuleBody<Label: View>: View {
    let label: Label
    let isPressed: Bool
    let isActive: Bool
    let height: CGFloat
    @State private var hovered = false
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let lit = isEnabled && (hovered || isActive)
        label
            .padding(.horizontal, 8)
            .frame(height: height)
            .foregroundStyle(lit ? SessionPalette.text : SessionPalette.secondary)
            .background {
                if lit {
                    Capsule(style: .continuous)
                        .fill(isActive ? SessionPalette.selection : SessionPalette.hover)
                }
            }
            .contentShape(Capsule(style: .continuous))
            .opacity(isEnabled ? (isPressed ? 0.7 : 1) : 0.45)
            .onHover { hovered = $0 }
    }
}

/// One row of a codeg-style dropdown panel.
public struct DropdownItem: Identifiable {
    public enum Kind { case option(selected: Bool), action, header, note, divider }

    public let id: String
    public let kind: Kind
    public let title: String
    public let detail: String?
    public let systemImage: String?
    public let isEnabled: Bool
    public let perform: @MainActor () -> Void

    private init(id: String, kind: Kind, title: String, detail: String? = nil, systemImage: String? = nil,
                 isEnabled: Bool = true, perform: @escaping @MainActor () -> Void = {}) {
        self.id = id
        self.kind = kind
        self.title = title
        self.detail = detail
        self.systemImage = systemImage
        self.isEnabled = isEnabled
        self.perform = perform
    }

    public static func option(id: String, title: String, detail: String? = nil, systemImage: String? = nil,
                              selected: Bool, enabled: Bool = true,
                              perform: @escaping @MainActor () -> Void) -> DropdownItem {
        DropdownItem(id: "option.\(id)", kind: .option(selected: selected), title: title, detail: detail,
                     systemImage: systemImage, isEnabled: enabled, perform: perform)
    }

    public static func action(id: String, title: String, detail: String? = nil, systemImage: String? = nil,
                              enabled: Bool = true, perform: @escaping @MainActor () -> Void) -> DropdownItem {
        DropdownItem(id: "action.\(id)", kind: .action, title: title, detail: detail,
                     systemImage: systemImage, isEnabled: enabled, perform: perform)
    }

    public static func header(_ title: String) -> DropdownItem {
        DropdownItem(id: "header.\(title)", kind: .header, title: title, isEnabled: false)
    }

    public static func note(_ text: String) -> DropdownItem {
        DropdownItem(id: "note.\(text)", kind: .note, title: text, isEnabled: false)
    }

    public static func divider(_ id: String) -> DropdownItem {
        DropdownItem(id: "divider.\(id)", kind: .divider, title: "", isEnabled: false)
    }
}

/// codeg `DropdownMenuContent` (`rounded-2xl p-1`) with `rounded-xl` items:
/// hover fill, trailing check for the current option.
struct DropdownPanel: View {
    let items: [DropdownItem]
    let dismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            ForEach(items) { item in
                switch item.kind {
                case .header:
                    Text(item.title)
                        .font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                        .foregroundStyle(SessionPalette.muted)
                        .padding(.horizontal, 10)
                        .padding(.top, 6)
                        .padding(.bottom, 2)
                case .note:
                    Text(item.title)
                        .font(AeroricFont.ui(AeroricFont.meta))
                        .foregroundStyle(SessionPalette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                case .divider:
                    Rectangle().fill(SessionPalette.edge).frame(height: 1)
                        .padding(.horizontal, 6).padding(.vertical, 3)
                case let .option(selected):
                    DropdownRow(item: item, selected: selected, dismiss: dismiss)
                case .action:
                    DropdownRow(item: item, selected: false, dismiss: dismiss)
                }
            }
        }
        .padding(5)
        .frame(minWidth: 240, maxWidth: 320, alignment: .leading)
        .font(AeroricFont.ui(AeroricFont.body))
        .foregroundStyle(SessionPalette.text)
    }
}

private struct DropdownRow: View {
    let item: DropdownItem
    let selected: Bool
    let dismiss: () -> Void
    @State private var hovered = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 12, style: .continuous)
        Button {
            dismiss()
            let perform = item.perform
            // Let the popover close before a sheet/file panel is presented.
            Task { @MainActor in perform() }
        } label: {
            HStack(spacing: 10) {
                if let systemImage = item.systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 13))
                        .foregroundStyle(SessionPalette.secondary)
                        .frame(width: 16)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.title).lineLimit(1).truncationMode(.middle)
                    if let detail = item.detail {
                        Text(detail)
                            .font(AeroricFont.ui(AeroricFont.meta))
                            .foregroundStyle(SessionPalette.muted)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                if selected {
                    Image(systemName: "checkmark")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(SessionPalette.accent)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(hovered && item.isEnabled ? SessionPalette.hover : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .disabled(!item.isEnabled)
        .opacity(item.isEnabled ? 1 : 0.5)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A ghost capsule trigger that opens a codeg-style dropdown panel.
/// The panel is a native popover, so focus, Escape and outside clicks keep
/// system behaviour.
public struct CapsuleDropdown<Label: View>: View {
    private let help: String
    private let identifier: String
    private let items: @MainActor () -> [DropdownItem]
    private let label: Label
    @State private var open = false

    public init(help: String, identifier: String,
                items: @escaping @MainActor () -> [DropdownItem],
                @ViewBuilder label: () -> Label) {
        self.help = help
        self.identifier = identifier
        self.items = items
        self.label = label()
    }

    public var body: some View {
        Button { open.toggle() } label: { label }
            .buttonStyle(GhostCapsuleButtonStyle(isActive: open))
            .popover(isPresented: $open, arrowEdge: .top) {
                DropdownPanel(items: items(), dismiss: { open = false })
            }
            .help(help)
            .accessibilityIdentifier(identifier)
    }
}

/// codeg agent selector: a muted capsule track with a sliding white
/// "droplet" behind the selected harness, which alone spells its name.
public struct CapsuleHarnessPicker: View {
    @Binding private var selection: String
    @Namespace private var namespace
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(selection: Binding<String>) { _selection = selection }

    public var body: some View {
        HStack(spacing: 0) {
            ForEach(NativeHarnessPicker.harnesses) { harness in
                pill(harness)
            }
        }
        .padding(2)
        .background(Capsule(style: .continuous).fill(SessionPalette.chip.opacity(0.8)))
        .overlay {
            Capsule(style: .continuous)
                .strokeBorder(SessionPalette.edge.opacity(0.7), lineWidth: 1)
                .allowsHitTesting(false)
        }
        .opacity(isEnabled ? 1 : 0.5)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Harness")
        .accessibilityIdentifier("new-session.harness")
    }

    private func pill(_ harness: NativeHarnessPicker.Harness) -> some View {
        let selected = harness.id == selection
        return Button {
            withAnimation(reduceMotion ? nil : .spring(response: 0.3, dampingFraction: 0.86)) {
                selection = harness.id
            }
        } label: {
            HStack(spacing: 6) {
                Image(nsImage: NSImage(named: harness.asset) ?? NSImage())
                    .resizable()
                    .interpolation(.high)
                    .frame(width: 16, height: 16)
                if selected {
                    Text(harness.title)
                        .font(AeroricFont.ui(12, weight: .medium))
                        .lineLimit(1)
                        .fixedSize()
                }
            }
            .padding(.horizontal, selected ? 12 : 8)
            .frame(height: 28)
            .foregroundStyle(selected ? SessionPalette.text : SessionPalette.muted)
            .background {
                if selected {
                    Capsule(style: .continuous)
                        .fill(SessionPalette.input)
                        .shadow(color: SessionPalette.shadow, radius: 2, y: 1)
                        .overlay {
                            Capsule(style: .continuous).strokeBorder(SessionPalette.edge.opacity(0.7), lineWidth: 1)
                        }
                        .matchedGeometryEffect(id: "harness.thumb", in: namespace)
                }
            }
            .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .help(harness.title)
        .accessibilityLabel(harness.title)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("new-session.harness.\(harness.id)")
    }
}
