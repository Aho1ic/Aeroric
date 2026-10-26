import AeroricClient
import SwiftUI

/// 号池: members, strategy, stickiness, and each member's live state.
struct PoolsPane: View {
    @Bindable var store: AgentConfigStore
    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            VStack(spacing: 0) {
                List(selection: Binding(get: { store.selectedPool }, set: { store.selectPool($0) })) {
                    ForEach(store.document.pools) { pool in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(pool.label).lineLimit(1)
                            Text("\(AgentConfigText.strategyTitle(pool.strategy)) · \(pool.members.count) 个成员")
                                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                        }
                        .tag(Optional(pool.id))
                    }
                }
                .listStyle(.sidebar)
                .scrollContentBackground(.hidden)
                HairlineDivider()
                Button { store.newPool() } label: { Label("新建号池", systemImage: "plus") }
                    .buttonStyle(.borderless)
                    .padding(AeroricSpace.m)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        } detail: {
            if store.poolDraft != nil {
                editor
            } else {
                Text("号池把多条连接放在一起，按策略轮换；某条连接出错会自动冷却并换下一条。")
                    .foregroundStyle(AeroricColor.textSecondary)
            }
        }
    }

    private var draft: Binding<ACC.Pool> {
        Binding(get: { store.poolDraft ?? ACC.Pool(id: "", label: "") }, set: { store.poolDraft = $0 })
    }

    @ViewBuilder private var editor: some View {
        SettingsCard("号池") {
            SettingsRow("名称") { TextField("名称", text: draft.label).frame(width: 280) }
            SettingsRow("策略") {
                Picker("策略", selection: draft.strategy) {
                    ForEach(AgentConfigText.strategies, id: \.self) { Text(AgentConfigText.strategyTitle($0)).tag($0) }
                }
                .labelsHidden()
                .frame(width: 200)
            }
            SettingsRow("会话粘滞", detail: "同一个会话尽量留在同一个成员上，省 prompt 缓存；只在限流、额度或鉴权出错后才换。") {
                Toggle("", isOn: draft.stickySessions).labelsHidden().toggleStyle(.switch)
            }
            if draft.wrappedValue.stickySessions {
                SettingsRow("粘滞时长（秒）") {
                    TextField("3600", value: draft.stickyTtlSeconds, format: .number).frame(width: 100)
                }
            }
        }
        SettingsCard("成员") {
            Grid(alignment: .leading, horizontalSpacing: AeroricSpace.m, verticalSpacing: AeroricSpace.s) {
                GridRow {
                    Text("连接"); Text("优先级"); Text("权重"); Text("免费"); Text("状态"); Text("")
                }
                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                ForEach(draft.members.indices, id: \.self) { index in
                    memberRow(index)
                }
            }
            Menu("添加成员") {
                ForEach(store.document.connections.filter { c in !draft.wrappedValue.members.contains { $0.connectionId == c.id } }) { connection in
                    Button(connection.label) { store.poolDraft?.members.append(.init(connectionId: connection.id)) }
                }
            }
            .fixedSize()
            Text("优先级数字小的先用；轮询只在当前最高的可用优先级里进行。权重 1–100。")
                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
        }
        EditorActions(
            dirty: store.poolDirty,
            canDelete: store.selectedPool != nil,
            onSave: { Task { await store.savePool() } },
            onRevert: { store.selectPool(store.selectedPool) },
            onDelete: { Task { await store.deletePool() } }
        )
    }

    private func memberRow(_ index: Int) -> some View {
        let member = draft.members[index]
        let id = member.wrappedValue.connectionId
        let label = store.document.connections.first { $0.id == id }?.label ?? "（已删除）\(id)"
        let cooling = store.cooldowns(forConnection: id)
        return GridRow {
            Text(label).lineLimit(1)
            TextField("0", value: member.priority, format: .number).frame(width: 60)
            TextField("1", value: member.weight, format: .number).frame(width: 60)
            Toggle("", isOn: member.free).labelsHidden()
            Group {
                if let first = cooling.first {
                    Text("冷却到 \(clock(first.untilEpochS)) · \(AgentConfigText.reasonTitle(first.reason))\(first.model.map { "（\($0)）" } ?? "")")
                        .foregroundStyle(AeroricColor.warning)
                } else {
                    Text("可用").foregroundStyle(AeroricColor.success)
                }
            }
            .font(.system(size: AeroricType.caption))
            Button { store.poolDraft?.members.remove(at: index) } label: { Image(systemName: "minus.circle") }
                .buttonStyle(.borderless)
        }
    }
}

/// 模型路由: read-only, grouped by mapped name (Q25).
struct RoutesPane: View {
    let store: AgentConfigStore

