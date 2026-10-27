import AeroricClient
import SwiftUI

/// Agent 配置 → Agent 升级 (2026-10-27): one rounded card per harness with its
/// installed and latest version, install channel, the 会话中提醒 switch and
/// upgrade / install; a running job unfolds its output under the card.
struct AgentUpgradePane: View {
    let book: AgentUpgradeBook?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                if let book, book.isAvailable {
                    header(book)
                    ForEach(book.snapshot?.agents ?? []) { harness in
                        AgentUpgradeCard(harness: harness, book: book)
                    }
                    if let error = book.errorText {
                        Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
                    }
                } else {
                    AeroricEmptyState(systemImage: "arrow.up.circle", title: "宿主暂不支持 Agent 升级",
                                      message: "更新 Aeroric 后即可在这里升级 Agent。")
                        .padding(.top, 80)
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .task { await book?.refresh() }
        .accessibilityIdentifier("agent-config.upgrades")
    }

    private func header(_ book: AgentUpgradeBook) -> some View {
        HStack(spacing: AeroricSpace.m) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Agent 升级").font(.system(size: AeroricType.headline, weight: .semibold))
                Text(summary(book)).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary)
            }
            Spacer()
            Button { Task { await book.checkUpdates() } } label: {
                if book.checking {
                    HStack(spacing: AeroricSpace.xs) { ProgressView().controlSize(.mini); Text("检查中…") }
                } else {
                    Label("检查更新", systemImage: "arrow.clockwise")
                }
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
            .disabled(book.checking)
            .accessibilityIdentifier("agent-upgrade.check")
            Button("全部升级") { Task { await book.upgradeAll() } }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .disabled(book.upgradable.isEmpty)
                .accessibilityIdentifier("agent-upgrade.all")
        }
    }

    private func summary(_ book: AgentUpgradeBook) -> String {
        var parts: [String] = []
        if let at = book.snapshot?.checkedAtMs {
            parts.append("上次检查 \(RelativeTime.short(Date(timeIntervalSince1970: Double(at) / 1000)))")
        } else {
            parts.append("还没有检查过新版本")
        }
        let count = book.upgradable.count
        parts.append(count == 0 ? "都是最新" : "\(count) 个可升级")
        if book.snapshot?.checkError != nil { parts.append("部分版本没查到") }
        return parts.joined(separator: " · ")
    }
}

private struct AgentUpgradeCard: View {
    let harness: AgentUpgradeClient.Harness
    let book: AgentUpgradeBook

    private var running: Bool { harness.job?.state == .running }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.l) {
                AgentGlyph(agent: harness.agent, size: 16)
                    .frame(width: 36, height: 36)
                    .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.m))
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: AeroricSpace.s) {
                        Text(harness.title).font(.system(size: AeroricType.body, weight: .semibold))
                        chip
                    }
                    Text(detail).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: AeroricSpace.m)
                if harness.installed != nil {
                    Toggle("会话中提醒", isOn: Binding(
                        get: { harness.remind },
                        set: { on in Task { await book.setReminder(harness.agent, on ? .on : .off) } }
                    ))
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("agent-upgrade.\(harness.agent).remind")
                }
                action
            }
            if let job = harness.job, job.state != .succeeded || running, !job.log.isEmpty || job.message != nil {
                log(job)
            }
        }
        .padding(AeroricSpace.l)
        .configSurface(AeroricRadius.l)
        .clipShape(shape)
        .animation(.easeOut(duration: AeroricMotion.standard), value: harness.job)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("agent-upgrade.\(harness.agent)")
    }

    private var detail: String {
        let channel = channelTitle
        switch harness.status {
        case .notInstalled:
            return ["最新 \(harness.latest ?? "—")", "将通过 npm 安装"].joined(separator: " · ")
        case .updateAvailable, .required:
            return ["\(harness.installed ?? "") → \(harness.latest ?? "")", channel].filter { !$0.isEmpty }.joined(separator: " · ")
        default:
            return [harness.installed ?? "", channel].filter { !$0.isEmpty }.joined(separator: " · ")
        }
    }

    private var channelTitle: String {
        switch harness.channel {
        case "npm": "npm"
        case "homebrew": "Homebrew"
        case "standalone": harness.agent == "omp" ? "官方脚本（omp update）" : "官方安装"
        case "": ""
        default: harness.channel
        }
    }

    @ViewBuilder private var chip: some View {
        if running {
            StatusChip(text: harness.job?.installing == true ? "安装中" : "升级中", tint: AeroricColor.running)
        } else if harness.job?.state == .failed {
            StatusChip(text: "失败", systemImage: "exclamationmark", tint: AeroricColor.danger)
        } else {
            switch harness.status {
            case .updateAvailable: StatusChip(text: "有新版本", tint: AeroricColor.accent)
            case .required: StatusChip(text: "需要升级", systemImage: "exclamationmark", tint: AeroricColor.warning)
            case .upToDate: StatusChip(text: "已是最新", systemImage: "checkmark", tint: AeroricColor.success)
            case .notInstalled: StatusChip(text: "未安装", tint: AeroricColor.danger)
            case .unknown: StatusChip(text: "未知最新版本", tint: AeroricColor.textTertiary)
            }
        }
    }

    @ViewBuilder private var action: some View {
        if running {
            ProgressView().controlSize(.small).frame(width: 60)
        } else {
            switch harness.status {
            case .notInstalled:
                Button("安装") { Task { await book.upgrade(harness.agent) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .accessibilityIdentifier("agent-upgrade.\(harness.agent).install")
            case .updateAvailable, .required:
                Button(harness.job?.state == .failed ? "重试" : "升级") { Task { await book.upgrade(harness.agent) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .accessibilityIdentifier("agent-upgrade.\(harness.agent).upgrade")
            default:
                Text("已是最新").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    .padding(.horizontal, AeroricSpace.m)
            }
        }
    }

    private func log(_ job: AgentUpgradeClient.Job) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(job.log.suffix(8).enumerated()), id: \.offset) { _, line in
                Text(line).lineLimit(1).truncationMode(.middle)
            }
            if job.state == .failed, let message = job.message {
                Text(message).foregroundStyle(AeroricColor.danger)
            }
        }
        .font(AeroricType.mono(AeroricType.caption))
        .foregroundStyle(AeroricColor.textSecondary)
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(AeroricSpace.m)
        .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.m))
        .padding(.leading, 48)
    }
}
