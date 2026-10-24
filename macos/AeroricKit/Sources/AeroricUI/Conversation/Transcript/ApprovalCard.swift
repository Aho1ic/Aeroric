import SwiftUI

/// Inline approval / question card at the end of the transcript (replaces
/// the old sheets). Same store and outcomes; the host's resolved event is
/// what removes it, so a submitted card stays visible but disabled.
struct ApprovalCard: View {
    let request: PendingRequestStore.PendingRequest
    let store: PendingRequestStore
    let onSubmit: (String, PendingRequestStore.Outcome) -> Void

    private var submitted: Bool { store.isSubmitted(eventId: request.id) }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        VStack(alignment: .leading, spacing: 0) {
            Rectangle().fill(AeroricColor.warning).frame(height: 3)
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                switch request.kind {
                case .approval: approval
                case .question:
                    QuestionSheet(request: request, store: store, onSubmit: onSubmit, onDismiss: {})
                        .padding(-16)
                        .padding(.vertical, AeroricSpace.xs)
                }
            }
            .padding(AeroricSpace.l)
        }
        .background(AeroricColor.layer1)
        .clipShape(shape)
        .overlay { shape.strokeBorder(AeroricColor.separator2, lineWidth: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.approval.\(request.id)")
    }

    @ViewBuilder private var approval: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "exclamationmark.shield.fill").foregroundStyle(AeroricColor.warning)
            Text("需要你的批准")
                .font(.system(size: AeroricType.body, weight: .semibold))
            Text(request.toolName ?? "工具")
                .font(.system(size: AeroricType.secondary, design: .monospaced))
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("approval.tool-name")
        }
        if let reason = request.reason, !reason.isEmpty {
            Text(reason)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .accessibilityIdentifier("approval.reason")
        }
        HStack(spacing: AeroricSpace.m) {
            Button("交给下一个应答者") { onSubmit(request.id, .next) }
                .buttonStyle(.link)
                .font(.system(size: AeroricType.caption))
                .accessibilityIdentifier("approval.next")
            Spacer()
            if submitted {
                Text("已提交，等待宿主确认…")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("approval.submitted")
            }
            Button("拒绝") { onSubmit(request.id, .approvalRejected) }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .accessibilityIdentifier("approval.reject")
            Button("批准一次") { onSubmit(request.id, .approvalAllowedOnce) }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .accessibilityIdentifier("approval.allow-once")
        }
        .disabled(submitted)
    }
}
