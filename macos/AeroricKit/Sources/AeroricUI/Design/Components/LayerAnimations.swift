import AppKit
import SwiftUI

// 2026-11-12 (perf): looping indicators run as Core Animation on the render
// server. A `TimelineView(.animation)` re-laid out the whole window every
// frame — one running dot in the sidebar kept the main thread ~96% busy and
// the App at ~60% CPU while nothing else happened.

/// A 30% arc that turns once a second, clockwise, starting at 3 o'clock.
struct SpinningArc: NSViewRepresentable {
    let color: Color
    let lineWidth: CGFloat

    func makeNSView(context: Context) -> SpinningArcView {
        let view = SpinningArcView()
        view.lineWidth = lineWidth
        view.color = NSColor(color)
        return view
    }

    func updateNSView(_ view: SpinningArcView, context: Context) {
        view.lineWidth = lineWidth
        view.color = NSColor(color)
    }
}

final class SpinningArcView: NSView {
    private let arc = CAShapeLayer()
    var color: NSColor = .labelColor { didSet { applyColor() } }
    var lineWidth: CGFloat = 1.5 { didSet { needsLayout = true } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        arc.fillColor = nil
        arc.lineCap = .round
        arc.strokeEnd = 0.3
        layer?.addSublayer(arc)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    // Top-left origin, like SwiftUI: the ellipse path then runs clockwise on
    // screen and a positive rotation turns clockwise too.
    override var isFlipped: Bool { true }

    // Decoration only: a click on the dot belongs to the row around it.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func layout() {
        super.layout()
        let inset = lineWidth / 2
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        arc.frame = bounds
        arc.lineWidth = lineWidth
        arc.path = CGPath(ellipseIn: bounds.insetBy(dx: inset, dy: inset), transform: nil)
        CATransaction.commit()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        // AppKit drops layer animations when a view leaves its window.
        if window != nil, arc.animation(forKey: "spin") == nil {
            let spin = CABasicAnimation(keyPath: "transform.rotation.z")
            spin.fromValue = 0
            spin.toValue = 2 * Double.pi
            spin.duration = 1
            spin.repeatCount = .infinity
            spin.isRemovedOnCompletion = false
            arc.add(spin, forKey: "spin")
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyColor()
    }

    private func applyColor() {
        var resolved: CGColor = color.cgColor
        effectiveAppearance.performAsCurrentDrawingAppearance { resolved = color.cgColor }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        arc.strokeColor = resolved
        CATransaction.commit()
    }
}

/// A soft band, half the view wide, that sweeps left to right every 1.6 s
/// (its leading edge from -50% to +100% of the width).
struct ShimmerBand: NSViewRepresentable {
    let color: Color

    func makeNSView(context: Context) -> ShimmerBandView {
        let view = ShimmerBandView()
        view.color = NSColor(color)
        return view
    }

    func updateNSView(_ view: ShimmerBandView, context: Context) {
        view.color = NSColor(color)
    }
}

final class ShimmerBandView: NSView {
    private let band = CAGradientLayer()
    private var sweptWidth: CGFloat = -1
    var color: NSColor = .clear { didSet { applyColor() } }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.masksToBounds = true
        band.startPoint = CGPoint(x: 0, y: 0.5)
        band.endPoint = CGPoint(x: 1, y: 0.5)
        layer?.addSublayer(band)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override var isFlipped: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        band.frame = CGRect(x: 0, y: 0, width: bounds.width * 0.5, height: bounds.height)
        CATransaction.commit()
        if bounds.width != sweptWidth { restartSweep() }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil { restartSweep() }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyColor()
    }

    private func restartSweep() {
        guard window != nil, bounds.width > 0 else { return }
        sweptWidth = bounds.width
        let sweep = CABasicAnimation(keyPath: "transform.translation.x")
        sweep.fromValue = -0.5 * bounds.width
        sweep.toValue = bounds.width
        sweep.duration = 1.6
        sweep.repeatCount = .infinity
        sweep.isRemovedOnCompletion = false
        band.add(sweep, forKey: "sweep")
    }

    private func applyColor() {
        var resolved: CGColor = color.cgColor
        effectiveAppearance.performAsCurrentDrawingAppearance { resolved = color.cgColor }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        band.colors = [resolved.copy(alpha: 0) ?? resolved, resolved, resolved.copy(alpha: 0) ?? resolved]
        CATransaction.commit()
    }
}
