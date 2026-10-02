import SwiftUI

/// Phase snapshot the shell consumes. AeroricUI has no dependency on the
/// client package (INV-02: SwiftUI never parses wire formats); the app maps
/// its `SupervisorPhase` onto this enum at the injection boundary.
public enum ConnectionPhaseSnapshot: Equatable, Sendable {
    case idle
    case connecting
    case ready
    case degraded(reason: String?)
    case reconnecting
    case incompatible(reason: String?)
    case stopped
}

/// One sidebar row. Real project data arrives with P5-03; P4-04 wires the
/// surface and its system `List(selection:)` behaviour.
public struct SidebarItem: Identifiable, Hashable, Sendable {
    public var id: String
    public var title: String

    public init(id: String, title: String) {
        self.id = id
        self.title = title
    }
}

/// Command-palette row. P4-04 ships the surface and its keyboard handling;
/// the command list is supplied by the app side.
public struct PaletteCommand: Identifiable, Hashable, Sendable {
    public var id: String
    public var title: String

    public init(id: String, title: String) {
        self.id = id
        self.title = title
    }
}

/// Shell-side read model. The app object (`AppServices`) conforms; the shell
/// never reaches into client types itself.
@MainActor
public protocol WorkspaceStateProviding: AnyObject {
    /// Current supervisor phase, mapped onto the shell snapshot enum.
    var phaseSnapshot: ConnectionPhaseSnapshot { get }
    /// Human-readable phase line (already formatted by the store).
    var phaseText: String { get }
    var environmentLabel: String { get }
    var protocolVersionText: String { get }
    var buildIdentifier: String { get }
    var applicationTag: String { get }
    var sidebarItems: [SidebarItem] { get }
    /// Commands offered by the palette; ordering is the app's choice.
    var paletteCommands: [PaletteCommand] { get }
}

/// The four deliberately separate shell states (P4-04 Action 5): loading is
/// never rendered as "no projects", disconnect is never rendered as empty,
/// protocol incompatibility is a distinct error, not a generic failure.
public enum DetailState: Equatable, Sendable {
    case loading
    case empty
    case disconnected(reason: String?)
    case incompatible(reason: String?)

    /// Pure resolver — unit-tested in AeroricUIUnitTests (ShellPolicyTests).
    public static func resolve(_ phase: ConnectionPhaseSnapshot) -> DetailState {
        switch phase {
        case .idle, .connecting:
            .loading
        case .ready:
            .empty
        case let .degraded(reason):
            .disconnected(reason: reason)
        case .reconnecting, .stopped:
            .disconnected(reason: nil)
        case let .incompatible(reason):
            .incompatible(reason: reason)
        }
    }
}

/// Detail column: the four states are distinct surfaces, plus a fixed
/// bootstrap info block whose accessibility identifiers are asserted by
/// `AppLaunchTests` — the block stays mounted in every state so the cold
/// launch can never race a state transition.
public struct WorkspaceDetail<State: WorkspaceStateProviding>: View {
    let state: State
    /// The window's selected sidebar item, if any. Rendered as a visible
    /// header (below) because macOS List selection state is not AX-queryable
    /// through XCUITest — a value-backed element is.
    let selection: String?
    /// P5-03 接线：ready 态的正文内容（App 侧提供任务工作区视图）。nil
    /// ＝保持 shell 占位（EmptyStateView），注入后 shell 不解析内容。
    let readyContent: AnyView?

    public init(state: State, selection: String? = nil, readyContent: AnyView? = nil) {
        self.state = state
        self.selection = selection
        self.readyContent = readyContent
    }

    private var selectedTitle: String? {
        selection.flatMap { id in
            state.sidebarItems.first(where: { $0.id == id })?.title
        }
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let selectedTitle {
                Text("已选项目：\(selectedTitle)")
                    .font(.callout)
                    .foregroundStyle(Color.dsTextSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .accessibilityIdentifier("detail.selection")
            }
            switch DetailState.resolve(state.phaseSnapshot) {
            case .loading:
                LoadingStateView()
            case .empty:
                if let readyContent {
                    readyContent
                } else {
                    EmptyStateView()
                }
            case let .disconnected(reason):
                DisconnectedStateView(reason: reason)
            case let .incompatible(reason):
                IncompatibleStateView(reason: reason)
            }
            Spacer(minLength: 0)
            BootstrapInfoBlock(state: state)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .frame(minWidth: 600, minHeight: 400)
        // No container accessibilityIdentifier here: an identifier on this
        // container propagates down and REPLACES the leaf identifiers
        // (bootstrap.*) in the AX tree — AppLaunchTests lost
        // bootstrap.environment to exactly that (measured 2026-10-01, AX
        // dump). Identifiers live on the leaves only.
    }
}

private struct LoadingStateView: View {
    var body: some View {
        VStack(spacing: 12) {
            ProgressView()
            Text("连接宿主中…")
                .foregroundStyle(Color.dsTextSecondary)
                .accessibilityIdentifier("detail.loading")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct EmptyStateView: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "tray")
                .font(.system(size: 32))
                .foregroundStyle(Color.dsTextMuted)
            Text("暂无项目")
                .foregroundStyle(Color.dsTextSecondary)
                .accessibilityIdentifier("detail.empty")
            Text("通过菜单 文件 → 打开项目… 选择一个目录")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct DisconnectedStateView: View {
    let reason: String?

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "bolt.slash")
                .font(.system(size: 32))
                .foregroundStyle(Color.dsWarning)
            Text("已断开")
                .foregroundStyle(Color.dsText)
                .accessibilityIdentifier("detail.disconnected")
            if let reason {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
                    .multilineTextAlignment(.center)
            }
            Text("宿主将自动重连；本窗口数据只读等待恢复")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct IncompatibleStateView: View {
    let reason: String?

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 32))
                .foregroundStyle(Color.dsDanger)
            Text("协议不兼容")
                .foregroundStyle(Color.dsText)
                .accessibilityIdentifier("detail.incompatible")
            if let reason {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
                    .multilineTextAlignment(.center)
            }
            Text("请更新 Aeroric 以匹配宿主协议版本")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Fixed info block. The five identifiers below are the AppLaunchTests
/// surface (bootstrap.*); text mirrors the P4-02 bootstrap semantics
/// (environment / protocol / build / connection / application).
struct BootstrapInfoBlock<State: WorkspaceStateProviding>: View {
    let state: State

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            LabeledContent("environment") {
                Text(state.environmentLabel)
                    .accessibilityIdentifier("bootstrap.environment")
            }
            LabeledContent("protocol") {
                Text(state.protocolVersionText)
                    .accessibilityIdentifier("bootstrap.protocol")
            }
            LabeledContent("build") {
                Text(state.buildIdentifier)
                    .accessibilityIdentifier("bootstrap.build")
            }
            LabeledContent("connection") {
                Text(state.phaseText)
                    .accessibilityIdentifier("bootstrap.connection")
            }
            LabeledContent("application") {
                Text(state.applicationTag)
                    .accessibilityIdentifier("bootstrap.application")
            }
        }
        .font(.caption)
        .foregroundStyle(Color.dsTextSecondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color.dsSurface.opacity(0.55))
        // No `bootstrap.info` container identifier: it would replace the
        // bootstrap.* leaf identifiers in the AX tree (same rule as
        // WorkspaceDetail above).
    }
}
