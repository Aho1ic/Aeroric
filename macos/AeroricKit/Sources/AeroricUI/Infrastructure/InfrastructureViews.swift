import SwiftUI

/// P7-03 Action 2：Infrastructure 五视图。
public struct DockerPanel: View {
    let store: InfrastructureStore
    public var onAction: ((_ containerID: String, _ action: DockerPanelAction) -> Void)?

    public init(store: InfrastructureStore, onAction: ((_ containerID: String, _ action: DockerPanelAction) -> Void)? = nil) {
        self.store = store
        self.onAction = onAction
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let reason = store.daemonUnavailableReason {
                // 无 Docker daemon 时显示不可用，不弹 sudo。
                VStack(spacing: 8) {
                    Image(systemName: "shippingbox.slash")
                        .font(.system(size: 28))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Text(reason)
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textSecondary)
                        .accessibilityIdentifier("infrastructure.daemon-unavailable")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                HSplitView {
                    ImageList(store: store)
                        .frame(minWidth: 200, maxWidth: 300)
                    ContainerDetail(store: store, onAction: onAction)
                        .frame(minWidth: 300, maxWidth: .infinity)
                }
            }
        }
        .accessibilityIdentifier("infrastructure.docker")
    }
}

/// Docker 动作（只读/破坏显式区分——Remove 是破坏操作需确认）。
public enum DockerPanelAction: String, CaseIterable {
    case start, stop, restart, remove

    var isDestructive: Bool {
        self == .remove
    }
}

public struct ImageList: View {
    let store: InfrastructureStore

    public init(store: InfrastructureStore) {
        self.store = store
    }

    public var body: some View {
        List(store.images) { image in
            VStack(alignment: .leading, spacing: 2) {
                Text("\(image.repository):\(image.tag)")
                    .font(.caption.monospaced())
                Text("\(image.size) · \(image.id)")
                    .font(.caption2)
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .accessibilityIdentifier("infrastructure.image.\(image.id)")
        }
        .accessibilityIdentifier("infrastructure.images")
        // 空镜像列表此前是一块纯黑矩形——与"渲染失败"无法区分(与 PortInspector
        // 同一类缺陷;daemon 不可用是另一条分支,已在 DockerPanel 处理)。
        .overlay {
            if store.images.isEmpty {
                PaneEmptyState(
                    systemImage: "shippingbox",
                    title: "还没有镜像",
                    hint: "本机没有可用的 Docker 镜像"
                )
                .accessibilityIdentifier("infrastructure.images-empty")
            }
        }
    }
}

public struct ContainerDetail: View {
    let store: InfrastructureStore
    public var onAction: ((_ containerID: String, _ action: DockerPanelAction) -> Void)?
    @State private var confirmRemove = false

    public init(store: InfrastructureStore, onAction: ((_ containerID: String, _ action: DockerPanelAction) -> Void)? = nil) {
        self.store = store
        self.onAction = onAction
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let container = store.selectedContainer {
                Text(container.names)
                    .font(.headline)
                Text("\(container.image) · \(container.state) · \(container.status)")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textSecondary)
                if !container.ports.isEmpty {
                    Text("端口：\(container.ports)")
                        .font(.caption)
                        .accessibilityIdentifier("infrastructure.container.ports")
                }
                // 只读/破坏显式区分：Remove 二段确认。
                HStack(spacing: 6) {
                    ForEach(DockerPanelAction.allCases, id: \.self) { action in
                        if action.isDestructive {
                            if confirmRemove {
                                Button("确认移除", role: .destructive) {
                                    onAction?(container.id, action)
                                    confirmRemove = false
                                }
                                .accessibilityIdentifier("infrastructure.container.confirm-remove")
                                Button("取消") { confirmRemove = false }
                            } else {
                                Button(action.rawValue.capitalized) { confirmRemove = true }
                                    .accessibilityIdentifier("infrastructure.container.\(action.rawValue)")
                            }
                        } else {
                            Button(action.rawValue.capitalized) {
                                onAction?(container.id, action)
                            }
                            .accessibilityIdentifier("infrastructure.container.\(action.rawValue)")
                        }
                    }
                }
            } else {
                Text("选择一个容器查看详情")
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(12)
        .accessibilityIdentifier("infrastructure.container-detail")
    }
}

/// PortInspector（port owner/stop 显式区分——stop 需 PID 身份确认）。
public struct PortInspector: View {
    let store: InfrastructureStore
    public var onOwner: ((_ port: UInt16) -> Void)?
    public var onStop: ((_ port: UInt16, _ pid: UInt32) -> Void)?
    @State private var confirmingStop: UInt16?

    public init(
        store: InfrastructureStore,
        onOwner: ((_ port: UInt16) -> Void)? = nil,
        onStop: ((_ port: UInt16, _ pid: UInt32) -> Void)? = nil
    ) {
        self.store = store
        self.onOwner = onOwner
        self.onStop = onStop
    }

    public var body: some View {
        List(store.ports) { port in
            HStack(spacing: 6) {
                Text("\(port.port)")
                    .font(.caption.monospaced())
                    .frame(width: 48)
                Text(port.processName)
                    .font(.caption)
                Spacer()
                Text("PID \(port.pid)")
                    .font(.caption2)
                    .foregroundStyle(AeroricColor.textTertiary)
                Button("Owner") { onOwner?(port.port) }
                    .font(.caption2)
                    .accessibilityIdentifier("infrastructure.port.owner.\(port.port)")
                // stop 显式区分（二段确认；PID 身份由宿主校验避免重用误杀）。
                if confirmingStop == port.port {
                    Button("确认停止", role: .destructive) {
                        onStop?(port.port, port.pid)
                        confirmingStop = nil
                    }
                    .accessibilityIdentifier("infrastructure.port.confirm-stop.\(port.port)")
                } else {
                    Button("Stop") { confirmingStop = port.port }
                        .font(.caption2)
                        .accessibilityIdentifier("infrastructure.port.stop.\(port.port)")
                }
            }
            .accessibilityIdentifier("infrastructure.port.\(port.port)")
        }
        .accessibilityIdentifier("infrastructure.ports")
        .overlay {
            if store.ports.isEmpty {
                PaneEmptyState(
                    systemImage: "network.slash",
                    title: "没有监听中的端口",
                    hint: "本机当前没有端口被占用"
                )
                .accessibilityIdentifier("infrastructure.ports-empty")
            }
        }
    }
}

/// CondaEnvironmentPicker（选择传递经校验的环境路径；不能污染全局 shell rc）。
public struct CondaEnvironmentPicker: View {
    let store: InfrastructureStore
    public var onPick: ((_ path: String) -> Void)?

    public init(store: InfrastructureStore, onPick: ((_ path: String) -> Void)? = nil) {
        self.store = store
        self.onPick = onPick
    }

    public var body: some View {
        Picker("Conda 环境", selection: Binding(
            get: { store.selectedCondaEnvPath ?? "" },
            set: { onPick?($0) }
        )) {
            if store.condaEnvironments.isEmpty {
                Text("未发现 Conda 环境").tag("")
            }
            ForEach(store.condaEnvironments) { env in
                Text(env.name).tag(env.path)
            }
        }
        .accessibilityIdentifier("infrastructure.conda-picker")
    }
}
