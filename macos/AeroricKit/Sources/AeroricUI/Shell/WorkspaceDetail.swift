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

/// Which section of the navigation spine a row belongs to.
///
/// The converged convention across the reference clients (opencode,
/// deepseek-harness, ZCode, orca, LiveAgent, codeg) is that a sidebar
/// **groups** its rows under labelled headers and never renders one flat
/// list — `codeg`'s own sidebar is `folders / chats / recent` plus a
/// `pinned` bucket (`src/lib/sidebar-view-mode-storage.ts:30,39`), and the
/// reference `opencode` TUI groups by `Pinned / Today / <date>`
/// (`dialog-session-list.tsx:232-260`). The group is therefore part of the
/// row model, not something the view invents per call site.
public enum SidebarGroup: String, CaseIterable, Identifiable, Hashable, Sendable {
    /// The daily job: running and watching agent tasks.
    case work
    /// Projects, each scoping the task list.
    case projects
    /// Imported sessions whose cwd matches no open project (M4).
    case imports
    /// Tools scoped to the open project.
    case tools
    /// Machine- and agent-level configuration surfaces.
    case system

    public var id: String { rawValue }

    var titleKey: LocalizedStringKey {
        switch self {
        case .work: "工作"
        case .projects: "项目"
        case .imports: "未归类导入"
        case .tools: "工具"
        case .system: "系统"
        }
    }

    /// Render order of the groups. Fixed, so a row can never float to a
    /// surprising place by arriving in a different order.
    static let renderOrder: [SidebarGroup] = [.work, .projects, .imports, .tools, .system]
}

/// The one place that decides the order rows appear in.
///
/// Both the sidebar's rendering and the keyboard's ↑/↓ movement go through
/// this, because they must agree: the shell's arrow-key helper moves a
/// selection index through the row list, and if that list is in a different
/// order than the one on screen, pressing ↓ jumps the highlight to a row that
/// is not visually below the current one.
///
/// Measured 2026-10-13: `AppServices.sidebarItems` supplies the twelve
/// workspace rows first and the projects after, while the sidebar renders
/// them grouped (`工作 → 项目 → 工具 → 系统`). Moving by the supplied order
/// made ↓ from 任务 land on Git — a row three groups further down the screen.
public enum SidebarOrdering {
    /// Rows bucketed by group in `renderOrder`. Empty groups are dropped
    /// rather than rendered as an empty header.
    public static func grouped(
        _ items: [SidebarItem]
    ) -> [(group: SidebarGroup, items: [SidebarItem])] {
        SidebarGroup.renderOrder.compactMap { group in
            let rows = items.filter { $0.group == group }
            return rows.isEmpty ? nil : (group, rows)
        }
    }

    /// The rows in the order they are actually rendered — the order keyboard
    /// navigation must follow. Order within a group is the caller's.
    public static func rendered(_ items: [SidebarItem]) -> [SidebarItem] {
        grouped(items).flatMap(\.items)
    }

    public static func rendered(_ items: [SidebarItem], expanded: Set<String>) -> [SidebarItem] {
        rendered(items).flatMap { item in
            [item] + (expanded.contains(item.id) ? item.children ?? [] : [])
        }
    }

    public static func item(id: String?, in items: [SidebarItem]) -> SidebarItem? {
        guard let id else { return nil }
        for item in items {
            if item.id == id { return item }
            if let child = Self.item(id: id, in: item.children ?? []) { return child }
        }
        return nil
    }
}

/// One sidebar row. Real project data arrives with P5-03; P4-04 wires the
/// surface and its system `List(selection:)` behaviour.
///
/// Row anatomy follows the reference clients' shared grammar — a leading
/// status/type glyph, a truncating title, and a trailing count badge
/// (`opencode` `sidebar-items.tsx:110-160`, `deepseek` `Rows.tsx:339-473`,
/// `codeg`'s folder rows carrying `ml-auto` count pills).
public struct SidebarItem: Identifiable, Hashable, Sendable {
    public var id: String
    public var title: String
    /// SF Symbol for the leading glyph. `nil` renders a title-only row
    /// (project rows are title-only, matching the reference's folder rows).
    public var systemImage: String?
    /// Trailing count. `nil` renders no badge rather than a zero.
    public var badge: Int?
    /// Which labelled section the row sits under.
    public var group: SidebarGroup
    /// Project -> task hierarchy. Nil is a leaf; [] is an empty project.
    public var children: [SidebarItem]?
    /// Host task status for session rows (`running`, `input_required`, …).
    /// Mirrors the wire vocabulary; nil for non-task rows.
    public var status: String?
    /// Host-authoritative star, shown as "置顶" in the sidebar.
    public var pinned: Bool
    /// Agent that runs the session (`claude`, `codex`, …) for the row glyph.
    public var agent: String?
    /// Last activity, for the row's relative time.
    public var updatedAt: Date?
    /// Read-only imported session row (M4); shows "已导入".
    public var imported: Bool
    /// Working directory of an imported session, for "添加为项目…".
    public var path: String?

