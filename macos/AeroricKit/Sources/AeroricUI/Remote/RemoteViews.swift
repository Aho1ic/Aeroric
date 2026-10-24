import SwiftUI

import AeroricClient

/// P8-03 Action 4：远程访问四视图（RemoteAccessSettingsView / PairingView /
/// DeviceListView / RemoteAuditView）。服务启停与安全默认同 core 控制面
/// （loopback 起步、配对/公网端点显式扩宽——G07 语义）；审计面只读。
/// 状态经 RemoteStore（UDS `remote.*` 九方法），Swift 不复制校验。

// MARK: - Store

/// 远程 plane 的视图状态。connection 注入便于预览与测试；refresh 是唯一
/// 权威拉取点（start/stop/config 后回读 current_status 对账）。
@MainActor
public final class RemoteStore: ObservableObject {
    @Published public private(set) var status: RemoteClient.RemoteStatus
    @Published public private(set) var devices: [RemoteClient.DeviceView]
    @Published public private(set) var pairing: RemoteClient.PairingInvite?
    @Published public private(set) var lastError: String?
    @Published public private(set) var readState: WorkspaceReadState
    /// A ready main connection does not imply an assembled remote client.
    public var canControl: Bool { client != nil && readState == .loaded }
/// 审计行(`<ts>\t<event>\t<payload>` 的原始文本;只读展示)。
    @Published public private(set) var auditLines: [String]
    /// AP6:推送登记状态(设备表 + 计数 + 退避窗口)。
    @Published public private(set) var pushStatus: RemoteClient.PushStatus = .unavailable()
    /// AP6:自托管 relay 的最近一次诊断结果。
    @Published public private(set) var relayDiagnostics: RemoteClient.RelayDiagnostics?
    /// 产品方关于 relay 责任的原话(不提供默认 relay);由宿主给出,这里不改写。
    @Published public private(set) var relayDisclosure: String = ""

    private let client: RemoteClient?
    /// 审计文件读取钩子（生产=DataDir/remote/audit.log；测试注入内存行）。
    private let auditReader: () -> [String]

    public init(
        client: RemoteClient?,
        status: RemoteClient.RemoteStatus = .offline(),
        devices: [RemoteClient.DeviceView] = [],
        auditLines: [String] = []
    ) {
        self.client = client
        self.readState = client == nil ? .unavailable : .notLoaded
        self.status = status
        self.devices = devices
        self.auditLines = auditLines
        self.auditReader = { [] }
    }

    /// 生产构造：审计行从磁盘读（调用方给读取闭包，避免 Kit 依赖路径解析）。
    public init(client: RemoteClient?, auditReader: @escaping () -> [String]) {
        self.client = client
        self.readState = client == nil ? .unavailable : .notLoaded
        self.status = .offline()
        self.devices = []
        self.auditLines = auditReader()
        self.auditReader = auditReader
    }

/// 刷新状态+设备表(+AP6 推送状态)。
    public func refresh() async {
        guard let client = clientForWrite() else { return }
        readState = .loading
        do {
            status = try await client.status()
            devices = try await client.listDevices()
            // 推送状态与远程面同批读:RPC 失败不该让整个面板空白,所以单独兜住。
            pushStatus = (try? await client.pushStatus()) ?? pushStatus
            lastError = nil
            readState = .loaded
        } catch {
            lastError = String(describing: error)
            readState = .failed(String(describing: error))
        }
        auditLines = auditReader()
    }

    // MARK: - AP6: push plane and relay diagnostics

