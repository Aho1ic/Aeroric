import SwiftUI

/// B3-1 (2026-11-18): the owner's rule is that every text field is a capsule.
/// `CapsuleField` draws one from scratch; this style gives an existing
/// `TextField` (with its own focus, submit and axis) the same chrome, so no
/// field needs the system bordered style.
public struct CapsuleTextFieldStyle: TextFieldStyle {
    public var height: CGFloat = 28

    public init(height: CGFloat = 28) {
        self.height = height
    }

    public func _body(configuration: TextField<Self._Label>) -> some View {
        configuration
            // Round-3 review: a custom style still draws the AppKit bezel, a
            // small-corner rectangle inside the capsule, unless the field
            // itself is plain; and a focused field drew the focus ring.
            .textFieldStyle(.plain)
            .focusEffectDisabled()
            .font(.system(size: AeroricType.body))
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.xs)
            .frame(minHeight: height)
            .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
    }
}

public extension TextFieldStyle where Self == CapsuleTextFieldStyle {
    /// A capsule like `CapsuleField`.
    static var capsule: CapsuleTextFieldStyle { CapsuleTextFieldStyle() }
}
