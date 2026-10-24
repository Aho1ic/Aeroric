import AeroricProtocol
import SwiftUI

/// Settings → Agents entry for the legacy custom Agent cleanup (Q8: disabled with
/// its reason when the host does not offer it).
struct LegacyCleanupEntry: View {
    let store: LegacyCleanupStore
    @State private var isPresented = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text("旧版自定义 Agent")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(AeroricColor.text)
                Text(store.unavailableReason ?? "已迁移到新配置的旧 Agent 可在确认后清理旧启动脚本与凭据文件。")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button("清理…") { isPresented = true }
                .disabled(store.availability != .available)
                .accessibilityIdentifier("settings.legacy-cleanup.open")
        }
        .padding(.horizontal, 12)
        .sheet(isPresented: $isPresented) {
            LegacyCleanupSheet(store: store) { isPresented = false }
        }
    }
}

struct LegacyCleanupSheet: View {
    let store: LegacyCleanupStore
    let close: () -> Void
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("清理旧版自定义 Agent")
                .font(.headline)
            Text("只处理已迁移到新 Agent 配置的旧自定义 Agent，内建 Agent 不受影响。勾选后才会执行，每一项都会在宿主上重新核对。")
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)

            content

            if let error = store.lastError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
            }
            if let batch = store.lastBatch {
                Label("已移入隔离区：数据目录/legacy-agent-quarantine/\(batch)", systemImage: "archivebox")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
            }

            Toggle(
                "我已用新配置启动过所选 Agent，并确认可以正常使用",
                isOn: Binding(get: { store.launchConfirmed }, set: { store.launchConfirmed = $0 })
            )
            .toggleStyle(.checkbox)
            .disabled(store.selection.isEmpty || store.isBusy)
            .accessibilityIdentifier("settings.legacy-cleanup.launch-confirmed")

            HStack {
                Button("刷新") { Task { await store.reload() } }
                    .disabled(store.isBusy || store.availability != .available)
                if store.isBusy { ProgressView().controlSize(.small) }
                Spacer()
                Button("关闭", action: close)
                    .keyboardShortcut(.cancelAction)
                Button("清理所选…") { confirming = true }
                    .disabled(!store.canApply)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("settings.legacy-cleanup.apply")
            }
        }
        .padding(16)
        .frame(width: 520, height: 420)
        .task { await store.reload() }
        .confirmationDialog(
            "清理所选的 \(store.selection.count) 个旧 Agent？",
            isPresented: $confirming,
            titleVisibility: .visible
        ) {
            Button("清理", role: .destructive) { Task { await store.apply() } }
            Button("取消", role: .cancel) {}
        } message: {
            Text("每一项依次：从设置中移除旧档案 → 旧凭据文件移入隔离区 → 旧启动脚本移入隔离区 → 删除旧钥匙串项。隔离区不会自动清空，但不会自动恢复。某一步失败时该项停在“部分完成”，可以稍后继续。")
        }
    }

    @ViewBuilder private var content: some View {
        if let report = store.report {
            if report.candidates.isEmpty {
                Text("没有需要清理的旧版自定义 Agent。")
                    .font(.callout)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(report.candidates) { candidate in
                    LegacyCleanupRow(
                        candidate: candidate,
                        isSelected: store.selection.contains(candidate.source),
                        isSelectable: store.isSelectable(candidate),
                        toggle: { store.toggle(candidate.source) }
                    )
                }
                .listStyle(.inset)
            }
        } else {
            ProgressView("正在读取清理计划…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private struct LegacyCleanupRow: View {
    let candidate: LegacyCleanupCandidate
    let isSelected: Bool
    let isSelectable: Bool
    let toggle: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Toggle("", isOn: Binding(get: { isSelected }, set: { _ in toggle() }))
                .toggleStyle(.checkbox)
                .labelsHidden()
                .disabled(!isSelectable)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(candidate.agentID)
                        .font(.body.monospaced())
                    Text(statusTitle)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(statusColor)
                }
                if let detail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var statusTitle: String {
        switch candidate.state {
        case .eligible: "可清理"
        case .blocked: "已阻止"
        case .partial: "部分完成"
        case .done: "已完成"
        }
    }

    private var statusColor: Color {
        switch candidate.state {
        case .eligible: AeroricColor.accent
        case .blocked: AeroricColor.warning
        case .partial: AeroricColor.warning
        case .done: AeroricColor.success
        }
    }

    private var detail: String? {
        switch candidate.state {
        case .eligible, .done: nil
        case let .blocked(reason): reason
        case let .partial(remaining, reason):
            ["剩余：" + remaining.map(Self.stepTitle).joined(separator: " → "), reason]
                .compactMap { $0 }.joined(separator: "\n")
        }
    }

    private static func stepTitle(_ step: LegacyCleanupStep) -> String {
        switch step {
        case .removeProfile: "移除旧档案"
        case .quarantineSidecar: "隔离旧凭据文件"
        case .quarantineWrapper: "隔离旧启动脚本"
        case .deleteOldSecrets: "删除旧钥匙串项"
        }
    }
}
