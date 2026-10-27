import AeroricClient
import SwiftUI

/// 连接: the two official-login cards (G1), then the user's API connections.
/// 2026-10-26 polish: card list, an overview card, rounded sections,
/// expandable model rows and a floating save bar.
struct ConnectionsPane: View {
    @Bindable var store: AgentConfigStore
    @State private var expandedModel: String?
    @State private var testedModel: String?
    @State private var confirmDelete = false
    /// `codex` / `claude` while one of the built-in local logins is shown.
    @State private var builtin: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            ConfigListColumn("连接", newTitle: "新建", newIdentifier: "agent-config.new-connection",
                             onNew: { newConnection() }) {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
                        listSection("本机登录")
                        builtinRow("codex", title: "Codex", login: store.official?.codex)
                        builtinRow("claude", title: "Claude Code", login: store.official?.claude)
                        listSection("我的连接").padding(.top, AeroricSpace.m)
                        if store.document.connections.isEmpty {
                            Text("还没有连接，点右上角“新建”。")
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .padding(.horizontal, AeroricSpace.l)
                        }
                        ForEach(store.document.connections) { connection in
                            HoverCapsuleRow(selected: builtin == nil && store.selectedConnection == connection.id,
                                            height: 50, horizontalPadding: AeroricSpace.s) {
                                builtin = nil
                                store.selectConnection(connection.id)
                            } label: {
                                ConfigListItem(title: connection.label,
                                               subtitle: AgentConfigText.connectionSubtitle(connection),
                                               status: status(connection).tint)
                            }
                            .help(connection.baseUrl)
                            .accessibilityIdentifier("agent-config.connection.\(connection.id)")
                        }
                    }
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.bottom, AeroricSpace.l)
                }
                .scrollIndicators(.hidden)
                .accessibilityIdentifier("agent-config.connections")
            }
        } detail: {
            VStack(alignment: .leading, spacing: AeroricSpace.xxxl) {
                if let builtin {
                    builtinDetail(builtin)
                } else if store.connectionDraft != nil {
                    overview
                    endpointCard
                    networkCard
                    modelsCard
                    coolingCard
                } else {
                    emptyDetail
                }
            }
            .padding(.bottom, 72)
        } overlay: {
            FloatingSaveBar(visible: store.connectionDirty,
                            onRevert: { store.selectConnection(store.selectedConnection) },
                            onSave: { Task { await store.saveConnection() } })
        }
        .onChange(of: store.selectedConnection) { _, _ in
            expandedModel = nil
            testedModel = nil
        }
        .sheet(isPresented: Binding(get: { store.modelPick != nil }, set: { if !$0 { store.modelPick = nil } })) {
            ModelPickSheet(store: store)
        }
        .confirmationDialog("删除这个连接？", isPresented: $confirmDelete) {
            Button("删除", role: .destructive) { Task { await store.deleteConnection() } }
                .accessibilityIdentifier("agent-config.delete")
            Button("取消", role: .cancel) {}
        } message: {
            Text("钥匙串里的 key 会一起删掉。用到它的号池会自动去掉这个成员。")
        }
    }

    private func status(_ connection: ACC.Connection) -> AgentConfigText.ConnectionStatus {
        AgentConfigText.connectionStatus(
            enabled: connection.enabled,
            credential: store.snapshot?.credentials[connection.id],
            cooling: !store.cooldowns(forConnection: connection.id).isEmpty
        )
    }

    private var draft: Binding<ACC.Connection> {
        Binding(get: { store.connectionDraft ?? ACC.Connection(id: "", label: "") },
                set: { store.connectionDraft = $0 })
    }

    private var credential: String {
        store.selectedConnection.flatMap { store.snapshot?.credentials[$0] } ?? "missing"
    }

    // MARK: Overview

    private var overview: some View {
        let connection = draft.wrappedValue
        let cooling = store.selectedConnection.map { store.cooldowns(forConnection: $0) } ?? []
        return HStack(alignment: .top, spacing: AeroricSpace.l) {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                TextField("连接名称", text: draft.label)
                    .textFieldStyle(.plain)
                    .font(.system(size: AeroricType.largeTitle, weight: .semibold))
                    .accessibilityIdentifier("agent-config.connection-name")
                Text(URL(string: connection.baseUrl)?.host ?? (connection.baseUrl.isEmpty ? "还没填 Base URL" : connection.baseUrl))
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                HStack(spacing: AeroricSpace.s) {
                    if !connection.enabled {
                        StatusChip(text: "已停用", systemImage: "pause.fill", tint: AeroricColor.textTertiary)
                    }
                    if credential == "stored" {
                        StatusChip(text: "已存密钥", systemImage: "key.fill", tint: AeroricColor.success)
                    } else {
                        StatusChip(text: "缺少密钥", systemImage: "key", tint: AeroricColor.danger)
                    }
                    if let soonest = cooling.map(\.untilEpochS).min() {
                        StatusChip(text: "冷却中 · 到 \(clock(soonest))", systemImage: "snowflake", tint: AeroricColor.warning)
                    }
                    if let id = store.selectedConnection, store.reasoningStripped.contains(id) {
                        StatusChip(text: "不收回放思考", systemImage: "brain", tint: AeroricColor.textSecondary)
                            .help(AgentConfigText.reasoningStrippedNote)
                            .accessibilityIdentifier("agent-config.reasoning-stripped")
                    }
                }
            }
            Spacer(minLength: 0)
            HStack(spacing: AeroricSpace.m) {
                Toggle("启用", isOn: draft.enabled)
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .help(connection.enabled ? "已启用：会被路由" : "已停用：不会被路由")
                Menu {
                    Button("删除连接…", role: .destructive) { confirmDelete = true }
                        .disabled(store.selectedConnection == nil)
                } label: {
                    Image(systemName: "ellipsis.circle").font(.system(size: 16))
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .accessibilityIdentifier("agent-config.more")
            }
        }
        .padding(AeroricSpace.xl)
        .configSurface(AeroricRadius.xl)
    }

    // MARK: Sections

    private var endpointCard: some View {
        ConfigCard("接入") {
            ConfigRow("协议", detail: "这个端点接受的协议；需要时网关会在三者之间转换。", first: true) {
                ProtocolChips(selection: draft.protocols)
            }
            ConfigRow("Base URL", detail: "例如 https://api.deepseek.com/v1") {
                ConfigTextField(placeholder: "https://…", text: draft.baseUrl, monospaced: true)
                    .accessibilityIdentifier("agent-config.base-url")
            }
            ConfigRow("API Key", detail: credential == "stored" ? "已存在钥匙串里；留空表示不修改。" : "保存后只存进钥匙串。") {
                ConfigTextField(placeholder: credential == "stored" ? "已存（留空不改）" : "粘贴 API Key",
                                text: $store.credentialDraft, secure: true)
                    .accessibilityIdentifier("agent-config.api-key")
            }
        }
    }

    private var networkCard: some View {
        ConfigCard("网络") {
            ConfigRow("使用全局代理", detail: "默认直连；勾上后走设置里的全局代理。", first: true) {
                Toggle("", isOn: draft.useGlobalProxy).labelsHidden().toggleStyle(.switch)
            }
            ConfigRow("限流就暂停", detail: "429 时让这个连接休息一段时间。中转站、聚合站可以关掉。") {
                Toggle("", isOn: draft.restOnLimit).labelsHidden().toggleStyle(.switch)
            }
        }
    }

    @ViewBuilder private var coolingCard: some View {
        let cooling = store.selectedConnection.map { store.cooldowns(forConnection: $0) } ?? []
        if !cooling.isEmpty {
            ConfigCard("冷却中") {
                ForEach(Array(cooling.enumerated()), id: \.offset) { index, entry in
                    ConfigRow(entry.model ?? "整条连接", detail: AgentConfigText.reasonTitle(entry.reason), first: index == 0) {
                        StatusChip(text: "到 \(clock(entry.untilEpochS))", systemImage: "snowflake", tint: AeroricColor.warning)
                    }
                }
            }
        }
    }

    // MARK: Models

    private var modelsCard: some View {
        ConfigCard("模型") {
            HStack(spacing: AeroricSpace.s) {
                Button { Task { await store.fetchModels() } } label: {
                    Label("拉取模型", systemImage: "arrow.down.circle")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .disabled(!store.canFetchModels)
                .help(store.canFetchModels ? "用上面填的地址和 key 拉取 /models 并合并；已有的条目和你改过的字段保留。不用先保存。"
                      : "先填 Base URL 和 API Key")
                .accessibilityIdentifier("agent-config.fetch-models")
                Button { addModel() } label: { Label("手动添加", systemImage: "plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier("agent-config.add-model")
            }
        } content: {
            if draft.wrappedValue.models.isEmpty {
                VStack(spacing: AeroricSpace.m) {
                    Image(systemName: "cube.transparent")
                        .font(.system(size: 28, weight: .light))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Text("还没有模型").font(.system(size: AeroricType.body, weight: .medium))
                    Text("填好 Base URL 和 API Key 后点“拉取模型”，或者手动添加。映射名相同的模型会组成一条路由，故障时自动切换。")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 360)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, AeroricSpace.xxxl)
            } else {
                ForEach(Array(draft.wrappedValue.models.enumerated()), id: \.offset) { index, model in
                    modelRow(index, model)
                }
            }
        }
    }

    private func newConnection() {
        builtin = nil
        expandedModel = nil
        testedModel = nil
        store.newConnection()
    }

    private func listSection(_ title: String) -> some View {
        Text(title)
            .font(AeroricType.sectionLabel())
            .foregroundStyle(AeroricColor.textTertiary)
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.xs)
    }

    private func builtinRow(_ id: String, title: String, login: ACC.OfficialLogin?) -> some View {
        HoverCapsuleRow(selected: builtin == id, height: 50, horizontalPadding: AeroricSpace.s) {
            builtin = id
            store.selectConnection(nil)
            if store.official == nil { Task { await store.detectOfficial() } }
        } label: {
            ConfigListItem(title: title, subtitle: officialSubtitle(login),
                           status: login.map { $0.installed && $0.signedIn ? AeroricColor.success : AeroricColor.textTertiary })
        }
        .accessibilityIdentifier("agent-config.builtin.\(id)")
    }

    private func officialSubtitle(_ login: ACC.OfficialLogin?) -> String {
        guard let login else { return "本机登录 · 点开检测" }
        if !login.installed { return "未安装" }
        guard login.signedIn else { return "已安装，未登录" }
        return modeTitle(login.mode) + (login.account.map { " · \($0)" } ?? "")
    }

    private func addModel() {
        store.addManualModel()
        withAnimation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion)) {
            expandedModel = store.connectionDraft?.models.last?.upstreamId
        }
    }

    private func modelRow(_ index: Int, _ model: ACC.ModelEntry) -> some View {
        let expanded = expandedModel == model.upstreamId
        let binding = modelBinding(index, fallback: model)
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .rotationEffect(.degrees(expanded ? 90 : 0))
                Text(model.upstreamId).font(AeroricType.mono(AeroricType.secondary)).lineLimit(1)
                if model.routingName != model.upstreamId {
                    Image(systemName: "arrow.right").font(.system(size: 9)).foregroundStyle(AeroricColor.textTertiary)
                    Text(model.routingName).font(.system(size: AeroricType.secondary, weight: .medium)).lineLimit(1)
                }
                ForEach(AgentConfigText.modelBadges(model), id: \.self) { StatusChip(text: $0) }
                Spacer(minLength: AeroricSpace.m)
                if testedModel == model.upstreamId, let probe = store.probe {
                    let ok = AgentConfigText.probeSucceeded(probe)
                    StatusChip(text: ok ? "测试通过" : "测试失败", systemImage: ok ? "checkmark" : "xmark",
                               tint: ok ? AeroricColor.success : AeroricColor.danger)
                        .help(probe)
                        .transition(.opacity)
                        .accessibilityIdentifier("agent-config.test-result")
                }
                Button { test(model.upstreamId) } label: { Image(systemName: "bolt.horizontal") }
                    .buttonStyle(.borderless)
                    .disabled(store.connectionDirty || store.selectedConnection == nil)
                    .help(store.connectionDirty ? "先保存连接" : "向上游发一句“回复 pong”，最多回 64 个 token，消耗极少额度")
                    .accessibilityIdentifier("agent-config.test.\(index)")
                Toggle("", isOn: binding.enabled).labelsHidden().toggleStyle(.switch).controlSize(.mini)
            }
            .padding(.horizontal, AeroricSpace.xl)
            .padding(.vertical, AeroricSpace.m + 2)
            .contentShape(Rectangle())
            .onTapGesture {
                withAnimation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion)) {
                    expandedModel = expanded ? nil : model.upstreamId
                }
            }
            if testedModel == model.upstreamId, let probe = store.probe, !AgentConfigText.probeSucceeded(probe),
               probe != "测试中…" {
                // The reason, in the row rather than only in a tooltip.
                Text(probe)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .textSelection(.enabled)
                    .lineLimit(3)
                    .padding(.leading, AeroricSpace.xl + AeroricSpace.l + 4)
                    .padding(.trailing, AeroricSpace.xl)
                    .padding(.bottom, AeroricSpace.m)
            }
            if expanded {
                modelEditor(index, binding)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .background(expanded ? AeroricColor.layer2.opacity(0.6) : Color.clear)
        .overlay(alignment: .top) {
            if index > 0 { HairlineDivider().padding(.leading, AeroricSpace.xl) }
        }
    }

    /// Crash 2026-10-26: `draft.models[index]` read the index eagerly, and
    /// switching to a new (empty) connection rendered one more frame with the
    /// old indices. This binding tolerates an index that is already gone.
    private func modelBinding(_ index: Int, fallback: ACC.ModelEntry) -> Binding<ACC.ModelEntry> {
        Binding(
            get: {
                guard let models = store.connectionDraft?.models, models.indices.contains(index) else { return fallback }
                return models[index]
            },
            set: { value in
                guard let count = store.connectionDraft?.models.count, index < count else { return }
                store.connectionDraft?.models[index] = value
            }
        )
    }

    private func modelEditor(_ index: Int, _ model: Binding<ACC.ModelEntry>) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Grid(alignment: .leading, horizontalSpacing: AeroricSpace.l, verticalSpacing: AeroricSpace.m) {
                GridRow {
                    fieldLabel("上游 ID")
                    ConfigTextField(placeholder: "上游 ID", text: model.upstreamId, monospaced: true, width: 240)
                }
                GridRow {
                    fieldLabel("映射名")
                    ConfigTextField(placeholder: model.wrappedValue.upstreamId, text: model.mappedName, width: 240)
                }
                GridRow {
                    fieldLabel("显示名")
                    ConfigTextField(placeholder: "—", text: model.displayName, width: 240)
                }
                GridRow {
                    fieldLabel("上下文")
                    ConfigTextField(placeholder: "未知", text: Binding(
                        get: { model.wrappedValue.contextWindow.map(String.init) ?? "" },
                        set: { model.wrappedValue.contextWindow = UInt64($0.filter(\.isNumber)) }
                    ), monospaced: true, width: 120)
                }
                GridRow {
                    fieldLabel("协议")
                    HStack(spacing: AeroricSpace.s) {
                        Toggle("同连接", isOn: Binding(
                            get: { model.wrappedValue.protocols.isEmpty },
                            set: { if $0 { model.wrappedValue.protocols = [] } }
                        ))
                        .toggleStyle(CapsuleToggleStyle())
                        ProtocolChips(selection: model.protocols, allowEmpty: true)
                    }
                }
            }
            HStack {
                Spacer()
                Button(role: .destructive) {
                    expandedModel = nil
                    if let count = store.connectionDraft?.models.count, index < count {
                        store.connectionDraft?.models.remove(at: index)
                    }
                } label: { Label("删除这个模型", systemImage: "trash") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.destructive, compact: true))
            }
        }
        .padding(.leading, AeroricSpace.xl + AeroricSpace.l + 4)
        .padding(.trailing, AeroricSpace.xl)
        .padding(.bottom, AeroricSpace.l)
    }

    private func fieldLabel(_ text: String) -> some View {
        Text(text).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            .gridColumnAlignment(.trailing)
    }

    private func test(_ model: String) {
        testedModel = model
        Task {
            await store.testConnection(model: model)
            try? await Task.sleep(for: .seconds(8))
            withAnimation(AeroricMotion.animation(AeroricMotion.slow, reduceMotion: reduceMotion)) {
                if testedModel == model { testedModel = nil }
            }
        }
    }

    // MARK: Empty and official logins (G1)

    private var emptyDetail: some View {
        VStack(spacing: AeroricSpace.m) {
            Image(systemName: "link.circle")
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(AeroricColor.textTertiary)
            Text("选择左侧的连接，或者新建一个").font(.system(size: AeroricType.headline, weight: .medium))
            Text("连接 = 一个第三方 API 端点 + 它的 key 和模型。号池和 Agent 配置会用到它们。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
            Button { newConnection() } label: { Label("新建连接", systemImage: "plus") }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .padding(.top, AeroricSpace.s)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, AeroricSpace.huge)
    }

    /// A built-in local login, shown like a connection but read-only (G1).
    private func builtinDetail(_ id: String) -> some View {
        let title = id == "codex" ? "Codex" : "Claude Code"
        let login = id == "codex" ? store.official?.codex : store.official?.claude
        return VStack(alignment: .leading, spacing: AeroricSpace.xxxl) {
            HStack(alignment: .top, spacing: AeroricSpace.l) {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    Text(title).font(.system(size: AeroricType.largeTitle, weight: .semibold))
                    Text("本机登录 · 内置连接").font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                    HStack(spacing: AeroricSpace.s) {
                        if let login {
                            officialChip(login)
                        } else {
                            StatusChip(text: "还没检测", tint: AeroricColor.textTertiary)
                        }
                    }
                }
                Spacer(minLength: 0)
                Button { Task { await store.detectOfficial() } } label: {
                    Label(store.official == nil ? "检测" : "重新检测", systemImage: "arrow.clockwise")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .accessibilityIdentifier("agent-config.detect-official")
            }
            .padding(AeroricSpace.xl)
            .configSurface(AeroricRadius.xl)
            ConfigCard("说明") {
                ConfigRow("怎么用", detail: "新建会话时不选 Agent 配置，就用这个本机登录。Aeroric 只读取登录方式和账号，不复制 token。", first: true) {
                    EmptyView()
                }
                ConfigRow("读取了哪里", detail: login.map { $0.sources.isEmpty ? "—" : $0.sources.joined(separator: "、") }
                    ?? (id == "codex" ? "~/.codex/auth.json（只取登录方式和邮箱）"
                        : "~/.claude.json 的账号邮箱，以及钥匙串里是否有“Claude Code-credentials”条目（不读取内容）")) {
                    EmptyView()
                }
            }
        }
    }

    @ViewBuilder private func officialChip(_ login: ACC.OfficialLogin) -> some View {
        if !login.installed {
            StatusChip(text: "未安装", tint: AeroricColor.textTertiary)
        } else if login.signedIn {
            StatusChip(text: modeTitle(login.mode) + (login.account.map { " · \($0)" } ?? ""),
                       systemImage: "checkmark", tint: AeroricColor.success)
        } else {
            StatusChip(text: "已安装，未登录", tint: AeroricColor.warning)
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
}

func clock(_ epoch: UInt64) -> String {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm"
    return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(epoch)))
}
