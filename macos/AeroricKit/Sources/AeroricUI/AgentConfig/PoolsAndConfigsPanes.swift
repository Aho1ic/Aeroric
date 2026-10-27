import AeroricClient
import SwiftUI

// 号池 / 模型路由 / Agent 配置, in the connections tab's look (2026-10-27):
// a capsule list with 新建 on top, an overview card, dark cards, a floating
// save bar, and delete in the "…" menu behind a confirmation.

/// The overview card's chrome: title field or text, a subtitle, chips, and a
/// trailing area (switches, the "…" menu).
struct ConfigOverview<Title: View, Chips: View, Trailing: View>: View {
    let title: Title
    let subtitle: String
    let chips: Chips
    let trailing: Trailing

    init(subtitle: String, @ViewBuilder title: () -> Title, @ViewBuilder chips: () -> Chips,
         @ViewBuilder trailing: () -> Trailing) {
        self.subtitle = subtitle
        self.title = title()
        self.chips = chips()
        self.trailing = trailing()
    }

    var body: some View {
        HStack(alignment: .top, spacing: AeroricSpace.l) {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                title
                Text(subtitle)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                HStack(spacing: AeroricSpace.s) { chips }
            }
            Spacer(minLength: 0)
            HStack(spacing: AeroricSpace.m) { trailing }
        }
        .padding(AeroricSpace.xl)
        .configSurface(AeroricRadius.xl)
    }
}

/// The editable big name in an overview card.
struct ConfigNameField: View {
    let placeholder: String
    @Binding var text: String
    let identifier: String

    var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .font(.system(size: AeroricType.largeTitle, weight: .semibold))
            .accessibilityIdentifier(identifier)
    }
}

/// "…" with one destructive item.
struct ConfigMoreMenu: View {
    let deleteTitle: String
    let canDelete: Bool
    let onDelete: () -> Void

    var body: some View {
        Menu {
            Button(deleteTitle, role: .destructive, action: onDelete).disabled(!canDelete)
        } label: {
            Image(systemName: "ellipsis.circle").font(.system(size: 16))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .accessibilityIdentifier("agent-config.more")
    }
}

/// An empty right-hand side with an icon, a sentence and a primary button.
struct ConfigEmptyDetail: View {
    let systemImage: String
    let title: String
    let message: String
    let actionTitle: String?
    var action: () -> Void = {}

