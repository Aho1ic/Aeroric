import SwiftUI
import AeroricProtocol

/// B4′ (2026-10-30): the right sidebar's 进度 page — the task's checkpoint
/// as the host derived it, plus the few things a person adds by hand: a
/// constraint, an obligation settled, a side branch.
public struct CheckpointPanel: View {
    let checkpoint: LiveCheckpoint?
    let errorText: String?
    /// This task is a side branch (it can bring its conclusion back).
    let isBranch: Bool
    let onAddConstraint: (String) -> Void
    let onSetObligation: (_ id: String, _ state: String) -> Void
    let onStartBranch: () -> Void
    let onReturnBranch: () -> Void
    let onOpenTask: (String) -> Void
    @State private var constraint = ""

    public init(checkpoint: LiveCheckpoint?, errorText: String?, isBranch: Bool,
                onAddConstraint: @escaping (String) -> Void,
                onSetObligation: @escaping (_ id: String, _ state: String) -> Void,
                onStartBranch: @escaping () -> Void, onReturnBranch: @escaping () -> Void,
                onOpenTask: @escaping (String) -> Void) {
        self.checkpoint = checkpoint
        self.errorText = errorText
        self.isBranch = isBranch
        self.onAddConstraint = onAddConstraint
        self.onSetObligation = onSetObligation
        self.onStartBranch = onStartBranch
        self.onReturnBranch = onReturnBranch
        self.onOpenTask = onOpenTask
    }

    public var body: some View {
        ScrollView(showsIndicators: false) {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                Text(CheckpointPanelText.header(checkpoint))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                if let errorText {
                    CheckpointRow(symbol: "exclamationmark.triangle", text: errorText, highlight: AeroricColor.danger) {
                        EmptyView()
                    }
                }
                if let checkpoint, !checkpoint.isBlank {
                    CheckpointFacts(checkpoint: checkpoint)
                    obligationControls(checkpoint)
                    if !checkpoint.branches.isEmpty {
                        CheckpointSectionLabel(title: "侧支")
                        ForEach(checkpoint.branches, id: \.taskId) { branch in
                            Button { onOpenTask(branch.taskId) } label: {
                                CheckpointRow(symbol: "arrow.triangle.branch", text: "\(branch.title) · \(branch.summary)") {
                                    Image(systemName: "chevron.right")
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                }
                            }
                            .buttonStyle(.plain)
                        }
                    }
                } else if checkpoint != nil {
                    Text("这个会话还没有可记录的进展。回合结束后这里会列出目标、改动和未完成的事。")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, AeroricSpace.s)
                }
                CheckpointSectionLabel(title: "加一条约束")
                HStack(spacing: AeroricSpace.m) {
                    CapsuleField(placeholder: "例如：不要推送", text: $constraint, height: 26)
                        .onSubmit(addConstraint)
                    Button("添加", action: addConstraint)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
                HStack(spacing: AeroricSpace.m) {
                    Button("开侧支", action: onStartBranch)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .frame(maxWidth: .infinity)
                        .help("从当前对话分出一个子会话处理局部问题，结论再带回这里")
                        .accessibilityIdentifier("progress.branch.start")
                    if isBranch {
                        Button("带回结论", action: onReturnBranch)
                            .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                            .frame(maxWidth: .infinity)
                            .help("把这个侧支的最后一条回答作为结论交回主会话")
                            .accessibilityIdentifier("progress.branch.return")
                    }
                }
                .padding(.top, AeroricSpace.m)
            }
            .padding(AeroricSpace.l)
        }
    }

    /// Settle an accepted criterion by hand (when no command decides it).
    @ViewBuilder private func obligationControls(_ checkpoint: LiveCheckpoint) -> some View {
        let open = checkpoint.obligations.filter { $0.source != "plan" && $0.state != "passed" }
        if !open.isEmpty {
            CheckpointSectionLabel(title: "手动核销")
            ForEach(open, id: \.id) { obligation in
                HStack(spacing: AeroricSpace.s) {
                    Text(obligation.text)
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .help(obligation.text)
                    Button("通过") { onSetObligation(obligation.id, "passed") }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                    Button("未通过") { onSetObligation(obligation.id, "failed") }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
            }
        }
    }

    private func addConstraint() {
        let text = constraint.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        onAddConstraint(text)
        constraint = ""
    }
}

public enum CheckpointPanelText {
    /// "检查点 · 回合 7 结束 · 10:42", or why there is none yet.
    public static func header(_ checkpoint: LiveCheckpoint?, now: Date = Date()) -> String {
        guard let checkpoint else { return "正在读取检查点…" }
        guard checkpoint.updatedMs > 0 else { return "检查点 · 尚未开始" }
        let date = Date(timeIntervalSince1970: TimeInterval(checkpoint.updatedMs) / 1000)
        let time = Calendar.current.isDate(date, inSameDayAs: now)
            ? date.formatted(date: .omitted, time: .shortened)
            : date.formatted(date: .abbreviated, time: .shortened)
        return "检查点 · 回合 \(checkpoint.turn) 结束 · \(time)"
    }
}

extension LiveCheckpoint {
    /// Nothing worth showing yet.
    var isBlank: Bool {
        goal == nil && decisions.isEmpty && effects.isEmpty && unknown.isEmpty && intents.isEmpty
            && obligations.isEmpty && constraints.isEmpty && branches.isEmpty
    }
}
