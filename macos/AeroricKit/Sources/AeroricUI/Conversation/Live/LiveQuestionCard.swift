import SwiftUI
import AeroricProtocol

/// An agent's question that is not a tool approval (omp extension dialogs,
/// 2026-10-27 design): violet top bar, the options as rows (1–9 pick), 其他
/// for free text, a countdown when the agent set a timeout, 跳过 / 确定.
/// `confirm` is 是 / 否; `input` / `editor` are a text field / editor.
struct LiveQuestionCard: View {
    let question: LivePendingQuestion
    let submitting: Bool
    let onAnswer: (_ value: String?, _ confirmed: Bool?, _ cancelled: Bool) -> Void

    @State private var choice: Int?
    @State private var custom = false
    @State private var text = ""
    @FocusState private var focused: Bool

    private var tint: Color { AeroricColor.violet }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        VStack(alignment: .leading, spacing: 0) {
            Rectangle().fill(tint).frame(height: 3)
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: "questionmark.bubble.fill").foregroundStyle(tint)
                    Text("omp 想问你").font(.system(size: AeroricType.body, weight: .semibold))
                    Spacer()
                    if let timeout = question.timeoutMs, question.askedAtMs > 0 {
                        LiveCountdown(deadlineMs: question.askedAtMs + timeout)
                    }
                }
                Text(question.title)
                    .font(.system(size: AeroricType.body))
                    .foregroundStyle(AeroricColor.text)
                    .textSelection(.enabled)
                if let message = question.message, !message.isEmpty {
                    Text(message).font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
                        .textSelection(.enabled)
                }
                content
                buttons
            }
            .padding(AeroricSpace.l)
        }
        .background(AeroricColor.layer1)
        .clipShape(shape)
        .overlay { shape.strokeBorder(AeroricColor.separator2, lineWidth: 1) }
        .disabled(submitting)
        .onAppear { text = question.style == "editor" ? (question.placeholder ?? "") : "" }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.live.question.\(question.id)")
    }

    @ViewBuilder private var content: some View {
        switch question.style {
        case "select":
            VStack(spacing: AeroricSpace.xs) {
                ForEach(Array(question.options.enumerated()), id: \.offset) { index, option in
                    optionRow(index: index, title: option)
                }
                Button {
                    custom = true
                    choice = nil
                    focused = true
                } label: {
                    HStack(spacing: AeroricSpace.m) {
                        Image(systemName: "pencil").frame(width: 14)
                        if custom {
                            TextField("自己填写", text: $text).textFieldStyle(.plain).focused($focused)
                                .onSubmit(submit)
                        } else {
                            Text("其他，自己填写…")
                        }
                        Spacer()
                    }
                    .foregroundStyle(custom ? AeroricColor.text : AeroricColor.textTertiary)
                    .modifier(OptionRowChrome(selected: custom))
                }
                .buttonStyle(.plain)
            }
        case "input":
            TextField(question.placeholder ?? "输入回答", text: $text)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit(submit)
        case "editor":
            TextEditor(text: $text)
                .font(.system(size: AeroricType.body))
                .frame(minHeight: 80, maxHeight: 200)
                .padding(AeroricSpace.xs)
                .background(AeroricColor.canvas, in: AeroricRadius.shape(AeroricRadius.s))
                .overlay { AeroricRadius.shape(AeroricRadius.s).strokeBorder(AeroricColor.separator2) }
        default:
            EmptyView()
        }
    }

    private func optionRow(index: Int, title: String) -> some View {
        let selected = choice == index
        return Button {
            choice = index
            custom = false
        } label: {
            HStack(spacing: AeroricSpace.m) {
                Circle()
                    .strokeBorder(selected ? AeroricColor.accent : AeroricColor.separator4, lineWidth: selected ? 4 : 1.5)
                    .frame(width: 14, height: 14)
                Text(title).foregroundStyle(AeroricColor.text).multilineTextAlignment(.leading)
                Spacer()
                if index < 9 {
                    Text("\(index + 1)").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .modifier(OptionRowChrome(selected: selected))
        }
        .buttonStyle(.plain)
        .modifier(DigitShortcut(index: index))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    @ViewBuilder private var buttons: some View {
        HStack(spacing: AeroricSpace.m) {
            if submitting {
                Text("已提交，等待 omp 确认…").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            }
            Spacer()
            Button("跳过") { onAnswer(nil, nil, true) }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .accessibilityIdentifier("session.live.question.skip")
            if question.style == "confirm" {
                Button("否") { onAnswer(nil, false, false) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("是") { onAnswer(nil, true, false) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("session.live.question.yes")
            } else {
                Button("确定", action: submit)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .disabled(answer == nil)
                    .accessibilityIdentifier("session.live.question.submit")
            }
        }
    }

    /// The value 确定 sends, nil while nothing is chosen.
    private var answer: String? {
        if question.style == "select" && !custom {
            return choice.flatMap { question.options.indices.contains($0) ? question.options[$0] : nil }
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty && question.style != "editor" ? nil : text
    }

    private func submit() {
        guard let answer else { return }
        onAnswer(answer, nil, false)
    }
}

/// Digits 1–9 pick the first nine options.
private struct DigitShortcut: ViewModifier {
    let index: Int

    func body(content: Content) -> some View {
        if index < 9 {
            content.keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [])
        } else {
            content
        }
    }
}

private struct OptionRowChrome: ViewModifier {
    let selected: Bool

    func body(content: Content) -> some View {
        let shape = AeroricRadius.shape(AeroricRadius.s)
        content
            .font(.system(size: AeroricType.body))
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.s + 1)
            .background(selected ? AeroricColor.selection : Color.clear, in: shape)
            .overlay { shape.strokeBorder(selected ? AeroricColor.accent.opacity(0.5) : AeroricColor.separator2, lineWidth: 1) }
            .contentShape(shape)
    }
}

/// "30 秒后按默认处理", ticking each second.
private struct LiveCountdown: View {
    let deadlineMs: UInt64

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let left = max(0, Int((Double(deadlineMs) / 1000 - context.date.timeIntervalSince1970).rounded(.up)))
            Text(left > 0 ? "\(left) 秒后按默认处理" : "即将按默认处理")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .monospacedDigit()
        }
    }
}

/// "已按本会话授权自动批准 · bash · npm run lint · 撤销授权" (dashed outline).
struct LiveAutoApprovedRow: View {
    let kind: String
    let summary: String
    let revoked: Bool
    let onRevoke: () -> Void

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.s)
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "checkmark.circle").foregroundStyle(AeroricColor.success)
            Text("已按本会话授权自动批准")
            Text("\(kind) · \(summary.split(separator: "\n").first.map(String.init) ?? summary)")
                .font(AeroricType.mono(AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: AeroricSpace.m)
            if revoked {
                Text("已撤销，下次会再问你").foregroundStyle(AeroricColor.textTertiary)
            } else {
                Button("撤销授权", action: onRevoke)
                    .buttonStyle(.borderless)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("session.live.auto-approved.revoke")
            }
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.textSecondary)
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .overlay { shape.strokeBorder(AeroricColor.separator3, style: StrokeStyle(lineWidth: 1, dash: [4, 3])) }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("session.live.auto-approved")
    }
}

/// A quiet line from the agent (compaction, retries, plugin notes).
struct LiveInfoRow: View {
    let level: String
    let message: String

    var body: some View {
        Label(message, systemImage: level == "info" ? "info.circle" : "exclamationmark.triangle")
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(level == "error" ? AeroricColor.danger : level == "warning" ? AeroricColor.warning : AeroricColor.textTertiary)
            .textSelection(.enabled)
            .accessibilityIdentifier("session.live.info")
    }
}

/// Under a message sent during a turn: "插话 · 当前命令跑完后送达" → "已插话".
struct LiveSteerLabel: View {
    let steer: LiveSteer

    var body: some View {
        switch steer {
        case .none:
            EmptyView()
        case .waiting:
            HStack(spacing: AeroricSpace.xs) {
                Image(systemName: "arrow.turn.down.right")
                Text("插话 · 当前命令跑完后送达")
                Circle().fill(AeroricColor.accent).frame(width: 5, height: 5)
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textTertiary)
            .accessibilityIdentifier("session.live.steer.waiting")
        case .delivered:
            Label("已插话", systemImage: "checkmark")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
    }
}
