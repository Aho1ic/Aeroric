import SwiftUI
import AeroricProtocol

/// Inline approval (Q10): allow once / always allow in this session / deny
/// with an optional reason. Removed only when the host resolves it.
struct LiveApprovalCard: View {
    let request: LivePendingPermission
    let phase: LiveApprovalMachine.Phase
    let enabled: Bool
    /// e.g. "omp · bash", shown at the top right.
    var agentLabel: String? = nil
    let onDecide: (LiveDecision) -> Void
    let onBeginDeny: () -> Void
    let onReason: (String) -> Void
    let onCancelDeny: () -> Void

    private var title: String {
        switch request.kind {
        case "command": "需要你批准运行命令"
        case "fileChange": "需要你批准修改文件"
        case "permissions": "需要你批准额外权限"
        case "Bash", "bash", "eval": "需要你批准运行命令"
        case "Edit", "MultiEdit", "Write", "NotebookEdit", "edit", "write", "ast_edit", "apply_patch": "需要你批准修改文件"
        default: "需要你批准：\(request.kind)"
        }
    }

    private var submitting: Bool {
        if case .submitting = phase { true } else { false }
    }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        VStack(alignment: .leading, spacing: 0) {
            Rectangle().fill(AeroricColor.warning).frame(height: 3)
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: "exclamationmark.shield.fill").foregroundStyle(AeroricColor.warning)
                    Text(title).font(.system(size: AeroricType.body, weight: .semibold))
                    if let agentLabel {
                        Spacer()
                        Text(agentLabel).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                Text(request.summary)
                    .font(AeroricType.mono(AeroricType.code))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .accessibilityIdentifier("session.live.approval.summary")
                if case let .denying(reason) = phase {
                    TextField("拒绝理由（可空，会告诉 Agent）", text: Binding(get: { reason }, set: onReason))
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { onDecide(.deny) }
                        .accessibilityIdentifier("session.live.approval.reason")
                }
                if case let .failed(_, message) = phase {
                    Text("提交失败：\(message)，可重试")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.danger)
                }
                buttons
            }
            .padding(AeroricSpace.l)
        }
        .background(AeroricColor.layer1)
        .clipShape(shape)
        .overlay { shape.strokeBorder(AeroricColor.separator2, lineWidth: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.live.approval.\(request.id)")
    }

    @ViewBuilder private var buttons: some View {
        HStack(spacing: AeroricSpace.m) {
            if submitting {
                Text("已提交，等待 Agent 确认…")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            Spacer()
            if case .denying = phase {
                Button("取消") { onCancelDeny() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("确认拒绝") { onDecide(.deny) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .accessibilityIdentifier("session.live.approval.deny-confirm")
            } else {
                Button("拒绝") { onBeginDeny() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .accessibilityIdentifier("session.live.approval.deny")
                if request.options.contains(LiveDecision.allowForSession.rawValue) {
                    Button("本会话总是允许") { onDecide(.allowForSession) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                        .accessibilityIdentifier("session.live.approval.allow-session")
                }
                Button("批准一次") { onDecide(.allowOnce) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("session.live.approval.allow-once")
            }
        }
        .disabled(!enabled || submitting)
    }
}
