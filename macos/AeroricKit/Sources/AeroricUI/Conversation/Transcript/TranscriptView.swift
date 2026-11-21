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
                // 2026-10-29: no scroll bar; opens at the latest message.
                .scrollIndicators(.hidden)
                .modifier(OpensAtLatest())
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    footer()
                        .frame(width: column)
                        // 2026-10-29: right against the bottom (was `s`).
                        .padding(.bottom, AeroricSpace.xs)
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

/// User bubble (userBubble fill, radius 16, ≤ 70% of the column, against the
/// column's right edge) or unboxed assistant prose.
struct MessageBubble: View {
    let role: String
    let content: [SessionContent]
    let columnWidth: CGFloat

    static let userWidthRatio: CGFloat = 0.70
    private var isUser: Bool { role == "user" }

    /// Where a message sits in its column. The user's bubble is trailing both
    /// inside its 70% box and in the column; aligning it leading inside the
    /// box left short messages near the middle (2026-10-26 E).
    static func alignment(role: String) -> Alignment { role == "user" ? .trailing : .leading }

    var body: some View {
        // H3: what Aeroric put in front of the person's first message is
        // shown above it, folded, not as their words.
        let split = isUser ? AttachedContext.split(content) : nil
        VStack(alignment: .trailing, spacing: AeroricSpace.xs) {
            if let split {
                AttachedContextRow(text: split.context, reason: AttachedContext.reason(content))
                    .frame(maxWidth: columnWidth * Self.userWidthRatio, alignment: .trailing)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            bubble(split?.rest ?? content)
        }
    }

    @ViewBuilder
    private func bubble(_ content: [SessionContent]) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            ForEach(Array(content.enumerated()), id: \.offset) { _, block in
                ContentBlock(block: block)
            }
        }
        .font(.system(size: AeroricType.conversation))
        .padding(.horizontal, isUser ? AeroricSpace.l + 2 : 0)
        .padding(.vertical, isUser ? AeroricSpace.m + 2 : 0)
        .background {
            if isUser { AeroricRadius.shape(AeroricRadius.l).fill(AeroricColor.userBubble) }
        }
        .frame(maxWidth: isUser ? columnWidth * Self.userWidthRatio : .infinity,
               alignment: Self.alignment(role: role))
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: Self.alignment(role: role))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(isUser ? "用户" : "Agent")
    }
}

/// Opens a transcript at its latest message while short content still starts
/// at the top (2026-10-29). A plain `defaultScrollAnchor(.bottom)` also
/// aligns content shorter than the viewport to the bottom: on a 27" screen a
/// two-message session sat at the bottom of an empty page. macOS 14 has no
/// per-role anchor; the views' own scroll-to-bottom on load covers it there.
struct OpensAtLatest: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 15.0, *) {
            content
                .defaultScrollAnchor(.bottom, for: .initialOffset)
                .defaultScrollAnchor(.top, for: .alignment)
        } else {
            content
        }
    }
}

/// H3 (2026-11-20): the envelope Aeroric puts in front of the first message
/// of a session with no system layer (`opening.rs` in core): its first
/// line opens it, `[Aeroric 附加的上下文结束]` closes it, the person's own
/// words follow. Its body is escaped, so it cannot close early.
public enum AttachedContext {
    public static let marker = "[Aeroric 附加的上下文]"
    public static let end = "[Aeroric 附加的上下文结束]"

    /// The envelope's body and the message after it, or nil.
    public static func split(text: String) -> (context: String, rest: String)? {
        guard text.hasPrefix(marker),
              let close = text.range(of: "\n" + end) else { return nil }
        let head = text[text.startIndex..<close.lowerBound]
        guard let firstBreak = head.firstIndex(of: "\n") else { return nil }
        let context = String(head[head.index(after: firstBreak)...])
        let rest = String(text[close.upperBound...]).trimmingCharacters(in: .newlines)
        return (context, rest)
    }

    /// E12: why the envelope is there, from its opening line
    /// (`来自：Aeroric · <why> · <authority>`): the opening, what recall found,
    /// the tasks referenced with "@".
    public static func reason(text: String) -> String? {
        guard text.hasPrefix(marker), split(text: text) != nil,
              let line = text.split(separator: "\n", maxSplits: 1).first else { return nil }
        let parts = line.components(separatedBy: " · ")
        guard parts.count >= 3 else { return nil }
        let why = parts[1].trimmingCharacters(in: .whitespaces)
        return why.isEmpty ? nil : why
    }

    public static func reason(_ content: [SessionContent]) -> String? {
        for block in content { if case let .text(text) = block { return reason(text: text) } }
        return nil
    }

    /// E12: the person's own words of a message that may carry the envelope.
    public static func typed(_ text: String) -> String { split(text: text)?.rest ?? text }

    /// For a message whose first text block starts with the envelope: the
    /// envelope's body, and the content with that block cut to the person's words.
    public static func split(_ content: [SessionContent]) -> (context: String, rest: [SessionContent])? {
        guard let first = content.firstIndex(where: { if case .text = $0 { return true }; return false }),
              case let .text(text) = content[first],
              let parts = split(text: text) else { return nil }
        var rest = content
        if parts.rest.isEmpty {
            rest.remove(at: first)
        } else {
            rest[first] = .text(text: parts.rest)
        }
        return (parts.context, rest)
    }
}

/// H3: the attached opening, folded under one line.
struct AttachedContextRow: View {
    let text: String
    /// E12: the envelope's own reason; nil reads as the opening (H3).
    var reason: String?
    @State private var expanded = false

    var body: some View {
        FlatDisclosureRow(symbol: "paperclip", title: "Aeroric 附加的上下文",
                          summary: "\(text.count) 字 · \(reason ?? "这个 Agent 没有系统层，开场内容随第一条消息发出")",
                          expanded: $expanded, identifier: "session.attached-context") {
            Text(text)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
        .help("Aeroric 附在这条消息前面的内容，不是你这一轮说的话")
    }
}
