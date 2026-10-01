import SwiftUI
import AppKit

// Window chrome configuration (Task P4-03 Action 4).
//
// Constraints (spec / ADR-001): the standard red/yellow/green traffic
// lights stay untouched; no global swizzling; no reaching into private
// subviews (unlike primuse's button-alignment repair pass, which the
// Aeroric windows do not need because nothing is drawn over the title
// bar). Applying the same values twice must be a no-op.

/// Attaches to the hosting window once it exists and switches it to the
/// transparent title-bar / full-size-content chrome used by the native
/// client. Dragging in the blank title area stays the system behaviour
/// (`isMovableByWindowBackground`).
public struct WindowAppearance: NSViewRepresentable {
    public init() {}

    public func makeNSView(context: Context) -> ConfiguratorView {
        let view = ConfiguratorView(frame: .zero)
        view.onWindowChange = { Self.apply(to: $0) }
        return view
    }

    public func updateNSView(_ view: ConfiguratorView, context: Context) {
        view.onWindowChange = { Self.apply(to: $0) }
        if let window = view.window {
            Self.apply(to: window)
        }
    }

    /// The host view reports its window through an override instead of
    /// didMoveToWindow so SwiftUI re-parenting still triggers the apply.
    public final class ConfiguratorView: NSView {
        var onWindowChange: (NSWindow) -> Void = { _ in }

        public override init(frame frameRect: NSRect) {
            super.init(frame: frameRect)
        }

        @available(*, unavailable)
        public required init?(coder: NSCoder) {
            fatalError("ConfiguratorView is created in code only")
        }

        public override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { onWindowChange(window) }
        }
    }

    static func apply(to window: NSWindow) {
        let requiredStyleMask: NSWindow.StyleMask = [
            .titled, .closable, .miniaturizable, .resizable, .fullSizeContentView,
        ]
        let missing = requiredStyleMask.subtracting(window.styleMask)
        if !missing.isEmpty {
            window.styleMask.formUnion(missing)
        }
        if window.titleVisibility != .hidden {
            window.titleVisibility = .hidden
        }
        if !window.titlebarAppearsTransparent {
            window.titlebarAppearsTransparent = true
        }
        if window.titlebarSeparatorStyle != .none {
            // `.automatic` makes AppKit relayout the title bar while a
            // list under it scrolls, dragging the traffic lights along;
            // there is no custom hairline here to suppress.
            window.titlebarSeparatorStyle = .none
        }
        if window.backgroundColor.alphaComponent > 0.001 {
            window.backgroundColor = .clear
        }
        if !window.isMovableByWindowBackground {
            window.isMovableByWindowBackground = true
        }
    }
}
