import SwiftUI

/// P8-02 Action 2：provider/proxy/local router 设置五视图。凭据导出默认脱敏；
/// router listener 沿用安全默认（回环地址＋非回环要求 32 字符 token——core
/// `RouterRuntimeConfig::validate` 同语义，Swift 不复制一套校验）；OAuth 回调
/// 状态经 core `CredentialOptions` 门控（secret 强制服务拒内置凭据）。

/// 代理设置（proxy：shared business；URL/no_proxy/认证经调用方提交 core）。
public struct ProxySettingsView: View {
    public var initialURL: String
    public var initialNoProxy: String
    public var onProxySave: ((_ url: String, _ noProxy: String) -> Void)?
    @State public var url: String
    @State private var noProxy: String
    @State private var saveError: String?

    public init(
        initialURL: String = "",
        initialNoProxy: String = "127.0.0.1,localhost,::1",
        onProxySave: ((_ url: String, _ noProxy: String) -> Void)? = nil
    ) {
        self.initialURL = initialURL
        self.initialNoProxy = initialNoProxy
        self._url = State(initialValue: initialURL)
        self._noProxy = State(initialValue: initialNoProxy)
        self.onProxySave = onProxySave
    }

    public var body: some View {
        Form {
            TextField("代理地址（留空禁用）", text: $url)
                .accessibilityIdentifier("proxy.url")
            TextField("代理排除", text: $noProxy)
                .accessibilityIdentifier("proxy.no-proxy")
            Button("保存") {
                // URL 校验经 core（RouterOutboundProxy::new 同语义）；这里只拦
                // 空值与明显非法 scheme，格式错误由 core 在提交时拒绝并回显。
                let trimmed = url.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty, !trimmed.hasPrefix("http://"),
                   !trimmed.hasPrefix("https://"), !trimmed.hasPrefix("socks5://"),
                   !trimmed.hasPrefix("socks5h://") {
                    saveError = "代理必须使用 HTTP(S)、SOCKS5 或 SOCKS5H"
                    return
                }
                saveError = nil
                onProxySave?(trimmed, noProxy)
            }
            .disabled(url.isEmpty && noProxy.isEmpty)
            .accessibilityIdentifier("proxy.save")
            if let saveError {
                Text(saveError)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
                    .accessibilityIdentifier("proxy.error")
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.proxy")
    }
}

/// Provider（模型供应商）设置：provider catalog 与已安装 Agent 支持列表区分；
/// base URL/凭据经调用方提交 core，Swift 不复制校验。
public struct ProviderSettingsView: View {
    /// provider 目录条目（catalog 静态面：名称/默认 base URL/是否 OAuth）。
    public var providers: [ProviderCatalogEntry]
    public var onBaseURLChange: ((_ providerID: String, _ baseURL: String) -> Void)?
    @State private var draftURLs: [String: String] = [:]
    @State private var saveError: String?

    public init(
        providers: [ProviderCatalogEntry] = ProviderCatalogEntry.builtIn,
        onBaseURLChange: ((_ providerID: String, _ baseURL: String) -> Void)? = nil
    ) {
        self.providers = providers
        self.onBaseURLChange = onBaseURLChange
    }

    public var body: some View {
        List {
            ForEach(providers) { provider in
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(provider.name)
                            .accessibilityIdentifier("provider.name.\(provider.id)")
                        Spacer()
                        if provider.isOAuth {
                            Text("OAuth")
                                .font(.caption)
                                .foregroundStyle(Color.dsTextMuted)
                        }
                    }
                    TextField(
                        provider.defaultBaseURL,
                        text: Binding(
                            get: { draftURLs[provider.id] ?? "" },
                            set: { draftURLs[provider.id] = $0 }
                        )
                    )
                    .accessibilityIdentifier("provider.url.\(provider.id)")
                    Button("应用") {
                        let trimmed = (draftURLs[provider.id] ?? "")
                            .trimmingCharacters(in: .whitespacesAndNewlines)
                        // 校验经 core（normalize_base_url 同语义）；这里只拦空白提交。
                        guard !trimmed.isEmpty else {
                            saveError = "base URL 不能为空"
                            return
                        }
                        saveError = nil
                        onBaseURLChange?(provider.id, trimmed)
                    }
                    .accessibilityIdentifier("provider.apply.\(provider.id)")
                }
                .padding(.vertical, 2)
                .accessibilityIdentifier("settings.provider.\(provider.id)")
            }
            if let saveError {
                Text(saveError)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
                    .accessibilityIdentifier("provider.error")
            }
        }
        .accessibilityIdentifier("settings.providers")
    }
}

