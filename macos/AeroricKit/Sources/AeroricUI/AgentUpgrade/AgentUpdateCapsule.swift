import AeroricClient
import SwiftUI

/// The session page's new-version reminder (2026-10-27): a Liquid Glass capsule
/// floating at the top of the conversation (system material before macOS 26).
/// 下个版本前不再提醒 · 一键升级 · ··· (永久不提醒); a required upgrade cannot be
/// dismissed. Shows progress in place, then fades out three seconds after success.
struct AgentUpdateCapsule: View {
    let book: AgentUpgradeBook
    let family: String?
    @State private var dismissedJob: UInt64?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var harness: AgentUpgradeClient.Harness? { book.harness(forFamily: family) }

    var body: some View {
        let reminder = AgentUpdateReminder.make(harness, dismissedJob: dismissedJob)
        ZStack {
            if let reminder, let harness {
                capsule(reminder, agent: harness.agent, finishedAt: harness.job?.finishedAtMs)
                    .transition(reduceMotion ? .opacity : .move(edge: .top).combined(with: .opacity))
            }
        }
        .padding(.top, AeroricSpace.m)
        .animation(.spring(response: 0.38, dampingFraction: 0.86), value: reminder)
    }

    @ViewBuilder
    private func capsule(_ reminder: AgentUpdateReminder, agent: String, finishedAt: UInt64?) -> some View {
        HStack(spacing: AeroricSpace.m) {
            switch reminder {
            case let .available(title, latest, installed):
                Image(systemName: "sparkles").foregroundStyle(AeroricColor.accent)
                Text("\(title) \(Text(latest).fontWeight(.semibold)) 可用")
                Text("当前 \(installed)").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                Button("下个版本前不再提醒") { Task { await book.setReminder(agent, .skipVersion) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .accessibilityIdentifier("session.update.skip")
                Button { Task { await book.upgrade(agent) } } label: { Label("一键升级", systemImage: "arrow.down.circle") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .accessibilityIdentifier("session.update.upgrade")
                Menu {
                    Button("永久不提醒") { Task { await book.setReminder(agent, .off) } }
                } label: {
                    Image(systemName: "ellipsis").frame(width: 22, height: 22).contentShape(Circle())
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("更多")
                .accessibilityLabel("更多提醒选项")
            case let .required(title, installed, minimum):
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(AeroricColor.warning)
                Text("\(title) \(installed) 太旧，需要 \(minimum) 或更高才能继续对话")
                Button { Task { await book.upgrade(agent) } } label: { Label("一键升级", systemImage: "arrow.down.circle") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .accessibilityIdentifier("session.update.required")
            case let .upgrading(title):
                ProgressView().controlSize(.small)
                Text("正在升级 \(title)…")
                ProgressView().progressViewStyle(.linear).frame(width: 90).tint(AeroricColor.accent)
            case let .done(title, version):
                Image(systemName: "checkmark.circle.fill").foregroundStyle(AeroricColor.success)
                Text("已升级到 \(title) \(version)，新会话会使用新版本")
                    .task(id: finishedAt) {
                        try? await Task.sleep(for: .seconds(3))
                        dismissedJob = finishedAt
                    }
            case let .failed(_, message):
                Image(systemName: "exclamationmark.circle.fill").foregroundStyle(AeroricColor.danger)
                Text("升级没有完成：\(message)").lineLimit(1).truncationMode(.tail).frame(maxWidth: 360)
                Button("重试") { Task { await book.upgrade(agent) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                Button { dismissedJob = finishedAt } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("关闭")
            }
        }
        .font(.system(size: AeroricType.secondary))
        .padding(.leading, AeroricSpace.l)
        .padding(.trailing, AeroricSpace.s)
        .padding(.vertical, AeroricSpace.xs + 1)
        .frame(minHeight: 36)
        .glassSurface(.floatingCard, in: Capsule(style: .continuous))
        .overlay {
            if case .required = reminder {
                Capsule(style: .continuous).strokeBorder(AeroricColor.warning.opacity(0.55), lineWidth: 1)
            }
        }
        .shadow(color: AeroricColor.shadow, radius: 12, y: 4)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.update.capsule")
    }
}
