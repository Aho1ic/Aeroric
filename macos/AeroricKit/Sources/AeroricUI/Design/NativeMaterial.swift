import SwiftUI
import AppKit

// AppKit material bridge (Task P4-03 Action 2).
//
// The material baseline for the native client is `NSVisualEffectView`
// itself (ADR-001 / spec: "material base is NSVisualEffectView or SwiftUI
// Material"). `glassEffect` may add on top of interactive controls where
// available, but it is never the baseline and never replaced by a plain
// translucent colour.

/// A window-backed visual effect view. Rendering stays entirely inside
/// AppKit: SwiftUI never synthesises a translucency of its own.
public struct NativeMaterial: NSViewRepresentable {
    public var material: NSVisualEffectView.Material
    public var blending: NSVisualEffectView.BlendingMode

    public init(material: NSVisualEffectView.Material = .underWindowBackground,
                blending: NSVisualEffectView.BlendingMode = .behindWindow) {
        self.material = material
        self.blending = blending
    }

    public func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = blending
        view.state = .followsWindowActiveState
        view.isEmphasized = false
        return view
    }

    public func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.material = material
        view.blendingMode = blending
    }
}

/// Sidebar-grade material (`.sidebar` blending behind the window) for
/// navigation columns.
public extension NativeMaterial {
    static var sidebar: NativeMaterial {
        NativeMaterial(material: .sidebar, blending: .behindWindow)
    }

    /// Full-window backdrop under the content.
    static var underWindowBackground: NativeMaterial {
        NativeMaterial(material: .underWindowBackground, blending: .behindWindow)
    }
}
