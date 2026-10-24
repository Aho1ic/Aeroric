import AeroricClient
import SwiftUI

// AP7 的八个 Agent 工具区块。逐条对应 P9-04 的验收面:
// 认证(不给 token)、声音(先草稿)。沙箱(不假装就绪)、工具活动(执行者是外部
// Agent)、媒体(策略只用 JSON)、以及会话控制的容器。

/// OAuth 状态。**只显示状态**:token 由既有 owner 保管,不经过这个视图,
/// 也没有任何「复制令牌」入口。
public struct AuthenticationSection: View {
    let status: ExternalProtocolStore.AuthStatus

    public init(status: ExternalProtocolStore.AuthStatus) {
        self.status = status
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("认证").font(.headline)
            switch status.state {
            case .notConfigured:
                Text("未配置。登录会打开系统浏览器或给出设备码,令牌由系统凭据保管器持有。")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("auth.not-configured")
            case let .configured(owner, expiresAt, generation):
                Text("已配置 · 保管者 \(owner) · generation \(generation)")
                    .accessibilityIdentifier("auth.configured")
                if let expiresAt {
                    Text("过期时间 \(Date(timeIntervalSince1970: TimeInterval(expiresAt)).formatted())")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            case let .expired(owner, generation):
                Text("已过期 · 保管者 \(owner) · generation \(generation)。请重新登录。")
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("auth.expired")
            }
            if let url = status.launchURL {
                Text(url).font(.caption).textSelection(.enabled)
                    .accessibilityIdentifier("auth.launch-url")
            }
            if let code = status.userCode {
                Text("设备码 \(code)").font(.system(.body, design: .monospaced))
                    .accessibilityIdentifier("auth.user-code")
            }
            Text("Aeroric 从不把令牌交给界面层。")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// 有效沙箱。requested 与 effective **分开显示**:要求了却没就绪就是不会执行,
/// 这个视图要能解释为什么。
public struct SandboxSection: View {
    let status: ExternalProtocolStore.SandboxStatus

    public init(status: ExternalProtocolStore.SandboxStatus) {
        self.status = status
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("沙箱").font(.headline)
            Text("要求:\(status.requestedEnabled ? "启用" : "关闭") · 后端 \(status.backend)")
                .accessibilityIdentifier("sandbox.requested")
            switch status.state {
            case .ready:
                Text("已就绪:工具在当前沙箱内执行。")
                    .foregroundStyle(.green)
                    .accessibilityIdentifier("sandbox.ready")
            case .disabled:
                Text("未启用沙箱。")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("sandbox.disabled")
            case .notReady:
                Text(status.blockingReason ?? "未就绪")
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("sandbox.not-ready")
            case .unsupported:
                Text(status.blockingReason ?? "平台不支持")
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("sandbox.unsupported")
            }
            if !status.writableRoots.isEmpty {
                Text("可写根:").font(.caption)
                ForEach(status.writableRoots, id: \.self) { root in
                    Text(root).font(.system(.caption, design: .monospaced))
                }
            }
            if !status.allowsToolExecution {
                Text("工具不会执行(fail closed)。")
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("sandbox.fail-closed")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// 外部工具活动。执行者标注为外部 Agent:原生端不是执行者,也没有「代跑」按钮。
public struct ToolActivitySection: View {
    let activities: [ExternalProtocolStore.ToolActivity]
    let handoff: (String) -> String

    public init(
        activities: [ExternalProtocolStore.ToolActivity],
        handoff: @escaping (String) -> String
    ) {
        self.activities = activities
        self.handoff = handoff
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("工具活动").font(.headline)
            if activities.isEmpty {
                Text("暂无外部工具请求。")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("tool-activity.empty")
            }
            ForEach(activities) { activity in
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(activity.tool) · \(activity.summary)")
                    Text("执行者:\(activity.executor)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    ForEach(activity.permissions.sorted(by: { $0.key < $1.key }), id: \.key) { kind, _ in
                        Text("\(kind):\(handoff(kind))")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityIdentifier("tool-activity.\(activity.tool)")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// 浏览器/媒体工具。ACP 协商结果与「音频不被上游支持」时的处理都在这里说清楚:
/// 附件不被支持是**整条消息被拒绝**,不是静默丢附件。
public struct MediaSection: View {
    let acpVersion: UInt8?

    public init(acpVersion: UInt8?) {
        self.acpVersion = acpVersion
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("浏览器与媒体").font(.headline)
            if let acpVersion {
                Text("ACP 已协商到版本 \(acpVersion)")
                    .accessibilityIdentifier("media.acp-version")
            } else {
                Text("ACP 未协商(没有可用对端)。")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("media.acp-none")
            }
            Text("截图/录音权限由外部 Agent 使用,原生端只显示请求、权限与进度;不会代为开启辅助功能或屏幕录制。")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text("上游不支持音频时,带音频的消息会被拒绝,而不是把音轨丢掉继续发。")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("media.audio-refusal-policy")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// 语音输入。**默认关闭**;草稿要显式提交才发送,零音频/被拒/失败都不发空 prompt。
public struct VoiceInputSection: View {
    let voice: ExternalProtocolStore.VoiceState
    let onOptIn: (Bool) -> Void
    let onSubmit: () -> String?
    let onCancel: () -> Void

    public init(
        voice: ExternalProtocolStore.VoiceState,
        onOptIn: @escaping (Bool) -> Void,
        onSubmit: @escaping () -> String?,
        onCancel: @escaping () -> Void
    ) {
        self.voice = voice
        self.onOptIn = onOptIn
        self.onSubmit = onSubmit
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("语音输入").font(.headline)
            Toggle("启用语音输入", isOn: Binding(
                get: { voice.optedIn },
                set: { onOptIn($0) }
            ))
            .accessibilityIdentifier("voice.opt-in")

            if voice.optedIn {
                Text("麦克风:\(voice.micPermission)")
                    .font(.caption)
                    .accessibilityIdentifier("voice.mic-status")
                switch voice.phase {
                case .idle:
                    Text("点开始后按需申请麦克风权限。")
                        .foregroundStyle(.secondary)
                case .recording:
                    Text("录音中…")
                        .accessibilityIdentifier("voice.recording")
                case .draft:
                    TextEditor(text: .constant(voice.transcript))
                        .frame(height: 72)
                        .accessibilityIdentifier("voice.draft")
                    HStack {
                        Button("丢弃") { onCancel() }
                        Spacer()
                        Button("发送") { _ = onSubmit() }
                            .disabled(!voice.canSubmit)
                            .accessibilityIdentifier("voice.submit")
                    }
                case .sent:
                    Text("已发送:\(voice.submittedPrompt ?? "")")
                        .accessibilityIdentifier("voice.sent")
                case .cancelled:
                    Text("已取消,未发送任何内容。")
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("voice.cancelled")
                }
                Text("转写结果先进草稿;麦克风被拒、取消、零音频或转写失败都不会发送空提示。")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// 会话控制区块(面板里的一个 section,与工具条共用 store)。
public struct SessionControlSection: View {
    @Bindable private var store: AgentToolsStore

    public init(store: AgentToolsStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("会话控制").font(.headline)
            if let capabilities = store.capabilities {
                Text("适配器 \(capabilities.adapter)\(capabilities.version.map { " · \($0)" } ?? "")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("session-controls.adapter")
            }
            if store.offeredControls.isEmpty {
                Text(store.capabilities == nil ? "尚未探测到适配器能力" : "当前适配器不提供会话控制")
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("session-controls.none")
            } else {
                ForEach(store.offeredControls, id: \.rawValue) { control in
                    Text(control.rawValue).font(.system(.caption, design: .monospaced))
                        .accessibilityIdentifier("session-controls.\(control.rawValue)")
                }
            }
            if !store.outcomes.isEmpty {
                Text("最近裁决").font(.caption).foregroundStyle(.secondary)
                ForEach(store.outcomes.prefix(5)) { entry in
                    Text("\(entry.method)\(entry.confirmed ? " · 已确认" : "")")
                        .font(.caption2)
                        .accessibilityIdentifier("session-controls.outcome.\(entry.method)")
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
