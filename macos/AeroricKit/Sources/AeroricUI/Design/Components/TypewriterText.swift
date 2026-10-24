import SwiftUI

/// Heading that appears character by character (Agents-Anywhere reveal text);
/// static under Reduce Motion. Layout is reserved up front so nothing jumps.
public struct TypewriterText: View {
    let text: String
    let size: CGFloat
    @State private var shown = 0
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(_ text: String, size: CGFloat) {
        self.text = text
        self.size = size
    }

    public var body: some View {
        Text(text)
            .font(.system(size: size, weight: .semibold))
            .opacity(0)
            .overlay(alignment: .topLeading) {
                Text(String(text.prefix(reduceMotion ? text.count : shown)))
                    .font(.system(size: size, weight: .semibold))
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(text)
            .task(id: text) {
                guard !reduceMotion else { return }
                shown = 0
                let step = min(0.06, 0.6 / Double(max(1, text.count)))
                for index in 1...max(1, text.count) {
                    try? await Task.sleep(for: .seconds(step))
                    shown = index
                }
            }
    }
}