    var body: some View {
        let routes = AgentConfigText.routes(store.document)
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                Text("映射名相同的模型组成一条路由。选这个模型时，网关按 Agent 配置的策略依次尝试这些渠道，某条出错就自动换下一条。这里只读，改映射名请到“连接”页。")
                    .font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
                if routes.isEmpty {
                    Text("还没有任何模型。").foregroundStyle(AeroricColor.textTertiary)
                }
                ForEach(routes) { route in
                    SettingsCard(route.name) {
                        ForEach(route.channels, id: \.self) { channel in
                            HStack {
                                Text(channel.connectionLabel)
                                Text(channel.upstreamId).foregroundStyle(AeroricColor.textSecondary)
                                Spacer()
                                Text(channel.protocols.map(AgentConnectionsClient.protocolTitle).joined(separator: " / "))
                                    .foregroundStyle(AeroricColor.textTertiary)
                                if !channel.enabled { Text("已停用").foregroundStyle(AeroricColor.warning) }
                            }
                            .font(.system(size: AeroricType.secondary))
                        }
                    }
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(AeroricSpace.xl)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("agent-config.routes")
    }
}

/// Agent 配置: harness + target + defaults, decoupled from connections (Q30).
struct ConfigsPane: View {
    @Bindable var store: AgentConfigStore
    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            VStack(spacing: 0) {
                List(selection: Binding(get: { store.selectedConfig }, set: { id in Task { await store.selectConfig(id) } })) {
                    ForEach(store.document.configs) { config in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(config.label).lineLimit(1)
                            Text(AgentConfigText.harnessTitle(config.harness))
                                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                        }
                        .tag(Optional(config.id))
                    }
                }
                .listStyle(.sidebar)
                .scrollContentBackground(.hidden)
                HairlineDivider()
                Button { store.newConfig() } label: { Label("新建 Agent 配置", systemImage: "plus") }
                    .buttonStyle(.borderless)
                    .padding(AeroricSpace.m)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        } detail: {
            if store.configDraft != nil {
                editor
            } else {
                Text("Agent 配置 = Harness + 用哪些连接（单个连接、号池，或按模型自动路由）+ 默认模型。新建会话时先选 Harness，再选配置。")
                    .foregroundStyle(AeroricColor.textSecondary)
            }
        }
    }

    private var draft: Binding<ACC.AgentConfig> {
        Binding(get: { store.configDraft ?? ACC.AgentConfig(id: "", label: "", harness: "codex", target: .init(kind: "route")) },
                set: { store.configDraft = $0 })
    }

    @ViewBuilder private var editor: some View {
        SettingsCard("Agent 配置") {
            SettingsRow("名称") { TextField("名称", text: draft.label).frame(width: 280) }
            SettingsRow("Harness") {
                Picker("Harness", selection: draft.harness) {
                    ForEach(AgentConfigText.harnesses, id: \.self) { Text(AgentConfigText.harnessTitle($0)).tag($0) }
                }
                .labelsHidden()
                .frame(width: 200)
            }
            SettingsRow("目标") {
                Picker("目标", selection: draft.target.kind) {
                    Text("单个连接").tag("connection")
                    Text("号池").tag("pool")
                    Text("按模型自动路由").tag("route")
                }
                .labelsHidden()
                .frame(width: 200)
            }
            switch draft.wrappedValue.target.kind {
            case "connection":
                SettingsRow("连接") {
                    Picker("连接", selection: Binding(get: { draft.wrappedValue.target.connectionId ?? "" },
                                                    set: { store.configDraft?.target.connectionId = $0 })) {
                        Text("请选择").tag("")
                        ForEach(store.document.connections) { Text($0.label).tag($0.id) }
                    }
                    .labelsHidden().frame(width: 240)
                }
            default:
                SettingsRow(draft.wrappedValue.target.kind == "pool" ? "号池" : "排序策略来自号池（可选）") {
                    Picker("号池", selection: Binding(get: { draft.wrappedValue.target.poolId ?? "" },
                                                    set: { store.configDraft?.target.poolId = $0.isEmpty ? nil : $0 })) {
                        Text(draft.wrappedValue.target.kind == "pool" ? "请选择" : "不用（按优先级）").tag("")
                        ForEach(store.document.pools) { Text($0.label).tag($0.id) }
                    }
                    .labelsHidden().frame(width: 240)
                }
            }
            SettingsRow("默认模型") {
                Picker("默认模型", selection: draft.defaultModel) {
                    Text("不指定").tag("")
                    ForEach(AgentConfigText.modelNames(for: draft.wrappedValue, in: store.document), id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden().frame(width: 240)
            }
        }
        if let candidates = store.candidates, !store.configDirty {
            SettingsCard("能用的渠道\(candidates.model.isEmpty ? "" : "（\(candidates.model)）")") {
                if candidates.channels.isEmpty {
                    Text("没有可用渠道。").foregroundStyle(AeroricColor.warning)
                }
                ForEach(candidates.channels, id: \.self) { channel in
                    HStack {
                        Text(store.document.connections.first { $0.id == channel.connectionId }?.label ?? channel.connectionId)
                        Text(channel.upstreamModel).foregroundStyle(AeroricColor.textSecondary)
                        Spacer()
                        Text(channel.reach).foregroundStyle(AeroricColor.textTertiary)
                    }
                    .font(.system(size: AeroricType.secondary))
                }
                ForEach(Array(candidates.excluded.enumerated()), id: \.offset) { _, item in
                    Text("\(store.document.connections.first { $0.id == item.id }?.label ?? item.id)：\(AgentConfigText.excludedTitle(item.reason))")
                        .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                }
            }
        }
        EditorActions(
            dirty: store.configDirty,
            canDelete: store.selectedConfig != nil,
            onSave: { Task { await store.saveConfig() } },
            onRevert: { Task { await store.selectConfig(store.selectedConfig) } },
            onDelete: { Task { await store.deleteConfig() } }
        )
    }
}
