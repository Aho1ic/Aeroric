import AppKit
import SwiftUI
@testable import AeroricUI
import XCTest

/// Round-3 review (2026-11-18): `.textFieldStyle(.capsule)` drew the AppKit
/// bezel, a small-corner rectangle, inside the capsule. A field in the style
/// must render exactly like a plain field with the same capsule around it.
@MainActor
final class CapsuleTextFieldStyleTests: XCTestCase {
    private func render<V: View>(_ view: V) throws -> Data {
        let host = NSHostingView(rootView: view
            .frame(width: 300, height: 60)
            .background(Color.white)
            .environment(\.colorScheme, .light))
        host.appearance = NSAppearance(named: .aqua)
        host.frame = NSRect(x: 0, y: 0, width: 300, height: 60)
        let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        let rep = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: rep)
        return try XCTUnwrap(rep.representation(using: .png, properties: [:]))
    }

    func testTheCapsuleStyleDrawsNoBezel() throws {
        let styled = try render(
            TextField("x", text: .constant("capsule")).textFieldStyle(.capsule).padding(12))
        let reference = try render(
            TextField("x", text: .constant("capsule")).textFieldStyle(.plain)
                .focusEffectDisabled()
                .font(.system(size: AeroricType.body))
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.xs)
                .frame(minHeight: 28)
                .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
                .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
                .padding(12))
        XCTAssertEqual(styled, reference, "the capsule style looks like a plain field in a capsule")
    }
}
