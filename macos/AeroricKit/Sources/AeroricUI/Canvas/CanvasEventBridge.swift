import AppKit
import SwiftUI

/// Scroll (pan, or ⌘ + scroll to zoom), pinch (zoom) and the space key (hold to
/// pan with a drag) for the canvas. SwiftUI on macOS 14 has no scroll / magnify
/// hooks for an arbitrary view, so a background NSView installs a local event
/// monitor and only consumes events whose location is inside its own bounds.
struct CanvasEventBridge: NSViewRepresentable {
    var onPan: (CGSize) -> Void
    var onZoom: (CGFloat, CGPoint) -> Void
    var onSpace: (Bool) -> Void

    func makeNSView(context: Context) -> CatcherView {
        let view = CatcherView()
        view.handlers = self
        return view
    }

    func updateNSView(_ view: CatcherView, context: Context) {
        view.handlers = self
    }

    static func dismantleNSView(_ view: CatcherView, coordinator: ()) {
        view.removeMonitor()
    }

    final class CatcherView: NSView {
        var handlers: CanvasEventBridge?
        private var monitor: Any?

        override var isFlipped: Bool { true }
        // Mouse clicks go to the SwiftUI content above, never here.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            removeMonitor()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.scrollWheel, .magnify, .keyDown, .keyUp]) { [weak self] event in
                self?.handle(event) ?? event
            }
        }

        func removeMonitor() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
        }

        private func inside(_ event: NSEvent) -> CGPoint? {
            guard event.window === window else { return nil }
            let point = convert(event.locationInWindow, from: nil)
            return bounds.contains(point) ? point : nil
        }

        private func handle(_ event: NSEvent) -> NSEvent? {
            switch event.type {
            case .scrollWheel:
                guard let point = inside(event), let handlers else { return event }
                if event.modifierFlags.contains(.command) {
                    let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY / 200 : event.scrollingDeltaY / 20
                    handlers.onZoom(exp(delta), point)
                } else {
                    let factor: CGFloat = event.hasPreciseScrollingDeltas ? 1 : 8
                    handlers.onPan(CGSize(width: event.scrollingDeltaX * factor, height: event.scrollingDeltaY * factor))
                }
                return nil
            case .magnify:
                guard let point = inside(event), let handlers else { return event }
                handlers.onZoom(1 + event.magnification, point)
                return nil
            case .keyDown, .keyUp:
                // Space pans only while the pointer is over the canvas and no text field is editing.
                guard event.keyCode == 49, let window, event.window === window,
                      !(window.firstResponder is NSTextView),
                      bounds.contains(convert(window.mouseLocationOutsideOfEventStream, from: nil))
                else { return event }
                handlers?.onSpace(event.type == .keyDown)
                return nil
            default:
                return event
            }
        }
    }
}
