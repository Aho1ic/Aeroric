import AeroricProtocol
import SwiftUI

/// 2026-10-30: an imported session drawn like a live session's history —
/// turns with their work folded, copy on hover, the footer fading the page
/// out — with where it came from at the top left.
struct ImportedTranscript<Footer: View>: View {
    let book: ImportBook
    let sessionID: String
    let badge: String
    /// The agent's own session id, when the file name carries one.
    let agentSessionID: String?
    @ViewBuilder let footer: () -> Footer
    @State private var messages: [SessionMessage] = []
    /// Grouped once per read, not per body (a turn's expand toggle re-ran it).
    @State private var turns: [TranscriptTurn] = []
    @State private var loading = false
    @State private var errorText: String?
    @State private var expandedTurns: Set<String> = []
    /// A6: how many host pages the read takes; older ones are left; what did not read.
    @State private var pages = ImportBook.maxHistoryPages
    @State private var hasEarlier = false
    @State private var unreadable: SessionUnreadable?
    /// B2-3: only the newest read writes back.
    @State private var reads = HistoryReads()

    private static var bottomID: String { "imported.bottom" }

    var body: some View {
        GeometryReader { proxy in
            let column = TranscriptGeometry.columnWidth(for: proxy.size.width)
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 40) {
                        if hasEarlier || unreadable != nil {
                            EarlierHistoryBar(hasEarlier: hasEarlier, loading: loading, unreadable: unreadable) {
                                let shown = pages
                                pages += ImportBook.maxHistoryPages
                                load(keepingOnFailure: shown)
                            }
                        }
                        if turns.isEmpty {
                            AeroricEmptyState(systemImage: loading ? "hourglass" : "text.bubble",
                                              title: loading ? "正在读取会话历史…" : "这个会话没有可显示的内容",
                                              message: nil)
                                .padding(.top, 60)
                        }
                        ForEach(turns) { turn in
                            TranscriptTurnView(
                                turn: turn, column: column, isLatest: turn.id == turns.last?.id,
                                expanded: Binding(get: { expandedTurns.contains(turn.id) },
                                                  set: { if $0 { expandedTurns.insert(turn.id) } else { expandedTurns.remove(turn.id) } }),
                                row: { item in row(item, column: column) })
                        }
                        Color.clear.frame(height: 1).id(Self.bottomID)
                    }
                    .frame(width: column)
                    .padding(.vertical, AeroricSpace.xxxl)
                    .frame(maxWidth: .infinity)
                }
                .scrollIndicators(.hidden)
                .modifier(OpensAtLatest())
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    footer().frame(width: column).padding(.bottom, AeroricSpace.xs).frame(maxWidth: .infinity)
                        .background(alignment: .top) { FooterFade() }
                }
                .onChange(of: messages.count) { old, _ in
                    if old == 0 { scroller.scrollTo(Self.bottomID, anchor: .bottom) }
                }
            }
        }
        .background(AeroricColor.canvas)
        .overlay(alignment: .topLeading) { badgeView }
        .overlay(alignment: .topTrailing) { moreMenu }
        .alert("操作失败", isPresented: Binding(get: { errorText != nil }, set: { if !$0 { errorText = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(errorText ?? "")
        }
        .task(id: sessionID) { load() }
        .accessibilityIdentifier("session.view")
    }

    @ViewBuilder private func row(_ item: TranscriptItem, column: CGFloat) -> some View {
        switch item {
        case let .message(_, role, content):
            MessageBubble(role: role, content: content, columnWidth: column)
        case let .tool(call):
            ToolCallLine(call: call, index: 0)
        case let .toolGroup(_, calls):
            ToolGroupLine(calls: calls, firstIndex: 0)
        }
    }

    private var badgeView: some View {
        Label(badge, systemImage: "square.and.arrow.down")
            .font(.system(size: AeroricType.caption, weight: .semibold))
            .foregroundStyle(AeroricColor.accent)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, AeroricSpace.m)
            .padding(.vertical, 3)
            .background(AeroricColor.accent.opacity(0.12), in: Capsule(style: .continuous))
            .padding(AeroricSpace.m)
            .accessibilityIdentifier("imported.source")
    }

    /// No 导出 here: an import has nothing of Aeroric's own to export.
    private var moreMenu: some View {
        Menu {
            if let agentSessionID {
                Button("复制 Session ID", systemImage: "number") { TranscriptTurns.copy(agentSessionID) }
            }
            Button("重新读取历史", systemImage: "arrow.clockwise") { load() }
        } label: {
            Image(systemName: "ellipsis").frame(width: 26, height: 26)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .padding(AeroricSpace.m)
        .accessibilityLabel("会话操作")
        .accessibilityIdentifier("session.more")
    }

    /// B2-3: a newer read replaces an older one; a failed "load earlier"
    /// goes back to the depth that was shown (`keepingOnFailure`).
    private func load(keepingOnFailure shown: Int? = nil) {
        loading = true
        let (book, sessionID, pages) = (book, sessionID, pages)
        reads.begin { read in
            Task { @MainActor in
                do {
                    let history = try await book.history(id: sessionID, pages: pages)
                    guard reads.lands(read) else { return }
                    messages = history.messages
                    hasEarlier = history.hasEarlier
                    unreadable = history.unreadable
                    turns = TranscriptTurns.group(TranscriptBuilder.items(from: messages, taskRunning: false))
                } catch {
                    guard reads.lands(read) else { return }
                    if let shown { self.pages = shown }
                    if HistoryReads.shows(error) { errorText = "读取历史失败：\(error)" }
                }
                if reads.lands(read) { loading = false }
            }
        }
    }
}