    /// 注销某台设备的推送。
    ///
    /// 与「撤销设备」是两件事:撤销会断连并收窄 listener,注销只是不再给它推
    /// 通知。分开做,不借撤销的语义。
    public func unregisterPush(deviceID: String) async {
        guard let client = clientForWrite() else { return }
        do {
            try await client.pushRegister(deviceID: deviceID, platform: "ios", token: "")
            pushStatus = try await client.pushStatus()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 立刻推一条,验证「宿主 → 发送端 → APNs → 这台手机」整条链路。
    public func sendTestNotification() async {
        guard let client = clientForWrite() else { return }
        do {
            _ = try await client.pushTest()
            pushStatus = try await client.pushStatus()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 跑一次自托管 relay 诊断(真发一次 WS 注册)。
    public func diagnoseRelay() async {
        guard let client = clientForWrite() else { return }
        do {
            relayDiagnostics = try await client.relayDiagnostics()
            relayDisclosure = try await client.relayDisclosure().note
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 启动服务（可选端口）。
    public func start(port: Int?) async {
        guard let client = clientForWrite() else { return }
        do {
            status = try await client.start(port: port)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 停止服务。
    public func stop() async {
        guard let client = clientForWrite() else { return }
        do {
            status = try await client.stop()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 生成配对二维码（成功后 status 回读以反映可能的宽绑定）。
    public func createInvite() async {
        guard let client = clientForWrite() else { return }
        do {
            pairing = try await client.createInvite()
            status = try await client.status()
            lastError = nil
        } catch {
            pairing = nil
            lastError = String(describing: error)
        }
    }

    /// 清掉已展示的配对码（用户离开配对页时；invite 本身有过期）。
    public func dismissPairing() {
        pairing = nil
    }

    /// 撤销设备。
    public func revoke(id: String) async {
        guard let client = clientForWrite() else { return }
        do {
            try await client.revokeDevice(id: id)
            devices = try await client.listDevices()
            status = try await client.status()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// 更新 relay/公网端点配置。
    public func updateConfig(
        relayURL: String?, relayToken: String?, publicEndpoints: [String]?
    ) async {
        guard let client = clientForWrite() else { return }
        do {
            status = try await client.updateConfig(
                relayURL: relayURL, relayToken: relayToken, publicEndpoints: publicEndpoints
            )
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    private func clientForWrite() -> RemoteClient? {
        guard let client else {
            lastError = "未连接宿主"
            return nil
        }
        return client
    }
}

// MARK: - RemoteAccessSettingsView（服务开关＋relay 配置入口）

/// 远程访问设置主页：服务开关（回环起步）、relay/公网端点、
/// 设备列表与配对入口的导航容器。
public struct RemoteAccessSettingsView: View {
    @ObservedObject var store: RemoteStore
    @State private var relayURL: String = ""
    @State private var relayToken: String = ""
    @State private var showPairing = false
    /// 最近一次推送动作的结果提示(`store.lastError` 负责失败;这里只说成功)。
    @State private var pushNotice: String?

    public init(store: RemoteStore) {
        self.store = store
    }

    public var body: some View {
        Form {
            Section("远控能力") {
                Text(store.readState.notice ?? "已读取远控服务与设备状态")
                    .font(.callout)
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("remote.capability")
            }
            Section("服务") {
                Toggle("启用远程访问", isOn: Binding(
                    get: { store.status.enabled && store.status.running },
                    set: { on in
                        Task {
                            if on { await store.start(port: nil) } else { await store.stop() }
                        }
                    }
                ))
                .disabled(!store.canControl)
                .accessibilityIdentifier("remote.enable")
                HStack {
                    Text(store.readState == .loaded ? (store.status.running ? "运行中" : "已停止") : "服务状态未核实")
                        .font(.caption)
                        .foregroundStyle(store.status.running ? AeroricColor.success : AeroricColor.textSecondary)
                    if store.readState == .loaded {
                        Text(":\(store.status.port)")
                        .font(.caption.monospaced())
                    }
                    if store.status.networkExposed {
                        Text("已暴露到网络")
                            .font(.caption)
                            .foregroundStyle(AeroricColor.warning)
                    }
                }
                .accessibilityIdentifier("remote.status-line")
                Button("生成配对二维码") { showPairing = true }
                    .disabled(!store.canControl || !store.status.running)
                    .accessibilityIdentifier("remote.pair-button")
            }

            Section("Relay / 公网端点") {
                TextField("Relay URL（wss://，留空不用 relay）", text: $relayURL)
                    .disabled(!store.canControl)
                    .accessibilityIdentifier("remote.relay-url")
                SecureField("Relay 共享口令", text: $relayToken)
                    .disabled(!store.canControl)
                    .accessibilityIdentifier("remote.relay-token")
                Button("保存配置") {
                    Task {
                        await store.updateConfig(
                            relayURL: relayURL.isEmpty ? nil : relayURL,
                            relayToken: relayToken.isEmpty ? nil : relayToken,
                            publicEndpoints: nil
                        )
                    }
                }
                .disabled(!store.canControl)
                .accessibilityIdentifier("remote.config-save")
            }

            DeviceListView(store: store)

            if let error = store.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("remote.error")
            }
        }
        .formStyle(.grouped)
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityIdentifier("settings.remote")
        .task { await store.refresh() }
        .sheet(isPresented: $showPairing) {
            PairingView(store: store)
        }
    }

    // MARK: - AP6: push section

    /// 推送分区。三条要点都在界面上说清楚,因为它们决定用户会不会误以为
    /// 「没收到通知 = 没发生事情」:
    /// * 通知是提醒,不是审批入口;审批永远回连接里做。
    /// * APNs 尽力投递,不承诺必达。
    /// * 发送端是用户自己部署的,私钥不经过本机。
    @ViewBuilder
    private var pushSection: some View {
        HStack {
            Text(store.pushStatus.wired ? "推送可用" : "此宿主未装配推送出口")
                .font(.caption)
                .foregroundStyle(store.pushStatus.wired ? AeroricColor.success : AeroricColor.warning)
            Spacer()
            Text(store.pushStatus.enabled ? "已配置发送端" : "未配置发送端")
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
        }
        .accessibilityIdentifier("push.summary")

        if let senderURL = store.pushStatus.senderURL {
            LabeledContent("发送端", value: senderURL)
                .accessibilityIdentifier("push.sender-url")
        }
        Text(store.pushStatus.detail)
            .font(.caption)
            .foregroundStyle(AeroricColor.textSecondary)
            .accessibilityIdentifier("push.detail")

        if let suppressed = store.pushStatus.suppressedForMs {
            Text("发送端要求退避,约 \(suppressed / 1000) 秒内不再推送(APNs 限流/不可用)")
                .font(.caption)
                .foregroundStyle(AeroricColor.warning)
                .accessibilityIdentifier("push.backoff")
        }

        if store.pushStatus.devices.isEmpty {
            Text("还没有设备登记推送;在手机上打开伴随端并允许通知后即可上报。")
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("push.no-devices")
        } else {
            ForEach(store.pushStatus.devices, id: \.deviceID) { device in
                HStack {
                    VStack(alignment: .leading) {
                        Text(device.deviceID)
                            .accessibilityIdentifier("push.device.\(device.deviceID)")
                        Text(device.invalidated
                             ? "APNs 已判该令牌失效,需在手机上重新允许通知"
                             : "已登记(\(device.platform))")
                            .font(.caption)
                            .foregroundStyle(device.invalidated ? AeroricColor.warning : AeroricColor.textSecondary)
                    }
                    Spacer()
                    Button("取消推送") {
                        Task { await store.unregisterPush(deviceID: device.deviceID) }
                    }
                    .accessibilityIdentifier("push.unregister.\(device.deviceID)")
                }
            }
        }

        Button("发送测试通知") {
            Task {
                await store.sendTestNotification()
                pushNotice = "已请求发送端投递;通知不承诺必达,以连接内的状态为准"
            }
        }
        .disabled(!store.pushStatus.wired || store.pushStatus.devices.isEmpty)
        .accessibilityIdentifier("push.test")

        let stats = store.pushStatus.stats
        Text("本机计数:判定 \(stats.decisions) · 尝试 \(stats.attempts) · 已投递 \(stats.accepted) · 失效 \(stats.invalidatedDevices) · 失败 \(stats.failures) · 无设备 \(stats.skippedNoDevices)")
            .font(.caption.monospaced())
            .foregroundStyle(AeroricColor.textSecondary)
            .accessibilityIdentifier("push.stats")

        Text("通知只提示「任务完成/失败/等待审批」,不含标题、路径或命令;它不是审批授权,请回到已连接的会话确认。")
            .font(.caption)
            .foregroundStyle(AeroricColor.textSecondary)

        if let notice = pushNotice {
            Text(notice)
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("push.notice")
        }
        if let error = store.lastError {
            Text(error)
                .font(.caption)
                .foregroundStyle(AeroricColor.danger)
                .accessibilityIdentifier("push.error")
        }
    }

    // MARK: - AP6: relay diagnostics

    /// 诊断结果行。四项分开显示而不是只给一句成败:用户要据此判断是
    /// 「地址/口令写错了」还是「网络到不了」还是「反向代理没升级 WebSocket」。
    @ViewBuilder
    private func relayDiagnosticsRow(_ diagnostics: RemoteClient.RelayDiagnostics) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            LabeledContent("方案", value: diagnostics.schemeOK ? "合法(wss:// 或本机 ws://)" : "不合法")
            LabeledContent("可达", value: diagnostics.reachable ? "是" : "否")
            LabeledContent("口令被接受", value: diagnostics.authenticated ? "是" : "否")
            if let latency = diagnostics.latencyMs {
                LabeledContent("耗时", value: "\(latency) ms")
            }
            Text(diagnostics.detail)
                .font(.caption)
                .foregroundStyle(diagnostics.authenticated ? AeroricColor.success : AeroricColor.warning)
        }
        .accessibilityIdentifier("remote.relay-result")
    }
}

// MARK: - PairingView（二维码内容＋端点展示；QR 渲染本身由调用方注入）

/// 配对视图：展示 pairingUrl（桌面壳负责把 URL 渲染成 QR 图像——Kit 只出
/// 文本与复制按钮，避免引入 QR 依赖）。
public struct PairingView: View {
    @ObservedObject var store: RemoteStore
    @Environment(\.dismiss) private var dismiss
    @State private var copied = false

    public init(store: RemoteStore) {
        self.store = store
    }

    public var body: some View {
        VStack(spacing: 12) {
            if let pairing = store.pairing {
                Text("手机扫码或复制下面的链接完成配对")
                    .font(.headline)
                Text(pairing.pairingURL)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                    .lineLimit(3)
                    .accessibilityIdentifier("remote.pairing-url")
                Text("端点：\(pairing.endpoint)")
                    .font(.caption)
                    .accessibilityIdentifier("remote.pairing-endpoint")
                Text("\(pairing.expiresInSeconds) 秒内有效")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textSecondary)
                Button(copied ? "已复制" : "复制链接") {
                    #if canImport(AppKit)
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(pairing.pairingURL, forType: .string)
                    #endif
                    copied = true
                }
                .accessibilityIdentifier("remote.pairing-copy")
            } else {
                Text("正在生成配对码…")
                if let error = store.lastError {
                    Text(error).font(.caption).foregroundStyle(AeroricColor.danger)
                }
            }
            Button("关闭") {
                store.dismissPairing()
                dismiss()
            }
            .accessibilityIdentifier("remote.pairing-close")
        }
        .padding(24)
        .accessibilityIdentifier("remote.pairing")
    }
}

// MARK: - DeviceListView（设备表＋撤销二段确认）

/// 已配对设备列表；撤销需二段确认（与桌面语义一致：撤销后 listener 自动收窄）。
public struct DeviceListView: View {
    @ObservedObject var store: RemoteStore
    @State private var pendingRevoke: RemoteClient.DeviceView?

    public init(store: RemoteStore) {
        self.store = store
    }

    public var body: some View {
        Section("已配对设备") {
            if store.devices.isEmpty {
                Text(store.readState == .loaded ? "暂无设备" : (store.readState.notice ?? "设备列表尚未核实"))
                    .font(.caption).foregroundStyle(AeroricColor.textSecondary)
            }
            ForEach(store.devices, id: \.id) { device in
                HStack {
                    VStack(alignment: .leading) {
                        Text(device.name).accessibilityIdentifier("remote.device.\(device.id)")
                        Text(Self.timestamp(device.lastSeenAt))
                            .font(.caption)
                            .foregroundStyle(AeroricColor.textSecondary)
                    }
                    Spacer()
                    if device.online {
                        Text("在线").font(.caption).foregroundStyle(AeroricColor.success)
                    }
                    Button("撤销") { pendingRevoke = device }
                        .disabled(!store.canControl)
                        .accessibilityIdentifier("remote.revoke.\(device.id)")
                }
            }
            .confirmationDialog(
                "撤销设备「\(pendingRevoke?.name ?? "")」？",
                isPresented: Binding(
                    get: { pendingRevoke != nil },
                    set: { if !$0 { pendingRevoke = nil } }
                ),
                titleVisibility: .visible
            ) {
                Button("撤销", role: .destructive) {
                    if let device = pendingRevoke {
                        Task { await store.revoke(id: device.id) }
                    }
                    pendingRevoke = nil
                }
            }
        }
    }

    static func timestamp(_ ms: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}

// MARK: - RemoteAuditView（审计只读）

/// 审计只读视图：原始行（时间/事件/payload），倒序展示最新在前。
public struct RemoteAuditView: View {
    let lines: [String]

    public init(lines: [String]) {
        self.lines = lines
    }

    public var body: some View {
        List {
            ForEach(Array(lines.enumerated().reversed()), id: \.offset) { _, line in
                Text(line)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
            }
        }
        .accessibilityIdentifier("remote.audit")
    }
}