    public init(
        id: String,
        title: String,
        systemImage: String? = nil,
        badge: Int? = nil,
        group: SidebarGroup = .projects,
        children: [SidebarItem]? = nil,
        status: String? = nil,
        pinned: Bool = false,
        agent: String? = nil,
        updatedAt: Date? = nil,
        imported: Bool = false,
        path: String? = nil
    ) {
        self.imported = imported
        self.path = path
        self.id = id
        self.title = title
        self.systemImage = systemImage
        self.badge = badge
        self.group = group
        self.children = children
        self.status = status
        self.pinned = pinned
        self.agent = agent
        self.updatedAt = updatedAt
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
    /// What the window is currently scoped to, for the status bar's trailing
    /// slot (project name, live task count, …). The app formats it; the shell
    /// does not interpret it. `nil` leaves the slot empty rather than
    /// inventing a placeholder.
    var statusContext: String? { get }
    /// Commands offered by the palette; ordering is the app's choice.
    var paletteCommands: [PaletteCommand] { get }
    /// Live counts for the status bar; nil until the task mirror is loaded.
    var activityCounts: ActivityCounts? { get }
    /// Open `path` as a project (from "添加为项目…" on an unassigned import).
    func addProject(path: String)
}

public extension WorkspaceStateProviding {
    var activityCounts: ActivityCounts? { nil }
    func addProject(path: String) {}
}

/// Running / waiting-for-you counts shown in the status bar.
public struct ActivityCounts: Equatable, Sendable {
    public var running: Int
    public var waiting: Int
    public init(running: Int, waiting: Int) {
        self.running = running
        self.waiting = waiting
    }
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
    /// The window's selected sidebar item. Held as a **binding** because the
    /// ready-state content is built by the app from this selection — the
    /// shell owns the navigation spine, the app owns what a row means.
    ///
    /// It is also rendered as a visible title (below) because macOS List
    /// selection state is not AX-queryable through XCUITest — a
    /// value-backed element is.
    @Binding var selection: String?
    /// P5-03 接线：ready 态的正文内容（App 侧提供任务工作区视图）。nil
    /// ＝保持 shell 占位（EmptyStateView），注入后 shell 不解析内容。
    let readyContent: ((Binding<String?>) -> AnyView)?

