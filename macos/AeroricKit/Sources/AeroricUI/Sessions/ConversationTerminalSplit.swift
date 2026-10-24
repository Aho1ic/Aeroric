import SwiftUI

/// Redesign 01 session detail (Q15-b, codeg workspace/terminal split):
/// structured conversation on top, the real terminal docked underneath.
/// The terminal is the Agent's own PTY bytes; this view only sizes it.
/// Collapsing leaves a 32pt title bar; dragging the bar resizes the dock.
public struct ConversationTerminalSplit<Conversation: View, Terminal: View>: View {
    @Binding private var terminalExpanded: Bool
    @Binding private var terminalFraction: Double
    private let terminalTitle: String
    private let conversation: Conversation
    private let terminal: Terminal
    @State private var dragStartHeight: CGFloat?

    public static var minimumFraction: Double { 0.12 }
    public static var maximumFraction: Double { 0.8 }

    public init(terminalExpanded: Binding<Bool>, terminalFraction: Binding<Double>,
                terminalTitle: String,
                @ViewBuilder conversation: () -> Conversation,
                @ViewBuilder terminal: () -> Terminal) {
        _terminalExpanded = terminalExpanded
        _terminalFraction = terminalFraction
        self.terminalTitle = terminalTitle
        self.conversation = conversation()
        self.terminal = terminal()
    }

    /// Pure sizing rule, unit-tested: the conversation keeps at least 160pt.
    public static func terminalHeight(total: CGFloat, fraction: Double) -> CGFloat {
        let clamped = min(maximumFraction, max(minimumFraction, fraction))
        let upper = max(96, total - 160)
        return min(upper, max(96, (total * clamped).rounded()))
    }

    public var body: some View {
        GeometryReader { geometry in
            let total = geometry.size.height
            let height = Self.terminalHeight(total: total, fraction: terminalFraction)
            VStack(spacing: 0) {
                conversation
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                dockBar(total: total, height: height)
                if terminalExpanded {
                    terminal
                        .frame(maxWidth: .infinity)
                        .frame(height: height)
                        .background(SessionPalette.canvas)
                }
            }
        }
    }

    private func dockBar(total: CGFloat, height: CGFloat) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "terminal")
                .font(.system(size: 11))
                .foregroundStyle(SessionPalette.secondary)
            Text(terminalTitle)
                .font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                .foregroundStyle(SessionPalette.secondary)
                .lineLimit(1)
            Spacer(minLength: 8)
            NativeIconButton(systemImage: terminalExpanded ? "chevron.down" : "chevron.up",
                             label: terminalExpanded ? "收起终端" : "展开终端",
                             identifier: "session.terminal.toggle") { terminalExpanded.toggle() }
                .foregroundStyle(SessionPalette.secondary)
        }
        .padding(.leading, 14)
        .padding(.trailing, 6)
        .frame(height: NativePanelGeometry.mainShell.terminalBarHeight)
        .background(SessionPalette.card)
        .overlay(alignment: .top) { Rectangle().fill(SessionPalette.edge).frame(height: 1) }
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 2)
                .onChanged { value in
                    guard terminalExpanded, total > 0 else { return }
                    if dragStartHeight == nil { dragStartHeight = height }
                    let next = (dragStartHeight ?? height) - value.translation.height
                    terminalFraction = min(Self.maximumFraction, max(Self.minimumFraction, Double(next / total)))
                }
                .onEnded { _ in dragStartHeight = nil }
        )
        .onHover { inside in
            guard terminalExpanded else { return }
            if inside { NSCursor.resizeUpDown.push() } else { NSCursor.pop() }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.terminal.bar")
    }
}
