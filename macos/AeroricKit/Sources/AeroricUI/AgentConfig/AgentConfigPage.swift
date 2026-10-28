import AeroricClient
import SwiftUI

/// G: connections, pools, model routes and agent configurations, as one page
/// in the centre (sidebar → Agent 配置). Each tab is a list on the left and
/// an editor on the right; nothing is saved until the user clicks 保存 (G3).
public struct AgentConfigPage: View {
    @Bindable var store: AgentConfigStore
    @Environment(AgentUpgradeBook.self) private var upgrades: AgentUpgradeBook?

    public init(store: AgentConfigStore) { self.store = store }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: AeroricSpace.l) {
                CapsuleSlider(selection: $store.tab, groupTitle: "Agent 配置", identifier: "agent-config.tabs", segments: [
                    .init(value: .connections, title: "连接", systemImage: "link"),
                    .init(value: .pools, title: "号池", systemImage: "square.stack.3d.up"),
                    .init(value: .routes, title: "模型路由", systemImage: "arrow.triangle.branch"),
                    .init(value: .configs, title: "Agent 预设", systemImage: "slider.horizontal.3"),
                    .init(value: .upgrades, title: "Agent 升级", systemImage: "arrow.up.circle"),
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
            HairlineDivider()
            Group {
                switch store.tab {
                case .connections: ConnectionsPane(store: store)
                case .pools: PoolsPane(store: store)
                case .routes: RoutesPane(store: store)
                case .configs: ConfigsPane(store: store)
                case .upgrades: AgentUpgradePane(book: upgrades)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        // 2026-10-27: no message row under the tabs. Errors and notices float
        // up as a capsule over the bottom and fade on their own.
        .overlay(alignment: .bottom) { toast }
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

    @ViewBuilder private var toast: some View {
        let message = store.errorText ?? store.notice
        ZStack {
            if let message {
                HStack(spacing: AeroricSpace.m) {
                    Image(systemName: store.errorText != nil ? "exclamationmark.circle.fill" : "checkmark.circle.fill")
                        .foregroundStyle(store.errorText != nil ? AeroricColor.danger : AeroricColor.success)
                    Text(message).font(.system(size: AeroricType.secondary)).textSelection(.enabled).lineLimit(2)
                    Button { store.errorText = nil; store.notice = nil } label: { Image(systemName: "xmark") }
                        .buttonStyle(.borderless)
                }
                .padding(.horizontal, AeroricSpace.xl)
                .padding(.vertical, AeroricSpace.s + 2)
                .glassSurface(.floatingCard, in: Capsule(style: .continuous))
                .shadow(color: AeroricColor.shadow, radius: 14, y: 5)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                .task(id: message) {
                    try? await Task.sleep(for: .seconds(store.errorText != nil ? 8 : 4))
                    if store.errorText == message { store.errorText = nil }
                    if store.notice == message { store.notice = nil }
                }
            }
        }
        // Above the floating save bar.
        .padding(.bottom, 76)
        .animation(.easeOut(duration: AeroricMotion.standard), value: message)
    }
}

/// Left list + right editor, shared by the four tabs. `overlay` floats over
/// the bottom of the editor (the save bar) without scrolling with it.
struct MasterDetail<List: View, Detail: View, Overlay: View>: View {
    let list: List
    let detail: Detail
    let overlay: Overlay
    /// An empty state: the detail sits in the middle of the pane (2026-10-28).
    var centered = false

    init(centered: Bool = false, @ViewBuilder list: () -> List, @ViewBuilder detail: () -> Detail,
         @ViewBuilder overlay: () -> Overlay) {
        self.centered = centered
        self.list = list()
        self.detail = detail()
        self.overlay = overlay()
    }

    var body: some View {
        HStack(spacing: 0) {
            // One surface colour across the list and the editor: a lighter
            // list column made the window's 6pt sidebar strip read as a dark
            // seam in dark mode (2026-10-26).
            list
                .frame(width: 260)
                .frame(maxHeight: .infinity, alignment: .top)
            HairlineDivider(.vertical)
            GeometryReader { viewport in
                ScrollView {
                    // Centred, so the margins either side are even rather than a
                    // wide dark band on the right; an empty state also sits
                    // halfway down rather than near the top.
                    detail
                        .frame(maxWidth: 820, alignment: .leading)
                        .padding(.horizontal, AeroricSpace.huge)
                        .padding(.vertical, AeroricSpace.xxxl)
                        .frame(maxWidth: .infinity)
                        .frame(minHeight: centered ? viewport.size.height : 0)
                }
                .scrollIndicators(.hidden)
            }
            .overlay(alignment: .bottom) { overlay }
        }
    }
}

extension MasterDetail where Overlay == EmptyView {
    init(centered: Bool = false, @ViewBuilder list: () -> List, @ViewBuilder detail: () -> Detail) {
        self.init(centered: centered, list: list, detail: detail, overlay: { EmptyView() })
    }
}
