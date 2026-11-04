import AeroricProtocol
import SwiftUI

/// R2-1 (2026-11-04): which recorded turn a transcript turn is, and the words.
public enum TurnLedgerText {
    /// The record whose start falls inside this turn (±5 s, both clocks are
    /// this Mac's); none when nothing fits — no buttons rather than a guess.
    public static func record(for stat: SessionTranscriptStats.TurnStat?,
                              in records: [LiveTurnRecord]) -> LiveTurnRecord? {
        guard let stat else { return nil }
        let slack: UInt64 = 5000
        let low = stat.startedAt > slack ? stat.startedAt - slack : 0
        let high = (stat.endedAt ?? stat.startedAt) + slack
        return records
            .filter { $0.startedMs >= low && $0.startedMs <= high }
            .min { distance($0.startedMs, stat.startedAt) < distance($1.startedMs, stat.startedAt) }
    }

    private static func distance(_ a: UInt64, _ b: UInt64) -> UInt64 { a > b ? a - b : b - a }

    public static func badge(_ state: String) -> String {
        switch state {
        case "accepted": "已接受"
        case "undone": "已撤销"
        default: "待验收"
        }
    }

    /// `2 个文件 +18 −4`.
    public static func summary(_ record: LiveTurnRecord) -> String {
        var parts = ["\(record.files.count) 个文件"]
        if record.additions > 0 { parts.append("+\(record.additions)") }
        if record.deletions > 0 { parts.append("−\(record.deletions)") }
        return parts.joined(separator: " ")
    }

    public static func pending(_ records: [LiveTurnRecord]) -> Int {
        records.filter { $0.state == "pending" }.count
    }

    public static let footnote = "只恢复 git 能看到的文件；被忽略的文件和工作目录以外的改动不会回退。"
    public static let running = "这一轮结束后可撤销"
}

/// What a turn header offers for its recorded changes.
struct TurnLedgerControls {
    let record: LiveTurnRecord
    let running: Bool
    let onAccept: () -> Void
    let onUndo: () -> Void
    let onRedo: () -> Void
    let onRestore: () -> Void
}

/// The badge, the summary and the buttons on a finished turn's header line.
struct TurnLedgerAccessory: View {
    let controls: TurnLedgerControls

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            CheckpointBadge(text: TurnLedgerText.badge(controls.record.state), tint: tint)
            Text(TurnLedgerText.summary(controls.record))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .fixedSize()
            Spacer(minLength: AeroricSpace.s)
            switch controls.record.state {
            case "pending":
                button("接受", id: "accept", action: controls.onAccept)
                button("撤销", id: "undo", action: controls.onUndo)
                button("重做", id: "redo", action: controls.onRedo)
            case "accepted":
                button("撤销", id: "undo", action: controls.onUndo)
            case "undone":
                button("撤回撤销", id: "restore", action: controls.onRestore)
            default:
                EmptyView()
            }
        }
    }

    private var tint: Color {
        switch controls.record.state {
        case "accepted": AeroricColor.success
        case "undone": AeroricColor.textTertiary
        default: AeroricColor.warning
        }
    }

    private func button(_ title: String, id: String, action: @escaping () -> Void) -> some View {
        let locked = controls.running && id != "accept"
        return Button(title, action: action)
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .fixedSize()
            .disabled(locked)
            .help(locked ? TurnLedgerText.running : title)
            .accessibilityIdentifier("session.turn.\(id)")
    }
}

/// An undo (or redo, or taking an undo back) waiting for the user's yes.
struct TurnUndoRequest: Identifiable {
    var id: String { "\(turnId).\(action)" }
    let turnId: String
    /// `undo` / `restore`.
    let action: String
    let plan: LiveTurnPlan
    /// Redo: the turn's own words go back into the composer afterwards.
    let redoText: String?
}

/// The confirmation: what goes back, what is skipped and why.
struct TurnUndoSheet: View {
    let request: TurnUndoRequest
    let onConfirm: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text(title)
                .font(.system(size: AeroricType.body, weight: .semibold))
            if let blocked = request.plan.blocked {
                Text(blocked)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    Button("知道了") { dismiss() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                }
            } else {
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        if !request.plan.files.isEmpty {
                            CheckpointSectionLabel(title: request.action == "undo" ? "恢复到这一轮之前" : "回到这一轮的结果")
                            ForEach(request.plan.files, id: \.self) { file in
                                row(symbol: file.delete ? "trash" : "arrow.uturn.backward", path: file.path,
                                    note: file.delete ? "（会删除）" : nil, warning: false)
                            }
                        }
                        if !request.plan.skipped.isEmpty {
                            CheckpointSectionLabel(title: "跳过")
                            ForEach(request.plan.skipped, id: \.self) { file in
                                row(symbol: "exclamationmark.triangle", path: file.path,
                                    note: "· \(file.reason)", warning: true)
                            }
                        }
                        if request.plan.files.isEmpty {
                            Text("没有可以恢复的文件。")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                        }
                    }
                }
                .frame(maxHeight: 280)
                Text(TurnLedgerText.footnote)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: AeroricSpace.m) {
                    Spacer()
                    Button("取消") { dismiss() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .keyboardShortcut(.cancelAction)
                    Button(confirmTitle) { onConfirm(); dismiss() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .disabled(request.plan.files.isEmpty)
                        .accessibilityIdentifier("session.turn.confirm")
                }
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 440)
    }

    private var title: String {
        switch (request.action, request.redoText != nil) {
        case ("restore", _): "撤回这一轮的撤销？"
        case (_, true): "撤销这一轮，再重做？"
        default: "撤销这一轮的改动？"
        }
    }

    private var confirmTitle: String {
        request.action == "restore" ? "确认撤回" : "确认撤销"
    }

    private func row(symbol: String, path: String, note: String?, warning: Bool) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: symbol)
                .font(.system(size: AeroricType.caption))
                .frame(width: 16)
            Text(path)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.middle)
            if let note {
                Text(note)
                    .font(.system(size: AeroricType.caption))
                    .lineLimit(1)
                    .fixedSize()
            }
            Spacer(minLength: 0)
        }
        .foregroundStyle(warning ? AeroricColor.warning : AeroricColor.text)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 28)
        .background(Capsule(style: .continuous).fill((warning ? AeroricColor.warning : AeroricColor.layer2)
                .opacity(warning ? 0.12 : 1)))
    }
}

/// "N 轮待验收" over the conversation; a click goes to the earliest one.
struct TurnPendingCapsule: View {
    let count: Int
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text("\(count) 轮待验收")
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(AeroricColor.warning)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 24)
                .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.14)))
        }
        .buttonStyle(.plain)
        .fixedSize()
        .help("跳到最早一轮待验收")
        .accessibilityIdentifier("session.turns.pending")
    }
}
