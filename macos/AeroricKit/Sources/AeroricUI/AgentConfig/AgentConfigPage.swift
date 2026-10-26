import AeroricClient
import SwiftUI

/// G: connections, pools, model routes and agent configurations, as one page
/// in the centre (sidebar → Agent 配置). Each tab is a list on the left and
/// an editor on the right; nothing is saved until the user clicks 保存 (G3).
public struct AgentConfigPage: View {
    @Bindable var store: AgentConfigStore

    public init(store: AgentConfigStore) { self.store = store }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: AeroricSpace.l) {
                CapsuleSlider(selection: $store.tab, groupTitle: "Agent 配置", identifier: "agent-config.tabs", segments: [
                    .init(value: .connections, title: "连接", systemImage: "link"),
                    .init(value: .pools, title: "号池", systemImage: "square.stack.3d.up"),
                    .init(value: .routes, title: "模型路由", systemImage: "arrow.triangle.branch"),
                    .init(value: .configs, title: "Agent 配置", systemImage: "slider.horizontal.3"),
                ])
                .fixedSize()
                Spacer(minLength: 0)
                if store.busy { ProgressView().controlSize(.small) }
                NativeIconButton(systemImage: "arrow.clockwise", label: "刷新", identifier: "agent-config.refresh") {
                    Task { await store.refresh() }
                }
            }
            .padding(.horizontal, AeroricSpace.xl)
            .padding(.vertical, AeroricSpace.m)
            if let error = store.errorText {
                banner(error, tint: AeroricColor.danger) { store.errorText = nil }
            } else if let notice = store.notice {
                banner(notice, tint: AeroricColor.textSecondary) { store.notice = nil }
            }
            HairlineDivider()
            Group {
                switch store.tab {
                case .connections: ConnectionsPane(store: store)
                case .pools: PoolsPane(store: store)
                case .routes: RoutesPane(store: store)
                case .configs: ConfigsPane(store: store)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(AeroricColor.canvas)
        .task { await store.refresh() }
        .task {
            // Cooldowns change on their own; keep the pool page honest.
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(15))
                await store.refreshStatus()
            }
        }
        .accessibilityIdentifier("agent-config.page")
    }

    private func banner(_ text: String, tint: Color, dismiss: @escaping () -> Void) -> some View {
        HStack {
            Text(text).font(.system(size: AeroricType.secondary)).foregroundStyle(tint).textSelection(.enabled)
            Spacer()
            Button(action: dismiss) { Image(systemName: "xmark") }.buttonStyle(.borderless)
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.bottom, AeroricSpace.s)
    }
}

/// Left list + right editor, shared by the four tabs.
struct MasterDetail<List: View, Detail: View>: View {
    let list: List
    let detail: Detail

    init(@ViewBuilder list: () -> List, @ViewBuilder detail: () -> Detail) {
        self.list = list()
        self.detail = detail()
    }

    var body: some View {
        HStack(spacing: 0) {
            list
                .frame(width: 240)
                .frame(maxHeight: .infinity)
                .background(AeroricColor.layer1)
            HairlineDivider(.vertical)
            ScrollView {
                detail
                    .frame(maxWidth: 760, alignment: .leading)
                    .padding(AeroricSpace.xl)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// Save / delete row at the bottom of an editor.
struct EditorActions: View {
    let dirty: Bool
    let canDelete: Bool
    let onSave: () -> Void
    let onRevert: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Button("保存", action: onSave)
                .keyboardShortcut("s")
                .disabled(!dirty)
                .accessibilityIdentifier("agent-config.save")
            Button("还原", action: onRevert).disabled(!dirty)
            if dirty {
                Text("有未保存的修改").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.warning)
            }
            Spacer()
            if canDelete {
                Button("删除", role: .destructive, action: onDelete)
                    .accessibilityIdentifier("agent-config.delete")
            }
        }
        .padding(.top, AeroricSpace.m)
    }
}

/// Tick boxes for the three wire protocols.
struct ProtocolPicker: View {
    @Binding var selection: [String]
    var allowEmpty = false

    var body: some View {
        HStack(spacing: AeroricSpace.l) {
            ForEach(AgentConfigText.protocols, id: \.self) { raw in
                Toggle(AgentConnectionsClient.protocolTitle(raw), isOn: Binding(
                    get: { selection.contains(raw) },
                    set: { on in
                        if on { if !selection.contains(raw) { selection.append(raw) } }
                        else if allowEmpty || selection.count > 1 { selection.removeAll { $0 == raw } }
                    }
                ))
                .toggleStyle(.checkbox)
            }
        }
    }
}