/// provider 目录条目（catalog 静态面——与已安装 Agent 支持列表区分：catalog 描述
/// 供应商可配置项，支持列表描述本机检测到的 Agent 渠道）。
public struct ProviderCatalogEntry: Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let defaultBaseURL: String
    public let isOAuth: Bool

    public init(id: String, name: String, defaultBaseURL: String, isOAuth: Bool = false) {
        self.id = id
        self.name = name
        self.defaultBaseURL = defaultBaseURL
        self.isOAuth = isOAuth
    }

    /// 内置目录（默认 base URL 与 core local_router/local_router 常量同源）。
    public static let builtIn: [ProviderCatalogEntry] = [
        ProviderCatalogEntry(id: "claude", name: "Claude Code", defaultBaseURL: "https://api.anthropic.com"),
        ProviderCatalogEntry(id: "codex", name: "Codex", defaultBaseURL: "https://api.openai.com/v1"),
        ProviderCatalogEntry(id: "codex-chatgpt", name: "Codex (ChatGPT)", defaultBaseURL: "https://chatgpt.com/backend-api/codex", isOAuth: true),
    ]
}

/// 本地路由设置（local_router：shared business；listener 安全默认——回环地址
/// 不要求 token，非回环要求 32 字符，校验经 core `RouterRuntimeConfig::validate`）。
public struct LocalRouterSettingsView: View {
    public var initialEnabled: Bool
    public var initialListenHost: String
    public var initialListenPort: Int
    public var onRouterSave: ((_ enabled: Bool, _ host: String, _ port: Int) -> Void)?
    @State public var enabled: Bool
    @State public var listenHost: String
    @State public var listenPort: Int
    @State private var saveError: String?

    public init(
        initialEnabled: Bool = false,
        initialListenHost: String = "127.0.0.1",
        initialListenPort: Int = 15721,
        onRouterSave: ((_ enabled: Bool, _ host: String, _ port: Int) -> Void)? = nil
    ) {
        self.initialEnabled = initialEnabled
        self.initialListenHost = initialListenHost
        self.initialListenPort = initialListenPort
        self._enabled = State(initialValue: initialEnabled)
        self._listenHost = State(initialValue: initialListenHost)
        self._listenPort = State(initialValue: initialListenPort)
        self.onRouterSave = onRouterSave
    }

    public var body: some View {
        Form {
            Toggle("启用本地路由", isOn: $enabled)
                .accessibilityIdentifier("local-router.enabled")
            TextField("监听地址（安全默认 127.0.0.1；非回环需 ≥32 字符 token）", text: $listenHost)
                .accessibilityIdentifier("local-router.host")
            Stepper("监听端口 \(listenPort)", value: $listenPort, in: 1...65535)
                .accessibilityIdentifier("local-router.port")
            Button("保存") {
                let host = listenHost.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !host.isEmpty else {
                    saveError = "监听地址不能为空"
                    return
                }
                // 非回环 listener 的 token 要求经 core 校验；这里只拦空白提交。
                let loopback = host == "127.0.0.1" || host == "localhost" || host == "::1"
                if !loopback {
                    saveError = "非回环监听需要访问令牌（core 校验拒绝空 token 配置）"
                    return
                }
                saveError = nil
                onRouterSave?(enabled, host, listenPort)
            }
            .accessibilityIdentifier("local-router.save")
            if let saveError {
                Text(saveError)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
                    .accessibilityIdentifier("local-router.error")
            }
        }
        .padding(16)
        .accessibilityIdentifier("settings.local-router")
    }
}

/// Agent 操作面板（operations：后端持有状态——正在跑的操作、上次结果、取消）。
public struct AgentOperationPanel: View {
    public var operations: [AgentOperationSnapshotRow]
    public var onCancel: ((_ agent: String) -> Void)?

