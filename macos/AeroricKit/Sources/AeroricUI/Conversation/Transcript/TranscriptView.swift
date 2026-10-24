import SwiftUI

/// Conversation column. User turns are right-aligned bubbles (≤ 85%),
/// assistant turns are unboxed; tool calls are single lines. Renders
/// host-supplied history only — the live TUI stays in the terminal.
struct TranscriptView<Footer: View>: View {
    let items: [TranscriptItem]
    let isLoading: Bool
    let pending: [PendingRequestStore.PendingRequest]
    let store: PendingRequestStore
    let onRespond: (String, PendingRequestStore.Outcome) -> Void
    @ViewBuilder let footer: () -> Footer
    @State private var atBottom = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private static var bottomID: String { "transcript.bottom" }

    var body: some View {
        GeometryReader { proxy in
            let column = TranscriptGeometry.columnWidth(for: proxy.size.width)
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: AeroricSpace.l) {
                        if items.isEmpty && pending.isEmpty {
                            AeroricEmptyState(
                                systemImage: isLoading ? "hourglass" : "text.bubble",
                                title: isLoading ? "正在读取会话历史…" : "还没有结构化历史",
                                message: isLoading ? nil : "Agent 写出会话记录后会显示在这里。"
                            )
                            .padding(.top, 60)
                        }
                        ForEach(Array(items.enumerated()), id: \.element.id) { offset, item in
                            row(item, offset: offset, column: column)
                        }
                        ForEach(pending) { request in
                            ApprovalCard(request: request, store: store, onSubmit: onRespond)
                        }
                        Color.clear.frame(height: 1).id(Self.bottomID)
                            .onAppear { atBottom = true }
                            .onDisappear { atBottom = false }
                    }
                    .frame(width: column)
                    .padding(.vertical, AeroricSpace.xxxl)
                    .frame(maxWidth: .infinity)
                }
                .scrollIndicators(.automatic)
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    footer()
                        .frame(width: column)
                        .padding(.bottom, AeroricSpace.l)
                        .frame(maxWidth: .infinity)
                        .overlay(alignment: .top) {
                            if !atBottom && !items.isEmpty {
                                jumpButton { scroll(scroller) }.offset(y: -44)
                            }
                        }
                }
                .onChange(of: items.count) { _, _ in if atBottom { scroll(scroller) } }
                .onChange(of: pending.count) { _, count in if count > 0 { scroll(scroller) } }
                .onAppear { scroller.scrollTo(Self.bottomID, anchor: .bottom) }
            }
        }
        .background(AeroricColor.canvas)
        .accessibilityIdentifier("session.messages")
    }

    private func scroll(_ scroller: ScrollViewProxy) {
        withAnimation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion)) {
            scroller.scrollTo(Self.bottomID, anchor: .bottom)
        }
    }

    @ViewBuilder private func row(_ item: TranscriptItem, offset: Int, column: CGFloat) -> some View {
        switch item {
        case let .message(_, role, content):
            MessageBubble(role: role, content: content, columnWidth: column)
                .accessibilityIdentifier("session.message-row.\(offset)")
        case let .tool(call):
            ToolCallLine(call: call, index: offset)
        case let .toolGroup(_, calls):
            ToolGroupLine(calls: calls, firstIndex: offset)
        }
    }

    private func jumpButton(_ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label("回到底部", systemImage: "arrow.down")
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 28)
                .glassSurface(.popover, in: Capsule(style: .continuous))
                .aeroricElevation(.floating, in: Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("session.jump-to-latest")
    }
}

/// User bubble (userBubble fill, radius 16, ≤ 85% of the column) or
/// unboxed assistant prose.
struct MessageBubble: View {
    let role: String
    let content: [SessionContent]
    let columnWidth: CGFloat

    static let userWidthRatio: CGFloat = 0.85
    private var isUser: Bool { role == "user" }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            ForEach(Array(content.enumerated()), id: \.offset) { _, block in
                ContentBlock(block: block)
            }
        }
        .font(.system(size: AeroricType.body))
        .padding(.horizontal, isUser ? AeroricSpace.l + 2 : 0)
        .padding(.vertical, isUser ? AeroricSpace.m + 2 : 0)
        .background {
            if isUser { AeroricRadius.shape(AeroricRadius.l).fill(AeroricColor.userBubble) }
        }
        .frame(maxWidth: isUser ? columnWidth * Self.userWidthRatio : .infinity,
               alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: isUser ? .trailing : .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(isUser ? "用户" : "Agent")
    }
}
