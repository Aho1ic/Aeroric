import AeroricClient
import SwiftUI

/// AP7:会话控制条。
///
/// ## 只显示被广告的控制
///
/// 每一项的可用性来自[宿主探测结果](`AgentToolsStore.capabilities`),没有探测到
/// 就不渲染——不是渲染成灰色让用户去猜。不可用的原因由
/// [`AgentToolsStore.reason(for:)`] 给出,UI 直接展示这个理由。
///
/// ## 危险动作先给证据
///
/// 工作区 revert 需要先看 diff 并确认;公开 share 需要显式 endpoint + 确认。两个
/// 门槛都是**禁用按钮**而不是「点了再报错」:提交前 store 就构造不出请求
/// (`makeRequest` 返回 nil)。
public struct SessionControlBar: View {
    @Bindable private var store: AgentToolsStore
    /// 请求执行器:宿主调用(注入,便于测试与离线预览)。
    private let perform: ((AdapterClient.ControlRequest) async throws -> AdapterClient.ControlOutcome)?
    /// 工作区改动预览(由调用方提供真实 diff;没有 diff 就不能确认)。
    public var workspaceDiff: [String] = []

    @State private var showRevertSheet = false
    @State private var showShareSheet = false
    @State private var previewConfirmed = false
    @State private var shareIsPublic = false
    @State private var shareEndpoint = ""
    @State private var shareConfirmed = false
    @State private var busy = false

