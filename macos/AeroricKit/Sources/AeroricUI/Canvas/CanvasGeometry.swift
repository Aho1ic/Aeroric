import AeroricClient
import CoreGraphics
import Foundation

/// Viewport: screen = world × scale + offset. Pure value type so pan / zoom
/// and every hit test are unit-testable without SwiftUI.
public struct CanvasViewport: Equatable, Sendable {
    public static let minScale: CGFloat = 0.25
    public static let maxScale: CGFloat = 2.0

    public var scale: CGFloat
    public var offset: CGPoint

    public init(scale: CGFloat = 1, offset: CGPoint = .zero) {
        self.scale = Self.clamp(scale)
        self.offset = offset
    }

    public static func clamp(_ scale: CGFloat) -> CGFloat { min(maxScale, max(minScale, scale)) }

    public func toScreen(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x * scale + offset.x, y: point.y * scale + offset.y)
    }

    public func toWorld(_ point: CGPoint) -> CGPoint {
        CGPoint(x: (point.x - offset.x) / scale, y: (point.y - offset.y) / scale)
    }

    public func toScreen(_ rect: CGRect) -> CGRect {
        CGRect(origin: toScreen(rect.origin), size: CGSize(width: rect.width * scale, height: rect.height * scale))
    }

    public func toWorld(_ rect: CGRect) -> CGRect {
        CGRect(origin: toWorld(rect.origin), size: CGSize(width: rect.width / scale, height: rect.height / scale))
    }

    /// Zoom by `factor`, keeping the world point under `anchor` (screen) fixed.
    public func zoomed(by factor: CGFloat, around anchor: CGPoint) -> CanvasViewport {
        let world = toWorld(anchor)
        let scale = Self.clamp(self.scale * factor)
        return CanvasViewport(scale: scale, offset: CGPoint(x: anchor.x - world.x * scale, y: anchor.y - world.y * scale))
    }

    public func panned(by delta: CGSize) -> CanvasViewport {
        CanvasViewport(scale: scale, offset: CGPoint(x: offset.x + delta.width, y: offset.y + delta.height))
    }

    /// World rectangle currently on screen.
    public func visibleWorld(in size: CGSize) -> CGRect {
        toWorld(CGRect(origin: .zero, size: size))
    }

    /// Fit `bounds` (world) into `size` with `padding` (screen points); scale capped at 1.
    public static func fitting(_ bounds: CGRect, in size: CGSize, padding: CGFloat = 48) -> CanvasViewport {
        guard bounds.width > 0, bounds.height > 0, size.width > padding * 2, size.height > padding * 2 else {
            return CanvasViewport()
        }
        let scale = clamp(min(1, min((size.width - padding * 2) / bounds.width, (size.height - padding * 2) / bounds.height)))
        let offset = CGPoint(x: (size.width - bounds.width * scale) / 2 - bounds.minX * scale,
                             y: (size.height - bounds.height * scale) / 2 - bounds.minY * scale)
        return CanvasViewport(scale: scale, offset: offset)
    }
}

public enum CanvasGeometry {
    public static let grid: CGFloat = 8
    /// Off-screen margin (world points) still rendered so panning doesn't pop.
    public static let cullMargin: CGFloat = 200

    public static func snap(_ value: CGFloat, enabled: Bool = true) -> CGFloat {
        enabled ? (value / grid).rounded() * grid : value
    }

    public static func snap(_ point: CGPoint, enabled: Bool = true) -> CGPoint {
        CGPoint(x: snap(point.x, enabled: enabled), y: snap(point.y, enabled: enabled))
    }

    public static func frame(_ node: CanvasClient.Node) -> CGRect {
        CGRect(x: node.x, y: node.y, width: node.width, height: node.height)
    }

    /// Nodes that intersect the visible world rectangle (plus margin).
    public static func visible(_ nodes: [CanvasClient.Node], in world: CGRect) -> [CanvasClient.Node] {
        let area = world.insetBy(dx: -cullMargin, dy: -cullMargin)
        return nodes.filter { frame($0).intersects(area) }
    }

    /// Marquee selection: nodes the rectangle touches.
    public static func hits(_ nodes: [CanvasClient.Node], marquee: CGRect) -> Set<String> {
        Set(nodes.filter { frame($0).intersects(marquee) }.map(\.id))
    }

    /// Nodes fully inside a region (D2: regions group by containment). Other
    /// regions inside it come along too; the region itself does not.
    public static func contained(in region: CanvasClient.Node, among nodes: [CanvasClient.Node]) -> [CanvasClient.Node] {
        let box = frame(region)
        return nodes.filter { $0.id != region.id && box.contains(frame($0)) }
    }

    public static func bounds(_ nodes: [CanvasClient.Node]) -> CGRect? {
        nodes.map(frame).reduce(nil) { partial, rect in partial.map { $0.union(rect) } ?? rect }
    }

    /// Minimap: world area shown = all nodes ∪ viewport, fitted into `size`.
    public struct Minimap: Equatable {
        public var world: CGRect
        public var scale: CGFloat
        public var origin: CGPoint

        public func toMap(_ rect: CGRect) -> CGRect {
            CGRect(x: (rect.minX - world.minX) * scale + origin.x, y: (rect.minY - world.minY) * scale + origin.y,
                   width: rect.width * scale, height: rect.height * scale)
        }

        public func toWorld(_ point: CGPoint) -> CGPoint {
            CGPoint(x: (point.x - origin.x) / scale + world.minX, y: (point.y - origin.y) / scale + world.minY)
        }
    }

    public static func minimap(nodes: [CanvasClient.Node], visible: CGRect, size: CGSize) -> Minimap {
        let world = (bounds(nodes).map { $0.union(visible) } ?? visible).insetBy(dx: -40, dy: -40)
        let scale = min(size.width / max(world.width, 1), size.height / max(world.height, 1))
        let origin = CGPoint(x: (size.width - world.width * scale) / 2, y: (size.height - world.height * scale) / 2)
        return Minimap(world: world, scale: scale, origin: origin)
    }

    /// Default sizes for new nodes, and where to drop them (centre of view, snapped).
    public static func defaultSize(_ kind: CanvasClient.Kind) -> CGSize {
        switch kind {
        case .session: CGSize(width: 280, height: 112)
        case .note: CGSize(width: 220, height: 160)
        case .region: CGSize(width: 640, height: 400)
        case .file: CGSize(width: 320, height: 240)
        }
    }

    public static func dropOrigin(for kind: CanvasClient.Kind, viewport: CanvasViewport, viewSize: CGSize,
                                  cascade: Int = 0) -> CGPoint {
        let centre = viewport.toWorld(CGPoint(x: viewSize.width / 2, y: viewSize.height / 2))
        let size = defaultSize(kind)
        let step = CGFloat(cascade % 8) * 24
        return snap(CGPoint(x: centre.x - size.width / 2 + step, y: centre.y - size.height / 2 + step))
    }
}