    public init(
        operations: [AgentOperationSnapshotRow] = [],
        onCancel: ((_ agent: String) -> Void)? = nil
    ) {
        self.operations = operations
        self.onCancel = onCancel
    }

    public var body: some View {
        List {
            if operations.isEmpty {
                Text("没有进行中或最近的操作")
                    .foregroundStyle(Color.dsTextMuted)
                    .accessibilityIdentifier("agent-operations.empty")
            }
            ForEach(operations) { operation in
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(operation.agent)
                            .accessibilityIdentifier("agent-operation.\(operation.agent)")
                        Spacer()
                        Text(operation.stateLabel)
                            .font(.caption)
                            .foregroundStyle(operation.stateColor)
                    }
                    Text(operation.message)
                        .font(.caption)
                        .foregroundStyle(Color.dsTextMuted)
                    if operation.isRunning {
                        ProgressView(value: Double(operation.progress), total: 100)
                            .accessibilityIdentifier("agent-operation.progress.\(operation.agent)")
                        Button("取消") { onCancel?(operation.agent) }
                            .accessibilityIdentifier("agent-operation.cancel.\(operation.agent)")
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .accessibilityIdentifier("settings.agent-operations")
    }
}

/// Agent 操作快照行（operations wire 镜像：agent/state/progress/message）。
public struct AgentOperationSnapshotRow: Identifiable, Equatable, Sendable {
    public let id: String
    public let agent: String
    public let stateLabel: String
    public let isRunning: Bool
    public let progress: Int
    public let message: String

    public init(
        id: String,
        agent: String,
        stateLabel: String,
        isRunning: Bool,
        progress: Int,
        message: String
    ) {
        self.id = id
        self.agent = agent
        self.stateLabel = stateLabel
        self.isRunning = isRunning
        self.progress = progress
        self.message = message
    }

    public var stateColor: Color {
        switch stateLabel {
        case "running": .dsAccent
        case "succeeded": .dsSuccess
        case "cancelled": .dsWarning
        default: .dsDanger
        }
    }
}

/// 配置导入导出 sheet（凭据导出默认脱敏；「包含 secret」需二次确认）。
public struct ConfigImportExportSheet: View {
    public var onExport: ((_ includeSecrets: Bool, _ outputPath: String) -> Void)?
    public var onImport: ((_ sourcePath: String) -> Void)?
    @State public var includeSecrets = false
    @State public var confirmedSecrets = false
    @State private var outputPath = ""
    @State private var importPath = ""

    public init(
        onExport: ((_ includeSecrets: Bool, _ outputPath: String) -> Void)? = nil,
        onImport: ((_ sourcePath: String) -> Void)? = nil
    ) {
        self.onExport = onExport
        self.onImport = onImport
    }

    public var body: some View {
        Form {
            TextField("导出路径", text: $outputPath)
                .accessibilityIdentifier("config-export.path")
            // 凭据导出默认脱敏：includeSecrets 默认 false，勾选后还要二次确认。
            Toggle("包含凭据（默认脱敏）", isOn: $includeSecrets)
                .accessibilityIdentifier("config-export.secrets")
            if includeSecrets {
                Toggle("我已确认导出含明文凭据", isOn: $confirmedSecrets)
                    .accessibilityIdentifier("config-export.confirm")
                Text("导出文件含明文凭据，必须放入 0600 私有目录。")
                    .font(.caption)
                    .foregroundStyle(Color.dsWarning)
            }
            Button("导出") {
                onExport?(includeSecrets && confirmedSecrets, outputPath)
            }
            .disabled(outputPath.isEmpty || (includeSecrets && !confirmedSecrets))
            .accessibilityIdentifier("config-export.start")

            Divider()
            TextField("导入路径", text: $importPath)
                .accessibilityIdentifier("config-import.path")
            Button("导入") { onImport?(importPath) }
                .disabled(importPath.isEmpty)
                .accessibilityIdentifier("config-import.start")
        }
        .padding(16)
        .accessibilityIdentifier("settings.config-import-export")
    }
}