    public init(
        store: AgentToolsStore,
        workspaceDiff: [String] = [],
        perform: ((AdapterClient.ControlRequest) async throws -> AdapterClient.ControlOutcome)? = nil
    ) {
        self.store = store
        self.workspaceDiff = workspaceDiff
        self.perform = perform
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if store.offeredControls.isEmpty {
                Text(store.capabilities == nil ? "尚未探测到适配器能力" : "当前适配器不提供会话控制")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("session-controls.none")
            } else {
                HStack(spacing: 8) {
                    ForEach(store.offeredControls, id: \.rawValue) { control in
                        controlButton(control)
                    }
                }
            }
            if store.permissionDrift {
                Text("此任务按入队时的权限(\(store.queuedPermissionMode))执行;界面已改为 \(store.currentPermissionMode),宿主会拒绝按新权限继续。")
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("session-controls.permission-drift")
            }
            if let refusal = store.lastRefusal {
                Text(refusal)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("session-controls.refusal")
            }
        }
        .sheet(isPresented: $showRevertSheet) { revertSheet }
        .sheet(isPresented: $showShareSheet) { shareSheet }
    }

    @ViewBuilder
    private func controlButton(_ control: AdapterClient.CompanionControlName) -> some View {
        let title: LocalizedStringKey = switch control {
        case .plan: "计划模式"
        case .compact: "压缩上下文"
        case .fork: "分叉会话"
        case .revert: "回退"
        case .share: "分享"
        }
        Button(title) {
            switch control {
            case .revert:
                previewConfirmed = false
                showRevertSheet = true
            case .share:
                shareIsPublic = false
                shareConfirmed = false
                showShareSheet = true
            default:
                submit(control)
            }
        }
        .disabled(busy)
        .help(store.reason(for: control) ?? "")
        .accessibilityIdentifier("session-control.\(control.rawValue)")
    }

    private var revertSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("回退会话").font(.headline)
            Text("会话回退与工作区回退是两件事。工作区回退会改动文件,必须先看改动。")
                .font(.caption)
                .foregroundStyle(.secondary)
            if workspaceDiff.isEmpty {
                Text("没有待回退的工作区改动,将只回退会话历史。")
                    .accessibilityIdentifier("session-revert.no-diff")
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(workspaceDiff, id: \.self) { line in
                            Text(line).font(.system(.caption, design: .monospaced))
                        }
                    }
                }
                .frame(maxHeight: 200)
                .accessibilityIdentifier("session-revert.diff")
                Toggle("我已查看上述改动", isOn: $previewConfirmed)
                    .accessibilityIdentifier("session-revert.confirm-preview")
            }
            HStack {
                Button("取消") { showRevertSheet = false }
                Spacer()
                Button("回退") {
                    showRevertSheet = false
                    submit(.revert, workspacePaths: workspaceDiff, previewConfirmed: previewConfirmed)
                }
                .disabled(!workspaceDiff.isEmpty && !previewConfirmed)
                .accessibilityIdentifier("session-revert.submit")
            }
        }
        .padding(20)
        .frame(width: 520)
    }

    private var shareSheet: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("分享会话").font(.headline)
            Text("默认本地导出,凭据类字段与主目录路径会被脱敏。公开上传不可逆。")
                .font(.caption)
                .foregroundStyle(.secondary)
            Toggle("上传到公开地址", isOn: $shareIsPublic)
                .accessibilityIdentifier("session-share.public")
            if shareIsPublic {
                TextField("https://…", text: $shareEndpoint)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("session-share.endpoint")
                Toggle("我确认上传不可逆", isOn: $shareConfirmed)
                    .accessibilityIdentifier("session-share.confirm")
            }
            HStack {
                Button("取消") { showShareSheet = false }
                Spacer()
                Button("分享") {
                    showShareSheet = false
                    submit(
                        .share,
                        shareIsPublic: shareIsPublic,
                        shareEndpoint: shareEndpoint,
                        confirmed: shareConfirmed
                    )
                }
                .disabled(shareIsPublic && (!shareConfirmed || shareEndpoint.isEmpty))
                .accessibilityIdentifier("session-share.submit")
            }
        }
        .padding(20)
        .frame(width: 520)
    }

    private func submit(
        _ control: AdapterClient.CompanionControlName,
        workspacePaths: [String] = [],
        previewConfirmed: Bool = false,
        shareIsPublic: Bool = false,
        shareEndpoint: String? = nil,
        confirmed: Bool = false
    ) {
        guard let request = store.makeRequest(
            control: control,
            sessionID: store.activeSessionID,
            workspacePaths: workspacePaths,
            previewConfirmed: previewConfirmed,
            shareIsPublic: shareIsPublic,
            shareEndpoint: shareEndpoint,
            confirmed: confirmed
        ) else { return }
        guard let perform else {
            // 没有执行器(离线预览):不发请求,也不假装成功。
            return
        }
        busy = true
        Task {
            do {
                let outcome = try await perform(request)
                store.record(outcome, confirmed: confirmed || previewConfirmed)
            } catch {
                store.recordRefusal(error)
            }
            busy = false
        }
    }
}

/// AP7:Agent 工具工作区面板(8 视图的容器)。
///
/// 惯例与 `TaskWorkspaceView` 的其它面板一致:一个 section 一个 store,
/// 视图只读 store。这里列的是「外部 Agent 正在做什么」,不是「Aeroric 能做什么」
/// ——执行者永远是外部 Agent。
public struct AgentToolsPanel: View {
    @Bindable private var store: ExternalProtocolStore
    @Bindable private var controls: AgentToolsStore

    public init(store: ExternalProtocolStore, controls: AgentToolsStore) {
        self.store = store
        self.controls = controls
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                AuthenticationSection(status: store.auth)
                SandboxSection(status: store.sandbox)
                ToolActivitySection(activities: store.activities, handoff: store.permissionHandoff(for:))
                MediaSection(acpVersion: store.acpVersion)
                VoiceInputSection(
                    voice: store.voice,
                    onOptIn: { store.setVoiceOptIn($0) },
                    onSubmit: { store.submitVoiceDraft() },
                    onCancel: { store.cancelVoice() }
                )
                SessionControlSection(store: controls)
            }
            .padding(16)
        }
        .accessibilityIdentifier("agent-tools.panel")
    }
}
