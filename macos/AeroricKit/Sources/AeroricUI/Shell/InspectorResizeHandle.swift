import SwiftUI
import AppKit

/// The window root owns the stored inspector width preference; ready content
/// draws the handle in the gap beside the auxiliary panel and writes here.
private struct MainShellInspectorWidthKey: EnvironmentKey {
    static let defaultValue: Binding<Double>? = nil
}

public extension EnvironmentValues {
    /// Stored auxiliary-panel width; 0 means "first opening" (the 320pt default).
    var mainShellInspectorWidth: Binding<Double>? {
        get { self[MainShellInspectorWidthKey.self] }
        set { self[MainShellInspectorWidthKey.self] = newValue }
    }
}

/// Drag handle for the auxiliary panel. Dragging left widens the panel.
/// Clamped to 260–520pt; the budget still protects the centre.
public struct InspectorResizeHandle: View {
    private let width: Binding<Double>?
    private let effectiveWidth: CGFloat
    private let totalWidth: CGFloat
    @State private var dragStart: CGFloat?

    public init(width: Binding<Double>?, effectiveWidth: CGFloat, totalWidth: CGFloat) {
        self.width = width
        self.effectiveWidth = effectiveWidth
        self.totalWidth = totalWidth
    }

    public var body: some View {
        // The column hairline, centred in a wider invisible drag strip.
        Color.clear
            .overlay(alignment: .trailing) { HairlineDivider(.vertical) }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onHover { inside in
                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(
                // Global space: the handle moves with the panel edge.
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        guard let width else { return }
                        if dragStart == nil { dragStart = effectiveWidth }
                        let shell = NativePanelGeometry.mainShell
                        let next = (dragStart ?? effectiveWidth) - value.translation.width
                        width.wrappedValue = Double(min(shell.maximumInspectorWidth,
                                                        max(shell.minimumInspectorWidth, next)))
                    }
                    .onEnded { _ in dragStart = nil }
            )
            .accessibilityLabel("调整辅助面板宽度")
            .accessibilityIdentifier("auxiliary.resize.handle")
    }
}
