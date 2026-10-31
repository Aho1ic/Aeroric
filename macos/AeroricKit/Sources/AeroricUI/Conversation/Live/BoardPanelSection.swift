import SwiftUI
import AeroricProtocol

/// B4-3: what the progress page knows about the board a task works on.
public struct BoardPanelModel: Equatable, Sendable {
    public var board: LiveBoard
    /// The board is the parent's (this task is a delegated child).
    public var shared: Bool

    public init(board: LiveBoard, shared: Bool = false) {
        self.board = board
        self.shared = shared
    }
}

public enum BoardPanelAction: Equatable, Sendable {
    case add(kind: String, text: String)
    case setState(id: String, state: String)
    case decide(id: String, accept: Bool)
}

struct BoardPanelSection: View {
    let model: BoardPanelModel
    let onAction: (BoardPanelAction) -> Void
    @State private var kind = "hypothesis"
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            CheckpointSectionLabel(title: model.shared ? "黑板 · 主会话的" : "黑板")
            ForEach(BoardText.ordered(model.board.entries)) { entry in
                row(entry)
            }
            HStack(spacing: AeroricSpace.m) {
                CapsulePicker(title: BoardText.kind(kind), systemImage: BoardText.symbol(kind)) {
                    ForEach(BoardText.kinds, id: \.self) { k in
                        Button(BoardText.kind(k)) { kind = k }
                    }
                }
                CapsuleField(placeholder: "记一条", text: $text, height: 26)
                    .onSubmit(add)
                    .accessibilityIdentifier("progress.board.text")
                Button("添加", action: add)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
    }

    private func add() {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        onAction(.add(kind: kind, text: trimmed))
        text = ""
    }

    @ViewBuilder private func row(_ entry: LiveBoardEntry) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: BoardText.symbol(entry.kind))
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: 16)
            Text(BoardText.line(entry))
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help("\(entry.text)\n\(BoardText.author(entry.author))")
            if entry.kind == "decision" && entry.state == "proposed" {
                Button("采纳") { onAction(.decide(id: entry.id, accept: true)) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .accessibilityIdentifier("progress.board.accept")
                Button("否决") { onAction(.decide(id: entry.id, accept: false)) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
            } else if let next = BoardText.next(entry) {
                Menu {
                    ForEach(next, id: \.self) { state in
                        Button(BoardText.state(state).0) { onAction(.setState(id: entry.id, state: state)) }
                    }
                } label: {
                    let (word, tint) = BoardText.state(entry.state)
                    CheckpointBadge(text: word, tint: tint)
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
            } else {
                let (word, tint) = BoardText.state(entry.state)
                CheckpointBadge(text: word, tint: tint)
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 30)
        .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
    }
}

public enum BoardText {
    public static let kinds = ["goal", "hypothesis", "claim", "decision", "evidence", "question"]

    public static func kind(_ kind: String) -> String {
        switch kind {
        case "goal": return "目标"
        case "hypothesis": return "假设"
        case "claim": return "认领"
        case "decision": return "决定"
        case "evidence": return "证据"
        case "question": return "问题"
        default: return kind
        }
    }

    public static func symbol(_ kind: String) -> String {
        switch kind {
        case "goal": return "scope"
        case "hypothesis": return "questionmark.diamond"
        case "claim": return "hand.raised"
        case "decision": return "checkmark.seal"
        case "evidence": return "doc.text.magnifyingglass"
        default: return "questionmark.bubble"
        }
    }

    public static func state(_ state: String) -> (String, Color) {
        switch state {
        case "confirmed": return ("成立", AeroricColor.success)
        case "refuted": return ("不成立", AeroricColor.textSecondary)
        case "proposed": return ("提议", AeroricColor.warning)
        case "accepted": return ("已采纳", AeroricColor.success)
        case "rejected": return ("已否决", AeroricColor.textSecondary)
        case "answered": return ("已答", AeroricColor.success)
        case "done": return ("完成", AeroricColor.success)
        default: return ("待处理", AeroricColor.accent)
        }
    }

    /// The states an entry can move to by hand (decisions go through 采纳/否决).
    public static func next(_ entry: LiveBoardEntry) -> [String]? {
        switch entry.kind {
        case "hypothesis": return ["open", "confirmed", "refuted"].filter { $0 != entry.state }
        case "claim": return ["open", "done"].filter { $0 != entry.state }
        case "question": return ["open", "answered"].filter { $0 != entry.state }
        default: return nil
        }
    }

    /// "假设 · 是 DNS 慢" / "认领 · 查 hyper（task-4）".
    public static func line(_ entry: LiveBoardEntry) -> String {
        var out = "\(kind(entry.kind)) · \(entry.text)"
        if let member = entry.claimedBy { out += "（\(member)）" }
        return out
    }

    public static func author(_ author: String) -> String {
        author == "user" ? "你写的" : "会话 \(author) 写的"
    }

    /// Goals first, then open questions and proposed decisions, then the rest.
    public static func ordered(_ entries: [LiveBoardEntry]) -> [LiveBoardEntry] {
        func rank(_ e: LiveBoardEntry) -> Int {
            if e.kind == "goal" { return 0 }
            if e.state == "proposed" || (e.kind == "question" && e.state == "open") { return 1 }
            return 2
        }
        return entries.enumerated().sorted { a, b in
            rank(a.element) != rank(b.element) ? rank(a.element) < rank(b.element) : a.offset < b.offset
        }.map(\.element)
    }

    /// The attention row's detail.
    public static func attention(_ item: LiveBoardAttention) -> String {
        item.reason == "decision" ? "决定待确认：\(item.entry.text)" : "认领久未更新：\(item.entry.text)"
    }

    /// One line for a delegation's probe hits.
    public static func probes(_ probes: [LiveDelegationProbe]) -> String? {
        let open = probes.filter { !$0.acknowledged }
        guard !open.isEmpty else { return nil }
        return open.map(\.detail).joined(separator: "；")
    }
}
