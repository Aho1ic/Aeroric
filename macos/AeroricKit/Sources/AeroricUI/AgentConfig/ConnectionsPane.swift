import AeroricClient
import SwiftUI

/// 连接: the two official-login cards (G1), then the user's API connections.
struct ConnectionsPane: View {
    @Bindable var store: AgentConfigStore
    @State private var testModel = ""

    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            VStack(spacing: 0) {
                List(selection: Binding(get: { store.selectedConnection }, set: { store.selectConnection($0) })) {
                    ForEach(store.document.connections) { connection in
                        HStack(spacing: AeroricSpace.s) {
                            Circle()
                                .fill(dotColor(connection))
                                .frame(width: 7, height: 7)
                            Text(connection.label).lineLimit(1)
                            Spacer()
                            Text("\(connection.models.count)")
                                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                        }
                        .tag(Optional(connection.id))
                        .help(connection.baseUrl)
                    }
                }
                .listStyle(.sidebar)
                .scrollContentBackground(.hidden)
                .accessibilityIdentifier("agent-config.connections")
                HairlineDivider()
                Button { store.newConnection() } label: { Label("新建连接", systemImage: "plus") }
                    .buttonStyle(.borderless)
                    .padding(AeroricSpace.m)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("agent-config.new-connection")
            }
        } detail: {
            VStack(alignment: .leading, spacing: AeroricSpace.xl) {
                officialCards
                if store.connectionDraft != nil {
                    editor
                } else {
                    Text("选择左侧的连接，或新建一个第三方 API 连接。")
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
        }
    }

    private func dotColor(_ connection: ACC.Connection) -> Color {
        if !connection.enabled { return AeroricColor.textTertiary }
        if !store.cooldowns(forConnection: connection.id).isEmpty { return AeroricColor.warning }
        return store.snapshot?.credentials[connection.id] == "stored" ? AeroricColor.success : AeroricColor.danger
    }

    // MARK: Official logins (G1)

    private var officialCards: some View {
        SettingsCard("本机官方配置") {
            if let official = store.official {
                officialRow("Codex", official.codex)
                officialRow("Claude Code", official.claude)
                Text("不选 Agent 配置的会话，就用这里的本机登录。Aeroric 只读取登录方式和账号，不复制 token。")
                    .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            } else {
                SettingsRow("检测本机的 Codex 与 Claude Code 登录",
                            detail: "会读取 ~/.codex/auth.json（只取登录方式和邮箱）、~/.claude.json 的账号邮箱，并查看钥匙串里是否有“Claude Code-credentials”条目（不读取内容）。") {
                    Button("检测") { Task { await store.detectOfficial() } }
                        .accessibilityIdentifier("agent-config.detect-official")
                }
            }
        }
    }

    private func officialRow(_ title: String, _ login: ACC.OfficialLogin) -> some View {
        SettingsRow(title, detail: login.sources.joined(separator: "、")) {
            HStack(spacing: AeroricSpace.s) {
                if !login.installed {
                    Text("未安装").foregroundStyle(AeroricColor.textTertiary)
                } else if login.signedIn {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(AeroricColor.success)
                    Text(modeTitle(login.mode) + (login.account.map { " · \($0)" } ?? ""))
                } else {
                    Text("已安装，未登录").foregroundStyle(AeroricColor.warning)
                }
            }
            .font(.system(size: AeroricType.secondary))
        }
    }

    private func modeTitle(_ mode: String) -> String {
        switch mode {
        case "chatgpt": "ChatGPT 登录"
        case "apikey": "API Key"
        case "subscription": "订阅登录"
        default: "未登录"
        }
    }

    // MARK: Editor

    private var draft: Binding<ACC.Connection> {
        Binding(get: { store.connectionDraft ?? ACC.Connection(id: "", label: "") },
                set: { store.connectionDraft = $0 })
    }

    @ViewBuilder private var editor: some View {
        let credential = store.selectedConnection.flatMap { store.snapshot?.credentials[$0] } ?? "missing"
        SettingsCard("连接") {
            SettingsRow("名称") {
                TextField("名称", text: draft.label).frame(width: 280)
            }
            SettingsRow("协议", detail: "这个端点接受的协议；需要时网关会在三者之间转换。") {
                ProtocolPicker(selection: draft.protocols)
            }
            SettingsRow("Base URL", detail: "例如 https://api.deepseek.com/v1") {
                TextField("https://…", text: draft.baseUrl)
                    .autocorrectionDisabled()
                    .frame(width: 320)
                    .accessibilityIdentifier("agent-config.base-url")
            }
            SettingsRow("API Key", detail: credential == "stored" ? "已存在钥匙串里；留空表示不修改。" : "尚未保存 key。") {
                SecureField(credential == "stored" ? "已存（留空不改）" : "粘贴 API Key", text: $store.credentialDraft)
                    .autocorrectionDisabled()
                    .frame(width: 320)
                    .accessibilityIdentifier("agent-config.api-key")
            }
            SettingsRow("使用全局代理") { Toggle("", isOn: draft.useGlobalProxy).labelsHidden().toggleStyle(.switch) }
            SettingsRow("限流就暂停", detail: "429 时让这个连接休息一段时间。中转站、聚合站可以关掉。") {
                Toggle("", isOn: draft.restOnLimit).labelsHidden().toggleStyle(.switch)
            }
            SettingsRow("启用") { Toggle("", isOn: draft.enabled).labelsHidden().toggleStyle(.switch) }
        }
        modelsCard
        if store.selectedConnection != nil { testCard }
        let cooling = store.selectedConnection.map { store.cooldowns(forConnection: $0) } ?? []
        if !cooling.isEmpty {
            SettingsCard("冷却中") {
                ForEach(cooling, id: \.self) { entry in
                    Text("\(entry.model.map { "\($0) · " } ?? "整条连接 · ")\(AgentConfigText.reasonTitle(entry.reason)) · 到 \(clock(entry.untilEpochS))")
                        .font(.system(size: AeroricType.secondary))
                }
            }
        }
        EditorActions(
            dirty: store.connectionDirty,
            canDelete: store.selectedConnection != nil,
            onSave: { Task { await store.saveConnection() } },
            onRevert: { store.selectConnection(store.selectedConnection) },
            onDelete: { Task { await store.deleteConnection() } }
        )
    }

    private var modelsCard: some View {
        SettingsCard("模型") {
            HStack(spacing: AeroricSpace.m) {
                Button("拉取模型列表") { Task { await store.fetchModels() } }
                    .disabled(store.selectedConnection == nil || store.connectionDirty)
                    .help(store.connectionDirty ? "先保存连接" : "从上游的 /models 拉取，合并进下表；已有的条目和你改过的字段保留。")
                    .accessibilityIdentifier("agent-config.fetch-models")
                Button("手动添加") {
                    store.connectionDraft?.models.append(ACC.ModelEntry(upstreamId: "model-\(draft.wrappedValue.models.count + 1)"))
                }
                Spacer()
                Text("映射名相同的模型会组成一条路由，故障时自动切换。")
                    .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            }
            if draft.wrappedValue.models.isEmpty {
                Text("还没有模型。").foregroundStyle(AeroricColor.textTertiary)
            } else {
                Grid(alignment: .leading, horizontalSpacing: AeroricSpace.m, verticalSpacing: AeroricSpace.s) {
                    GridRow {
                        Text("上游 ID"); Text("映射名"); Text("显示名"); Text("上下文"); Text("协议"); Text("启用"); Text("")
                    }
                    .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    ForEach(draft.models.indices, id: \.self) { index in
                        modelRow(index)
                    }
                }
            }
        }
    }

    private func modelRow(_ index: Int) -> some View {
        let model = draft.models[index]
        return GridRow {
            TextField("上游 ID", text: model.upstreamId).frame(minWidth: 140)
            TextField(model.wrappedValue.upstreamId, text: model.mappedName).frame(minWidth: 120)
            TextField("—", text: model.displayName).frame(minWidth: 80)
            TextField("—", value: model.contextWindow, format: .number).frame(width: 80)
            Menu(model.wrappedValue.protocols.isEmpty ? "同连接"
                 : model.wrappedValue.protocols.map(AgentConnectionsClient.protocolTitle).joined(separator: "/")) {
                Button("同连接") { model.wrappedValue.protocols = [] }
                ForEach(AgentConfigText.protocols, id: \.self) { raw in
                    Button(AgentConnectionsClient.protocolTitle(raw)) { model.wrappedValue.protocols = [raw] }
                }
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            Toggle("", isOn: model.enabled).labelsHidden()
            Button { store.connectionDraft?.models.remove(at: index) } label: { Image(systemName: "minus.circle") }
                .buttonStyle(.borderless)
                .help("删除这个模型")
        }
    }

    private var testCard: some View {
        SettingsCard("测试") {
            HStack(spacing: AeroricSpace.m) {
                Picker("模型", selection: $testModel) {
                    ForEach(draft.wrappedValue.models.map(\.upstreamId), id: \.self) { Text($0).tag($0) }
                }
                .frame(width: 260)
                Button("发一条测试消息") { Task { await store.testConnection(model: testModel) } }
                    .disabled(testModel.isEmpty || store.connectionDirty)
                    .accessibilityIdentifier("agent-config.test")
            }
            Text("会向上游发一条“ping”，只让模型回 1 个 token，消耗极少额度。只测连通性的话，用“拉取模型列表”即可，不花额度。")
                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            if let probe = store.probe {
                Text(probe).font(.system(size: AeroricType.secondary)).textSelection(.enabled)
                    .accessibilityIdentifier("agent-config.test-result")
            }
        }
        .onAppear { if testModel.isEmpty { testModel = draft.wrappedValue.models.first?.upstreamId ?? "" } }
    }
}

func clock(_ epoch: UInt64) -> String {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm"
    return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(epoch)))
}
