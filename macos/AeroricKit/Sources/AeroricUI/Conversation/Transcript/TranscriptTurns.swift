import AeroricProtocol
import AppKit
import SwiftUI

/// One turn of a finished transcript (2026-10-29): the person's message, the
/// work in between (thinking, tool calls, interim replies) and the final
/// reply. The work folds under "已完成，用时 N 秒".
public struct TranscriptTurn: Identifiable, Equatable {
    public var id: String
    public var user: TranscriptItem?
    public var process: [TranscriptItem]
    public var answer: TranscriptItem?
    public var stat: SessionTranscriptStats.TurnStat?
}

public enum TranscriptTurns {
    /// Pure: splits at each user message; the last assistant message with
    /// text in a turn is its answer, everything else before it is process.
    public static func group(_ items: [TranscriptItem]) -> [TranscriptTurn] {
        var turns: [TranscriptTurn] = []
        var current = TranscriptTurn(id: "turn.0", user: nil, process: [], answer: nil)
        func flush() {
            guard current.user != nil || !current.process.isEmpty else { return }
            if let index = current.process.lastIndex(where: hasAnswerText) {
                current.answer = current.process[index]
                current.process.remove(at: index)
            }
            turns.append(current)
        }
        for item in items {
            if case let .message(id, role, _) = item, role == "user" {
                flush()
                current = TranscriptTurn(id: "turn.\(id)", user: item, process: [], answer: nil)
            } else {
                current.process.append(item)
            }
        }
        flush()
        return turns
    }

    /// Pure: attaches each turn's timing, matched from the newest turn back
    /// (the transcript and the history may disagree on early turns, e.g. a
    /// resumed session's preamble).
    public static func attach(_ stats: [SessionTranscriptStats.TurnStat], to turns: [TranscriptTurn]) -> [TranscriptTurn] {
        var out = turns
        var statIndex = stats.count - 1
        for index in out.indices.reversed() where out[index].user != nil {
            guard statIndex >= 0 else { break }
            out[index].stat = stats[statIndex]
            statIndex -= 1
        }
        return out
    }

    static func hasAnswerText(_ item: TranscriptItem) -> Bool {
        guard case let .message(_, role, content) = item, role != "user" else { return false }
        return content.contains { if case let .text(text) = $0 { return !text.isEmpty }; return false }
    }

    /// The plain text a copy button puts on the pasteboard.
    public static func text(of item: TranscriptItem?) -> String {
        guard case let .message(_, _, content)? = item else { return "" }
        return content.compactMap { if case let .text(text) = $0 { return text }; return nil }
            .joined(separator: "\n\n")
    }

    /// `已完成，用时 13 秒`; `已完成` without timing.
    public static func header(_ stat: SessionTranscriptStats.TurnStat?) -> String {
        guard let stat, let ended = stat.endedAt, ended >= stat.startedAt else { return "已完成" }
        return "已完成，用时 \(SessionStatsText.duration(ended - stat.startedAt))"
    }

    /// `思考 · 3 步工具调用`.
    public static func processSummary(_ process: [TranscriptItem]) -> String {
        var thought = false
        var steps = 0
        for item in process {
            switch item {
            case let .message(_, _, content):
                if content.contains(where: { if case .thinking = $0 { return true }; return false }) { thought = true }
            case .tool: steps += 1
            case let .toolGroup(_, calls): steps += calls.count
            }
        }
        return [thought ? "思考" : nil, steps > 0 ? "\(steps) 步工具调用" : nil].compactMap { $0 }.joined(separator: " · ")
    }

    /// `11:47` today, `10/28 16:25` on another day.
    public static func clock(_ ms: UInt64, now: Date = Date()) -> String {
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.dateFormat = Calendar.current.isDate(date, inSameDayAs: now) ? "HH:mm" : "M/d HH:mm"
        return formatter.string(from: date)
    }

    static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

/// A finished turn, drawn like the reference the owner chose (2026-10-29):
/// the person's bubble with time and copy (on hover, always on the latest),
/// "已完成，用时 N 秒 ⌄" over a hairline with the work folded inside, the
/// reply, then copy · 用量 · time.
struct TranscriptTurnView<Row: View>: View {
    let turn: TranscriptTurn
    let column: CGFloat
    let isLatest: Bool
    @Binding var expanded: Bool
    let row: (TranscriptItem) -> Row
    @State private var userHovered = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            if let user = turn.user {
                VStack(alignment: .trailing, spacing: AeroricSpace.xs) {
                    row(user)
                    HStack(spacing: AeroricSpace.l) {
                        if let started = turn.stat?.startedAt { Text(TranscriptTurns.clock(started)) }
                        copyButton(TranscriptTurns.text(of: user))
                    }
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.trailing, AeroricSpace.s)
                    .opacity(isLatest || userHovered ? 1 : 0)
                }
                .frame(maxWidth: .infinity, alignment: .trailing)
                .contentShape(Rectangle())
                .onHover { userHovered = $0 }
            }
            if !turn.process.isEmpty || turn.stat?.endedAt != nil {
                header
            }
            if expanded {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    ForEach(turn.process) { item in row(item) }
                }
            }
            if let answer = turn.answer {
                row(answer)
                actions(answer)
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Button { withAnimation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion)) {
                expanded.toggle()
            } } label: {
                HStack(spacing: AeroricSpace.s) {
                    Text(TranscriptTurns.header(turn.stat))
                        .foregroundStyle(AeroricColor.textSecondary)
                    if !turn.process.isEmpty {
                        Image(systemName: "chevron.down")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .rotationEffect(.degrees(expanded ? 180 : 0))
                        let summary = TranscriptTurns.processSummary(turn.process)
                        if !summary.isEmpty {
                            Text(summary).foregroundStyle(AeroricColor.textTertiary)
                                .font(.system(size: AeroricType.caption))
                        }
                    }
                }
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .fixedSize()
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(turn.process.isEmpty)
            .accessibilityIdentifier("session.turn.header")
            HairlineDivider()
        }
    }

    private func actions(_ answer: TranscriptItem) -> some View {
        HStack(spacing: AeroricSpace.l) {
            copyButton(TranscriptTurns.text(of: answer))
            if let tokens = turn.stat?.tokens, tokens > 0 {
                Label("用量 \(SessionStatsText.compact(tokens)) tok", systemImage: "cylinder")
                    .labelStyle(.titleAndIcon)
                    .monospacedDigit()
            }
            if let ended = turn.stat?.endedAt { Text(TranscriptTurns.clock(ended)) }
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.textTertiary)
        .lineLimit(1)
        .fixedSize()
    }

    private func copyButton(_ text: String) -> some View {
        CopyGlyphButton(text: text)
    }
}

/// Copy icon that turns into a check for a moment.
struct CopyGlyphButton: View {
    let text: String
    @State private var copied = false

    var body: some View {
        Button {
            TranscriptTurns.copy(text)
            copied = true
            Task { try? await Task.sleep(for: .seconds(1.2)); copied = false }
        } label: {
            Image(systemName: copied ? "checkmark" : "doc.on.doc")
                .font(.system(size: 12))
                .frame(width: 18, height: 18)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("复制")
        .accessibilityLabel("复制")
    }
}
