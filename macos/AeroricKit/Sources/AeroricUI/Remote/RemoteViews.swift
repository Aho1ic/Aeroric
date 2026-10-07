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
    /// 审计行（`<ts>\t<event>\t<payload>` 的原始文本；只读展示）。
    @Published public private(set) var auditLines: [String]

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
        self.status = status
        self.devices = devices
        self.auditLines = auditLines
        self.auditReader = { [] }
    }

    /// 生产构造：审计行从磁盘读（调用方给读取闭包，避免 Kit 依赖路径解析）。
    public init(client: RemoteClient?, auditReader: @escaping () -> [String]) {
        self.client = client
        self.status = .offline()
        self.devices = []
        self.auditLines = auditReader()
        self.auditReader = auditReader
    }

    /// 刷新状态＋设备表。
    public func refresh() async {
        guard let client = clientForWrite() else { return }
        do {
            status = try await client.status()
            devices = try await client.listDevices()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
        auditLines = auditReader()
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

    public init(store: RemoteStore) {
        self.store = store
    }

    public var body: some View {
        Form {
            Section("服务") {
                Toggle("启用远程访问", isOn: Binding(
                    get: { store.status.enabled && store.status.running },
                    set: { on in
                        Task {
                            if on { await store.start(port: nil) } else { await store.stop() }
                        }
                    }
                ))
                .accessibilityIdentifier("remote.enable")
                HStack {
                    Text(store.status.running ? "运行中" : "已停止")
                        .font(.caption)
                        .foregroundStyle(store.status.running ? Color.dsSuccess : Color.dsTextSecondary)
                    Text(":\(store.status.port)")
                        .font(.caption.monospaced())
                    if store.status.networkExposed {
                        Text("已暴露到网络")
                            .font(.caption)
                            .foregroundStyle(Color.dsWarning)
                    }
                }
                .accessibilityIdentifier("remote.status-line")
                Button("生成配对二维码") { showPairing = true }
                    .disabled(!store.status.running)
                    .accessibilityIdentifier("remote.pair-button")
            }

            Section("Relay / 公网端点") {
                TextField("Relay URL（wss://，留空不用 relay）", text: $relayURL)
                    .accessibilityIdentifier("remote.relay-url")
                SecureField("Relay 共享口令", text: $relayToken)
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
                .accessibilityIdentifier("remote.config-save")
            }

            DeviceListView(store: store)

            if let error = store.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
                    .accessibilityIdentifier("remote.error")
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.remote")
        .task { await store.refresh() }
        .sheet(isPresented: $showPairing) {
            PairingView(store: store)
        }
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
                    .foregroundStyle(Color.dsTextSecondary)
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
                    Text(error).font(.caption).foregroundStyle(Color.dsDanger)
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
                Text("暂无设备").font(.caption).foregroundStyle(Color.dsTextSecondary)
            }
            ForEach(store.devices, id: \.id) { device in
                HStack {
                    VStack(alignment: .leading) {
                        Text(device.name).accessibilityIdentifier("remote.device.\(device.id)")
                        Text(Self.timestamp(device.lastSeenAt))
                            .font(.caption)
                            .foregroundStyle(Color.dsTextSecondary)
                    }
                    Spacer()
                    if device.online {
                        Text("在线").font(.caption).foregroundStyle(Color.dsSuccess)
                    }
                    Button("撤销") { pendingRevoke = device }
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
