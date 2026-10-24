import SwiftUI

// Bare icon button style (stage 01).
//
// The approved rule (review-round-07, `shape-policy-mapping.md`): a
// standalone icon button shows its glyph and nothing else by default — no
// permanent ring, no permanent fill, no circular tray. Everything else
// stays a real `Button`: semantics, hit target, `help`, disabled state,
// hover feedback and the system keyboard focus ring.
//
// `glassEffect` is deliberately NOT applied here. macOS 26+ Liquid Glass
// belongs on interactive controls, but "the product uses Liquid Glass" is
// not a reason to give a bare icon button a permanent glass tray — the
// approved shape rule is explicit about that. Text-bearing actions may use
// the system glass button style where available (see the review window).
//
// Hover is the one state that paints: a subdued capsule that exists only
// while the pointer is over an enabled button. That is pointer feedback,
// not a selection state — adjacent mutually exclusive controls stay on the
// system selection states (`NativeSelection.swift`).

public struct NativeIconButtonStyle: PrimitiveButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Chrome(configuration: configuration)
    }

    private struct Chrome: View {
        let configuration: Configuration

        @Environment(\.isEnabled) private var isEnabled
        @State private var isHovering = false

        var body: some View {
            // Delegate activation, pressed/disabled feedback and keyboard
            // focus to a real system plain Button, not a gesture or a
            // custom label-only ButtonStyle that must rebuild those states.
            Button(role: configuration.role, action: configuration.trigger) {
                configuration.label
                    .frame(minWidth: NativePanelGeometry.iconButtonSize,
                           minHeight: NativePanelGeometry.iconButtonSize)
                    .contentShape(Rectangle())
                    .background {
                        if isHovering && isEnabled {
                            Capsule(style: .continuous)
                                .fill(Color.dsText.opacity(NativePanelGeometry.iconHoverFillOpacity))
                        }
                    }
            }
            .buttonStyle(.plain)
            .onHover { isHovering = $0 }
        }
    }
}

/// A bare icon button with the shared accessibility contract: a readable
/// label, a `help` tooltip and a leaf-level identifier.
///
/// The identifier is required on purpose — the AX rule for this codebase is
/// that identifiers live on leaves and name something a person can read
/// (`docs/native/frontend-redesign-prompt.md` §2.6), and a button is a leaf.
public struct NativeIconButton: View {
    private let systemImage: String
    private let label: LocalizedStringKey
    private let identifier: String
    private let help: LocalizedStringKey?
    private let action: () -> Void

    public init(systemImage: String,
                label: LocalizedStringKey,
                identifier: String,
                help: LocalizedStringKey? = nil,
                action: @escaping () -> Void) {
        self.systemImage = systemImage
        self.label = label
        self.identifier = identifier
        self.help = help
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: DSTypography.body))
        }
        .buttonStyle(NativeIconButtonStyle())
        .accessibilityLabel(label)
        .help(help ?? label)
        .accessibilityIdentifier(identifier)
    }
}