    public init(
        state: State,
        selection: Binding<String?> = .constant(nil),
        readyContent: ((Binding<String?>) -> AnyView)? = nil
    ) {
        self.state = state
        self._selection = selection
        self.readyContent = readyContent
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let readyContent {
                // Keep the per-window draft owner mounted through a
                // disconnect. Read-only overlay never resets its identity.
                readyContent($selection)
                    .disabled(state.phaseSnapshot != .ready)
                    .overlay {
                        if state.phaseSnapshot != .ready {
                            connectionPanel
                        }
                    }
            } else {
                connectionPanel
            }
            if BootstrapInfoBlock<State>.shouldDisplay(state: state) {
                BootstrapInfoBlock(state: state)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // This is the centre + optional inspector envelope, not the
        // 534pt centre alone at the approved 1000pt window minimum.
        .frame(minWidth: 600, minHeight: 400)
    }

    /// Overhaul M1: connection states sit on the opaque reading canvas.
    private var connectionPanel: some View {
        connectionContent
            .background(AeroricColor.canvas)
    }

    @ViewBuilder private var connectionContent: some View {
        switch DetailState.resolve(state.phaseSnapshot) {
        case .loading:
            LoadingStateView()
        case .empty:
            EmptyStateView()
        case let .disconnected(reason):
            DisconnectedStateView(reason: reason)
        case let .incompatible(reason):
            IncompatibleStateView(reason: reason)
        }
    }
}

private struct LoadingStateView: View {
    var body: some View {
        VStack(spacing: AeroricSpace.l) {
            ProgressView().controlSize(.small)
            Text("连接宿主中…")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("detail.loading")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct EmptyStateView: View {
    var body: some View {
        AeroricEmptyState(systemImage: "folder.badge.plus", title: "暂无项目",
                          message: "通过菜单 文件 → 打开项目… 选择一个目录，就可以开始新会话。",
                          titleIdentifier: "detail.empty")
    }
}

private struct DisconnectedStateView: View {
    let reason: String?

    var body: some View {
        AeroricEmptyState(systemImage: "bolt.slash", title: "已断开",
                          message: [reason, "宿主将自动重连；本窗口数据只读等待恢复"]
                              .compactMap { $0 }.joined(separator: "\n"),
                          tint: AeroricColor.warning, titleIdentifier: "detail.disconnected")
    }
}

private struct IncompatibleStateView: View {
    let reason: String?

    var body: some View {
        AeroricEmptyState(systemImage: "exclamationmark.triangle", title: "协议不兼容",
                          message: [reason, "请更新 Aeroric 以匹配宿主协议版本"]
                              .compactMap { $0 }.joined(separator: "\n"),
                          tint: AeroricColor.danger, titleIdentifier: "detail.incompatible")
    }
}

/// Whether a window is running in the environment that renders developer
/// scaffolding — the single place that decides, so the rule cannot drift per
/// call site.
///
/// Two surfaces show developer facts (`BootstrapInfoBlock`, `StatusBar`),
/// and both must agree on when that is honest.
///
/// ## Why only `ui-test` (2026-10-13, second revision)
///
/// The first revision also rendered in `dev`, on the theory that a developer
/// wants to see the build they are looking at. Measured in the real window:
/// that is the **user's own build** (`native:dev` is how the app is run
/// interactively), so the five-line block and the `dev` tag were permanent
/// furniture in exactly the surface being redesigned — the "杂乱" report.
///
/// The block exists to carry the `bootstrap.*` identifiers that
/// `AppLaunchTests` / `ShellLifecycleTests` assert, and **every UI test runs
/// with `AERORIC_ENV=ui-test`** (`scripts/native/ui-test.mjs:63`, forwarded
/// by each test's `launchEnvironment`). So the tests keep their surface and
/// every other launch looks like the product. A developer who wants the
/// facts has the host log and the status bar's connection phase.
public enum DiagnosticEnvironment {
    public static func isDiagnostic(_ label: String) -> Bool {
        label.trimmingCharacters(in: .whitespaces).lowercased() == "ui-test"
    }
}

/// Fixed info block. The five identifiers below are the AppLaunchTests
/// surface (bootstrap.*); text mirrors the P4-02 bootstrap semantics
/// (environment / protocol / build / connection / application).
///
/// ## It is **not** product UI (AP8, 2026-10-13)
///
/// This block renders `environment / protocol / build / connection /
/// application` — five lines whose names are literally the AX identifiers the
/// cold-launch test asserts. It has no user-facing value: nobody opens a task
/// runner to read `build 0.1.0-debug+1`. It was mounted **unconditionally in
/// every state** because a test needs it present at first paint, and that is
/// exactly how test scaffolding becomes the visible product surface — measured
/// 2026-10-13 in the real window: five developer lines sitting permanently under
/// the content area.
///
/// So it is rendered only where developer facts are honest to show:
/// the `ui-test` environment (injected by `native:ui-test`; every UI test
/// runs in it) or a connection that is **not** ready — in the latter case
/// `connection` is the one line a user actually wants, and the rest say
/// which build they are looking at. Any other `ready` window omits it.
///
/// The `dev` environment used to be included as well; that was wrong, since
/// `native:dev` is how the app is run **interactively** — so the block was
/// permanent furniture in the very surface being redesigned. See
/// `DiagnosticEnvironment`.
///
/// The `bootstrap.*` identifiers stay assertable because every state that a
/// cold-launch test can observe is either non-ready or `ui-test`;
/// [BootstrapInfoBlock.isDiagnosticEnvironment] is the single place that
/// decides, so the rule cannot drift per call site.
struct BootstrapInfoBlock<State: WorkspaceStateProviding>: View {
    let state: State

    /// Which environments render the block. Only `ui-test` — the
    /// environment `native:ui-test` injects and the one every UI test runs
    /// in. See `DiagnosticEnvironment` for why `dev` is not included.
    static func isDiagnosticEnvironment(_ label: String) -> Bool {
        DiagnosticEnvironment.isDiagnostic(label)
    }

    /// Whether this state should render the block at all.
    ///
    /// One place decides, so no call site can drift into showing developer
    /// facts in a ready production window.
    static func shouldDisplay(state: some WorkspaceStateProviding) -> Bool {
        if isDiagnosticEnvironment(state.environmentLabel) {
            return true
        }
        // A non-ready connection is the one case where `connection` is the line
        // the user wants, and the others say which build they are looking at.
        return DetailState.resolve(state.phaseSnapshot) != .empty
    }

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
        .foregroundStyle(AeroricColor.textSecondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(AeroricColor.canvas.opacity(0.55))
        // No identifier on this container — not even on the outermost
        // modifier. Measured 2026-10-13 from the AX dump of a failing
        // `AppLaunchTests` run: a `.accessibilityIdentifier` anywhere on this
        // subtree propagates down and **replaces** the `bootstrap.*` leaf
        // identifiers, so every value read `detail.bootstrap-info` and
        // `bootstrap.environment` matched nothing. The same rule is written
        // above `WorkspaceDetail`; an earlier revision of this block added
        // the container id with a comment asserting the leaves survived, and
        // that assertion was never checked against a real AX tree. The
        // identifiers live on the leaves only.
    }
}