    var body: some View {
        VStack(spacing: AeroricSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(AeroricColor.textTertiary)
            Text(title).font(.system(size: AeroricType.headline, weight: .medium))
            Text(message)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            if let actionTitle {
                Button(action: action) { Label(actionTitle, systemImage: "plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .padding(.top, AeroricSpace.s)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, AeroricSpace.huge)
    }
}

/// A small −/value/+ control for integers.
struct ConfigStepper: View {
    let title: String
    @Binding var value: Int
    let range: ClosedRange<Int>

    var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            Text(title).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            Button { value = max(range.lowerBound, value - 1) } label: { Image(systemName: "minus") }
                .buttonStyle(.borderless)
                .disabled(value <= range.lowerBound)
            Text("\(value)")
                .font(AeroricType.mono(AeroricType.secondary))
                .frame(minWidth: 24)
            Button { value = min(range.upperBound, value + 1) } label: { Image(systemName: "plus") }
                .buttonStyle(.borderless)
                .disabled(value >= range.upperBound)
        }
        .padding(.horizontal, AeroricSpace.s)
        .padding(.vertical, 3)
        .background(AeroricColor.layer2, in: Capsule(style: .continuous))
    }
}

// MARK: - 号池

/// 号池: members, strategy, stickiness, and each member's live state.
struct PoolsPane: View {
    @Bindable var store: AgentConfigStore
    @State private var confirmDelete = false
    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            ConfigListColumn("号池", newTitle: "新建", newIdentifier: "agent-config.new-pool",
                             onNew: { store.newPool() }) {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
                        if store.document.pools.isEmpty {
                            Text("还没有号池，点右上角“新建”。")
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .padding(.horizontal, AeroricSpace.l)
                        }
                        ForEach(store.document.pools) { pool in
                            HoverCapsuleRow(selected: store.selectedPool == pool.id, height: 50,
                                            horizontalPadding: AeroricSpace.s) {
                                store.selectPool(pool.id)
                            } label: {
                                ConfigListItem(title: pool.label,
                                               subtitle: "\(AgentConfigText.strategyTitle(pool.strategy)) · \(pool.members.count) 个成员",
                                               status: poolStatus(pool))
                            }
                            .accessibilityIdentifier("agent-config.pool.\(pool.id)")
                        }
                    }
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.bottom, AeroricSpace.l)
                }
                .scrollIndicators(.hidden)
            }
        } detail: {
            VStack(alignment: .leading, spacing: AeroricSpace.xxxl) {
                if store.poolDraft != nil {
                    overview
                    settingsCard
                    membersCard
                } else {
                    ConfigEmptyDetail(systemImage: "square.stack.3d.up", title: "选择左侧的号池，或者新建一个",
                                      message: "号池把多条连接放在一起，按策略轮换；某条连接出错会自动冷却并换下一条。",
                                      actionTitle: "新建号池") { store.newPool() }
                }
            }
            .padding(.bottom, 72)
        } overlay: {
            FloatingSaveBar(visible: store.poolDirty,
                            onRevert: { store.selectPool(store.selectedPool) },
                            onSave: { Task { await store.savePool() } })
        }
        .confirmationDialog("删除这个号池？", isPresented: $confirmDelete) {
            Button("删除", role: .destructive) { Task { await store.deletePool() } }
                .accessibilityIdentifier("agent-config.delete")
            Button("取消", role: .cancel) {}
        } message: {
            Text("成员连接本身不会被删除。")
        }
    }

    private var draft: Binding<ACC.Pool> {
        Binding(get: { store.poolDraft ?? ACC.Pool(id: "", label: "") }, set: { store.poolDraft = $0 })
    }

    /// Green when some member is ready, orange when all are cooling, grey when empty.
    private func poolStatus(_ pool: ACC.Pool) -> Color {
        guard !pool.members.isEmpty else { return AeroricColor.textTertiary }
        let ready = pool.members.contains { store.cooldowns(forConnection: $0.connectionId).isEmpty }
        return ready ? AeroricColor.success : AeroricColor.warning
    }

    private var overview: some View {
        let pool = draft.wrappedValue
        let cooling = pool.members.filter { !store.cooldowns(forConnection: $0.connectionId).isEmpty }.count
        return ConfigOverview(subtitle: AgentConfigText.strategyTitle(pool.strategy)) {
            ConfigNameField(placeholder: "号池名称", text: draft.label, identifier: "agent-config.pool-name")
        } chips: {
            StatusChip(text: "\(pool.members.count) 个成员", systemImage: "person.2.fill")
            if cooling > 0 {
                StatusChip(text: "\(cooling) 个冷却中", systemImage: "snowflake", tint: AeroricColor.warning)
            }
            if pool.stickySessions {
                StatusChip(text: "会话粘滞", systemImage: "pin.fill", tint: AeroricColor.textSecondary)
            }
        } trailing: {
            ConfigMoreMenu(deleteTitle: "删除号池…", canDelete: store.selectedPool != nil) { confirmDelete = true }
        }
    }

    private var settingsCard: some View {
        ConfigCard("策略") {
            ConfigRow("轮换策略", detail: "按优先级：小的先用；轮询只在当前最高的可用优先级里进行。", first: true) {
                Picker("策略", selection: draft.strategy) {
                    ForEach(AgentConfigText.strategies, id: \.self) { Text(AgentConfigText.strategyTitle($0)).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
            }
            ConfigRow("会话粘滞", detail: "同一个会话尽量留在同一个成员上，省 prompt 缓存；只在限流、额度或鉴权出错后才换。") {
                Toggle("", isOn: draft.stickySessions).labelsHidden().toggleStyle(.switch)
            }
            if draft.wrappedValue.stickySessions {
                ConfigRow("粘滞时长", detail: "单位：秒。") {
                    ConfigTextField(placeholder: "3600", text: Binding(
                        get: { String(draft.wrappedValue.stickyTtlSeconds) },
                        set: { draft.wrappedValue.stickyTtlSeconds = UInt64($0.filter(\.isNumber)) ?? 0 }
                    ), monospaced: true, width: 120)
                }
            }
        }
    }

    private var membersCard: some View {
        let available = store.document.connections.filter { c in
            !draft.wrappedValue.members.contains { $0.connectionId == c.id }
        }
        return ConfigCard("成员") {
            Menu {
                ForEach(available) { connection in
                    Button(connection.label) { store.poolDraft?.members.append(.init(connectionId: connection.id)) }
                }
            } label: {
                Label("添加成员", systemImage: "plus")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(available.isEmpty)
            .accessibilityIdentifier("agent-config.add-member")
        } content: {
            if draft.wrappedValue.members.isEmpty {
                Text("还没有成员。点右上角“添加成员”，从已保存的连接里选。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, AeroricSpace.xxxl)
            } else {
                ForEach(Array(draft.wrappedValue.members.enumerated()), id: \.offset) { index, member in
                    memberRow(index, member)
                }
            }
        }
    }

    private func memberBinding(_ index: Int, fallback: ACC.PoolMember) -> Binding<ACC.PoolMember> {
        Binding(
            get: {
                guard let members = store.poolDraft?.members, members.indices.contains(index) else { return fallback }
                return members[index]
            },
            set: { value in
                guard let count = store.poolDraft?.members.count, index < count else { return }
                store.poolDraft?.members[index] = value
            }
        )
    }

    private func memberRow(_ index: Int, _ member: ACC.PoolMember) -> some View {
        let binding = memberBinding(index, fallback: member)
        let label = store.document.connections.first { $0.id == member.connectionId }?.label ?? "（已删除）"
        let cooling = store.cooldowns(forConnection: member.connectionId)
        return HStack(spacing: AeroricSpace.m) {
            VStack(alignment: .leading, spacing: 3) {
                Text(label).font(.system(size: AeroricType.body, weight: .medium)).lineLimit(1)
                if let first = cooling.first {
                    StatusChip(text: "冷却到 \(clock(first.untilEpochS)) · \(AgentConfigText.reasonTitle(first.reason))",
                               systemImage: "snowflake", tint: AeroricColor.warning)
                } else {
                    StatusChip(text: "可用", systemImage: "checkmark", tint: AeroricColor.success)
                }
            }
            Spacer(minLength: AeroricSpace.m)
            ConfigStepper(title: "优先级", value: binding.priority, range: -99...99)
            ConfigStepper(title: "权重", value: Binding(
                get: { Int(binding.wrappedValue.weight) },
                set: { binding.wrappedValue.weight = UInt32(max(1, min(100, $0))) }
            ), range: 1...100)
            Toggle("免费", isOn: binding.free)
                .toggleStyle(CapsuleToggleStyle())
                .help("免费优先策略会先用免费成员")
            Button {
                if let count = store.poolDraft?.members.count, index < count {
                    store.poolDraft?.members.remove(at: index)
                }
            } label: { Image(systemName: "minus.circle") }
                .buttonStyle(.borderless)
                .help("从号池移除（不删除连接）")
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.vertical, AeroricSpace.m + 2)
        .overlay(alignment: .top) {
            if index > 0 { HairlineDivider().padding(.leading, AeroricSpace.xl) }
        }
    }
}

// MARK: - 模型路由

/// 模型路由: read-only, grouped by mapped name (Q25).
struct RoutesPane: View {
    let store: AgentConfigStore
    @State private var query = ""

    var body: some View {
        let routes = AgentConfigText.routes(store.document)
            .filter { AgentConfigText.fuzzyMatch(query, $0.name) }
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.xxxl) {
                ConfigOverview(subtitle: "映射名相同的模型组成一条路由；选这个模型时，网关按策略依次尝试这些渠道，某条出错就自动换下一条。") {
                    Text("模型路由").font(.system(size: AeroricType.largeTitle, weight: .semibold))
                } chips: {
                    StatusChip(text: "\(AgentConfigText.routes(store.document).count) 条路由", systemImage: "arrow.triangle.branch")
                    StatusChip(text: "只读 · 改映射名请到“连接”", tint: AeroricColor.textTertiary)
                } trailing: {
                    EmptyView()
                }
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "magnifyingglass").foregroundStyle(AeroricColor.textTertiary)
                    TextField("搜索模型（支持模糊匹配）", text: $query).textFieldStyle(.plain)
                }
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s + 1)
                .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
                if routes.isEmpty {
                    ConfigEmptyDetail(systemImage: "arrow.triangle.branch",
                                      title: query.isEmpty ? "还没有任何模型" : "没有匹配“\(query)”的路由",
                                      message: "在“连接”里添加模型后，这里会按映射名自动分组。", actionTitle: nil)
                }
                ForEach(routes) { route in
                    ConfigCard(route.name) {
                        StatusChip(text: "\(route.channels.count) 条渠道")
                    } content: {
                        ForEach(Array(route.channels.enumerated()), id: \.offset) { index, channel in
                            HStack(spacing: AeroricSpace.m) {
                                Text(channel.connectionLabel).font(.system(size: AeroricType.body, weight: .medium))
                                Text(channel.upstreamId).font(AeroricType.mono(AeroricType.secondary))
                                    .foregroundStyle(AeroricColor.textSecondary).lineLimit(1)
                                Spacer(minLength: AeroricSpace.m)
                                ForEach(channel.protocols, id: \.self) {
                                    StatusChip(text: AgentConnectionsClient.protocolTitle($0))
                                }
                                if !channel.enabled {
                                    StatusChip(text: "已停用", systemImage: "pause.fill", tint: AeroricColor.warning)
                                }
                            }
                            .padding(.horizontal, AeroricSpace.xl)
                            .padding(.vertical, AeroricSpace.m + 2)
                            .overlay(alignment: .top) {
                                if index > 0 { HairlineDivider().padding(.leading, AeroricSpace.xl) }
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: 820, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .accessibilityIdentifier("agent-config.routes")
    }
}

// MARK: - Agent 配置

/// Agent 配置: harness + target + defaults, decoupled from connections (Q30).
/// Saved connections are offered in the new-session picker on their own; a
/// configuration here is for pools, routing and fallbacks.
struct ConfigsPane: View {
    @Bindable var store: AgentConfigStore
    @State private var confirmDelete = false
    private typealias ACC = AgentConnectionsClient

    var body: some View {
        MasterDetail {
            ConfigListColumn("Agent 配置", newTitle: "新建", newIdentifier: "agent-config.new-config",
                             onNew: { store.newConfig() }) {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
                        if store.document.configs.isEmpty {
                            Text("还没有自定义配置。已保存的连接会直接出现在新建任务里；需要号池、自动路由或备用模型时再新建。")
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .fixedSize(horizontal: false, vertical: true)
                                .padding(.horizontal, AeroricSpace.l)
                        }
                        ForEach(store.document.configs) { config in
                            HoverCapsuleRow(selected: store.selectedConfig == config.id, height: 50,
                                            horizontalPadding: AeroricSpace.s) {
                                Task { await store.selectConfig(config.id) }
                            } label: {
                                ConfigListItem(title: config.label,
                                               subtitle: "\(AgentConfigText.harnessTitle(config.harness)) · \(targetTitle(config.target))")
                            }
                            .accessibilityIdentifier("agent-config.config.\(config.id)")
                        }
                    }
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.bottom, AeroricSpace.l)
                }
                .scrollIndicators(.hidden)
            }
        } detail: {
            VStack(alignment: .leading, spacing: AeroricSpace.xxxl) {
                if store.configDraft != nil {
                    overview
                    basicsCard
                    if AgentConfigText.supportsFallback(draft.wrappedValue.harness) { fallbackCard }
                    channelsCard
                } else {
                    ConfigEmptyDetail(systemImage: "slider.horizontal.3", title: "选择左侧的配置，或者新建一个",
                                      message: "Agent 配置 = Harness + 用哪些连接（单个连接、号池，或按模型自动路由）+ 默认模型和备用模型。",
                                      actionTitle: "新建 Agent 配置") { store.newConfig() }
                }
            }
            .padding(.bottom, 72)
        } overlay: {
            FloatingSaveBar(visible: store.configDirty,
                            onRevert: { Task { await store.selectConfig(store.selectedConfig) } },
                            onSave: { Task { await store.saveConfig() } })
        }
        .confirmationDialog("删除这个 Agent 配置？", isPresented: $confirmDelete) {
            Button("删除", role: .destructive) { Task { await store.deleteConfig() } }
                .accessibilityIdentifier("agent-config.delete")
            Button("取消", role: .cancel) {}
        } message: {
            Text("用到的连接和号池不会被删除。")
        }
    }

    private func targetTitle(_ target: ACC.ConfigTarget) -> String {
        switch target.kind {
        case "connection":
            store.document.connections.first { $0.id == target.connectionId }?.label ?? "单个连接"
        case "pool":
            store.document.pools.first { $0.id == target.poolId }.map { "号池 \($0.label)" } ?? "号池"
        default: "自动路由"
        }
    }

    private var draft: Binding<ACC.AgentConfig> {
        Binding(get: { store.configDraft ?? ACC.AgentConfig(id: "", label: "", harness: "codex", target: .init(kind: "route")) },
                set: { store.configDraft = $0 })
    }

    private var overview: some View {
        let config = draft.wrappedValue
        return ConfigOverview(subtitle: "\(AgentConfigText.harnessTitle(config.harness)) · \(targetTitle(config.target))") {
            ConfigNameField(placeholder: "配置名称", text: draft.label, identifier: "agent-config.config-name")
        } chips: {
            if !config.defaultModel.isEmpty {
                StatusChip(text: "默认 \(config.defaultModel)", systemImage: "cpu")
            }
            if !config.fallbacks.isEmpty {
                StatusChip(text: "\(config.fallbacks.count) 个备用模型", systemImage: "arrow.down.circle")
            }
            if !AgentConfigText.supportsFallback(config.harness) {
                StatusChip(text: "暂不能经网关启动", systemImage: "exclamationmark.triangle", tint: AeroricColor.warning)
            }
        } trailing: {
            ConfigMoreMenu(deleteTitle: "删除配置…", canDelete: store.selectedConfig != nil) { confirmDelete = true }
        }
    }

    private var basicsCard: some View {
        ConfigCard("基本") {
            ConfigRow("Harness", first: true) {
                Picker("Harness", selection: draft.harness) {
                    ForEach(AgentConfigText.harnesses, id: \.self) { Text(AgentConfigText.harnessTitle($0)).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
            }
            ConfigRow("目标", detail: "单个连接：只用它；号池：按号池策略轮换；自动路由：所有映射到同名模型的连接。") {
                CapsuleSlider(selection: draft.target.kind, groupTitle: "目标", identifier: "agent-config.target", segments: [
                    .init(value: "connection", title: "单个连接", systemImage: "link"),
                    .init(value: "pool", title: "号池", systemImage: "square.stack.3d.up"),
                    .init(value: "route", title: "自动路由", systemImage: "arrow.triangle.branch"),
                ])
                .fixedSize()
            }
            switch draft.wrappedValue.target.kind {
            case "connection":
                ConfigRow("连接") {
                    Picker("连接", selection: Binding(get: { draft.wrappedValue.target.connectionId ?? "" },
                                                    set: { store.configDraft?.target.connectionId = $0 })) {
                        Text("请选择").tag("")
                        ForEach(store.document.connections) { Text($0.label).tag($0.id) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
            default:
                ConfigRow(draft.wrappedValue.target.kind == "pool" ? "号池" : "排序策略来自号池",
                          detail: draft.wrappedValue.target.kind == "pool" ? nil : "可选；不选就按优先级。") {
                    Picker("号池", selection: Binding(get: { draft.wrappedValue.target.poolId ?? "" },
                                                    set: { store.configDraft?.target.poolId = $0.isEmpty ? nil : $0 })) {
                        Text(draft.wrappedValue.target.kind == "pool" ? "请选择" : "不用（按优先级）").tag("")
                        ForEach(store.document.pools) { Text($0.label).tag($0.id) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
            }
            ConfigRow("默认模型", detail: "新建任务时默认选中的模型。") {
                Picker("默认模型", selection: draft.defaultModel) {
                    Text("不指定").tag("")
                    ForEach(AgentConfigText.modelNames(for: draft.wrappedValue, in: store.document), id: \.self) {
                        Text($0).tag($0)
                    }
                }
                .labelsHidden()
                .fixedSize()
            }
        }
    }

    /// R8: ordered fallback models, tried once the default has no ready channel.
    private var fallbackCard: some View {
        let list = draft.wrappedValue.fallbacks
        let choices = AgentConfigText.fallbackChoices(for: draft.wrappedValue, in: store.document)
        return ConfigCard("备用模型") {
            Menu {
                ForEach(choices, id: \.self) { name in
                    Button(name) { store.configDraft?.fallbacks.append(name) }
                }
            } label: {
                Label("添加", systemImage: "plus")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(choices.isEmpty || list.count >= AgentConfigText.maxFallbacks)
            .accessibilityIdentifier("agent-config.fallback.add")
        } content: {
            Text("主模型所有通道都失败或冷却时，按顺序改用这些模型；主模型恢复后自动切回。最多 \(AgentConfigText.maxFallbacks) 个。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .padding(.horizontal, AeroricSpace.xl)
                .padding(.vertical, AeroricSpace.m)
            ForEach(Array(list.enumerated()), id: \.element) { index, name in
                HStack(spacing: AeroricSpace.m) {
                    Text("\(index + 1)")
                        .font(AeroricType.mono(AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .frame(width: 18, height: 18)
                        .background(AeroricColor.layer2, in: Circle())
                    Text(name).font(.system(size: AeroricType.body, weight: .medium))
                    Spacer()
                    NativeIconButton(systemImage: "chevron.up", label: "上移",
                                     identifier: "agent-config.fallback.up.\(index)") { move(index, by: -1) }
                        .disabled(index == 0)
                    NativeIconButton(systemImage: "chevron.down", label: "下移",
                                     identifier: "agent-config.fallback.down.\(index)") { move(index, by: 1) }
                        .disabled(index == list.count - 1)
                    NativeIconButton(systemImage: "minus.circle", label: "移除",
                                     identifier: "agent-config.fallback.remove.\(index)") {
                        if let count = store.configDraft?.fallbacks.count, index < count {
                            store.configDraft?.fallbacks.remove(at: index)
                        }
                    }
                }
                .padding(.horizontal, AeroricSpace.xl)
                .padding(.vertical, AeroricSpace.s + 2)
                .overlay(alignment: .top) { HairlineDivider().padding(.leading, AeroricSpace.xl) }
            }
        }
    }

    private func move(_ index: Int, by offset: Int) {
        guard let current = store.configDraft?.fallbacks else { return }
        store.configDraft?.fallbacks = AgentConfigText.moving(current, at: index, by: offset)
    }

    @ViewBuilder private var channelsCard: some View {
        if let candidates = store.candidates, !store.configDirty {
            ConfigCard("能用的渠道\(candidates.model.isEmpty ? "" : "（\(candidates.model)）")") {
                if candidates.channels.isEmpty {
                    Text("没有可用渠道。")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                        .padding(.horizontal, AeroricSpace.xl)
                        .padding(.vertical, AeroricSpace.l)
                }
                ForEach(Array(candidates.channels.enumerated()), id: \.offset) { index, channel in
                    HStack(spacing: AeroricSpace.m) {
                        Text(store.document.connections.first { $0.id == channel.connectionId }?.label ?? channel.connectionId)
                            .font(.system(size: AeroricType.body, weight: .medium))
                        Text(channel.upstreamModel).font(AeroricType.mono(AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                        Spacer()
                        StatusChip(text: channel.reach)
                    }
                    .padding(.horizontal, AeroricSpace.xl)
                    .padding(.vertical, AeroricSpace.m + 2)
                    .overlay(alignment: .top) {
                        if index > 0 { HairlineDivider().padding(.leading, AeroricSpace.xl) }
                    }
                }
                ForEach(Array(candidates.excluded.enumerated()), id: \.offset) { _, item in
                    Text("\(store.document.connections.first { $0.id == item.id }?.label ?? item.id)：\(AgentConfigText.excludedTitle(item.reason))")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .padding(.horizontal, AeroricSpace.xl)
                        .padding(.vertical, AeroricSpace.xs)
                }
            }
        }
    }
}
