import SwiftUI
import os

/// Root shell: TitleBar (top safe area) + Sidebar + Detail, with StatusBar
/// hung via `safeAreaInset(.bottom)` (the primuse lesson: a plain VStack
/// row would cut the material off behind floating chrome). Layout policy
/// sidebar initial 320pt, range 180–360pt; main window
/// minimum 1000×640pt. Secondary surfaces compress first via the sidebar
/// visibility toggle and the width drag handle.
public struct WorkspaceRootView<State: WorkspaceStateProviding>: View {
    let state: State
    /// App-side execution for palette rows (P4-04 Action 4): the shell stays
    /// dumb — it dismisses and forwards; the app decides what a command id
    /// does. Ids outside the registered set are ignored by the router.
    let onCommand: (PaletteCommand) -> Void
    /// P5-03 接线：ready 态正文（透传给 WorkspaceDetail；shell 不解析）。
    ///
    /// It receives the window's sidebar selection as a **binding**, not a
    /// value: the navigation spine is the shell's, so the app maps a row id
    /// onto its own surface and renders it — the shell never learns what a
    /// row means (INV-02 归属不变). A value would force the app to mirror
    /// the selection into its own state and keep the two in sync.
    let readyContent: ((Binding<String?>) -> AnyView)?
    let initialSelection: String?

    // Per-window state: WindowGroup copies the content view per window, so
    // each window owns its sidebar selection and palette presentation; the
    // focused window's bindings are exposed to AppCommands via
    // `focusedSceneValue` (below).
    //
    // The App injects the initial destination. Data arrival never chooses
    // a first project or replaces an explicit user target.
    @State private var selection: String?
    @State private var navigationEpoch: UInt64 = 0
    @State private var palettePresented = false

    @SceneStorage("workspace.preferred-sidebar-width") private var sidebarWidth: Double = 240
    @SceneStorage("workspace.inspector-visible") private var inspectorVisible = false
    /// Dragged auxiliary-panel width; 0 = first opening (0.45 of the window).
    @SceneStorage("workspace.inspector-width") private var inspectorWidth: Double = 0
    @SceneStorage("workspace.expanded-projects") private var expandedProjectJSON = ""
    @AppStorage(LegacySurfacePreview.storageKey) private var legacyPreviewStored = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Collapsed = the 56pt icon rail, never a hidden column.
    ///
    /// This replaced a `sidebarVisible` flag that was written once and never
    /// toggled: the sidebar was always shown, so its `.transition` and
    /// `.animation` were dead code and the window had no way to give the
    /// content more room. The reference clients all treat collapsing as a
    /// **rail** rather than a hide (`opencode` `w-16`; `deepseek-harness`
    /// `SIDEBAR_COLLAPSED = 56`; `ZCode` `WorkspaceSidebarCollapsedRail`),
    /// because hiding the column loses the navigation entirely.
    @State private var sidebarCollapsed = false

    public init(
        state: State,
        onCommand: @escaping (PaletteCommand) -> Void = { _ in },
        initialSelection: String? = nil,
        readyContent: ((Binding<String?>) -> AnyView)? = nil
    ) {
        self.state = state
        self.onCommand = onCommand
        self.initialSelection = initialSelection
        self.readyContent = readyContent
    }

    public var body: some View {
        GeometryReader { geometry in
            // Overhaul M1: flush columns — glass sidebar, a hairline drag
            // strip, then the opaque reading canvas (+ inspector inside it).
            let handle = NativePanelGeometry.mainShell.handleWidth
            let budget = NativePanelGeometry.mainShellBudget(
                totalWidth: max(0, geometry.size.width - handle), preferredSidebarWidth: sidebarWidth,
                inspectorVisible: inspectorVisible, sidebarCollapsed: sidebarCollapsed,
                preferredInspectorWidth: inspectorWidth > 0 ? inspectorWidth : nil
            )
            let railShown = sidebarCollapsed || budget.sidebarAutoCollapsed
            HStack(spacing: 0) {
                ProjectSidebar(
                    selection: navigationSelection, items: state.sidebarItems,
                    width: budget.sidebarWidth, collapsed: railShown,
                    expandedProjects: expandedProjects,
                    phase: state.phaseSnapshot, phaseText: state.phaseText,
                    onNewProjectTask: { navigationSelection.wrappedValue = SessionNavigation.draft(projectID: $0) },
                    onMoveSelection: moveSidebarSelection,
                    onOpenFolder: { onCommand(PaletteCommand(id: "project.open", title: "打开项目…")) },
                    onOpenSettings: { onCommand(PaletteCommand(id: "settings.show", title: "设置…")) },
                    onAddProject: { state.addProject(path: $0) }
                )
                Group {
                    if railShown {
                        Color.clear.overlay(alignment: .leading) { HairlineDivider(.vertical) }
                    } else {
                        SidebarResizeHandle(width: $sidebarWidth, effectiveWidth: budget.sidebarWidth)
                    }
                }
                .frame(width: handle)
                WorkspaceDetail(state: state, selection: navigationSelection, readyContent: readyContent)
                    .frame(width: max(0, budget.contentWidth))
            }
            .environment(\.mainShellBudget, budget)
            .environment(\.mainShellNavigationEpoch, navigationEpoch)
            .environment(\.mainShellInspectorWidth, $inspectorWidth)
        }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                NativeIconButton(systemImage: "sidebar.left", label: "切换侧栏",
                                 identifier: "workspace.toggle-sidebar") { sidebarCollapsed.toggle() }
            }
            ToolbarItemGroup(placement: .primaryAction) {
                moreToolsMenu
                Toggle(isOn: $inspectorVisible) { Image(systemName: "sidebar.right") }
                    .toggleStyle(.button)
                    .help("辅助面板（详情 / 文件 / 改动 / Git 日志）")
                    .accessibilityLabel("辅助面板")
                    .accessibilityIdentifier("workspace.toggle-inspector")
                NativeIconButton(systemImage: "magnifyingglass", label: "搜索与命令 ⌘K",
                                 identifier: "workspace.search") { palettePresented = true }
                NativeIconButton(systemImage: "square.and.pencil", label: "新建会话 ⌘N",
                                 identifier: "workspace.new-session") { openDraft() }
            }
        }
        .background(alignment: .topLeading) { selectionProbe }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            StatusBar(
                phase: state.phaseSnapshot,
                phaseText: state.phaseText,
                environmentLabel: state.environmentLabel,
                context: state.statusContext,
                counts: state.activityCounts,
                keepingAwake: state.keepingAwake
            )
        }
        .background(AeroricColor.canvas.ignoresSafeArea())
        .foregroundStyle(AeroricColor.text)
        .environment(\.legacySurfacePreview,
                     LegacySurfacePreview.isAvailable && (legacyPreviewStored || LegacySurfacePreview.launchEnabled))
        // Arrow handling stays on the sidebar List; editors retain their
        // own cursor keys.
        .animation(AeroricMotion.animation(AeroricMotion.slow, reduceMotion: reduceMotion), value: sidebarCollapsed)
        .font(Font.system(size: AeroricType.body))
        .frame(minWidth: NativePanelGeometry.mainShell.minimumWindowWidth,
               minHeight: NativePanelGeometry.mainShell.minimumWindowHeight)
        .focusedValue(\.palettePresentation, $palettePresented)
        .focusedValue(\.workspaceSelection, navigationSelection)
        .focusedSceneValue(\.workspaceSelection, navigationSelection)
        .focusedValue(\.workspaceActivity, true)
        .focusedValue(\.sidebarCollapse, $sidebarCollapsed)
        // Fill only the initial destination; never override a user target.
        .onAppear {
            if selection == nil {
                selection = initialSelection
            }
            if expandedProjectJSON.isEmpty {
                let current = state.sidebarItems.first {
                    $0.group == .projects && ($0.id == selection || $0.children?.contains { $0.id == selection } == true)
                }
                expandedProjects.wrappedValue = Set(current.map { [$0.id] } ?? [])
            }
        }
        .overlay {
            if palettePresented {
                CommandPalette(
                    isPresented: $palettePresented,
                    commands: state.paletteCommands,
                    onSelect: handleCommand
                )
                .zIndex(1)
            }
        }
    }

    private func handleCommand(_ command: PaletteCommand) {
        if command.id == "task.new" { openDraft() }
        // Section jumps are window-local navigation, like the sidebar rows.
        else if command.id.hasPrefix("section.") { navigationSelection.wrappedValue = command.id }
        else { onCommand(command) }
    }

    /// The current item's title, read by the launch tests through
    /// `detail.selection`. The visible crumb was removed (2026-10-26 UI
    /// review); this zero-size element keeps the accessibility contract.
    private var selectionProbe: some View {
        let title = SidebarOrdering.item(id: selection, in: state.sidebarItems)?.title ?? "Aeroric"
        return Color.clear
            .frame(width: 1, height: 1)
            .accessibilityElement()
            .accessibilityLabel(title)
            .accessibilityValue(title)
            .accessibilityIdentifier("detail.selection")
    }

    /// The 11 tool and system surfaces (Git, 数据库, IDE …): one toolbar menu
    /// and ⌘K, never permanent sidebar rows.
    private var moreToolsMenu: some View {
        let tools = state.sidebarItems.filter { $0.group == .tools || $0.group == .system }
        return Menu {
            ForEach([SidebarGroup.tools, .system], id: \.self) { group in
                let rows = tools.filter { $0.group == group }
                if !rows.isEmpty {
                    Section(group == .tools ? "工具" : "系统") {
                        ForEach(rows) { item in
                            Button {
                                navigationSelection.wrappedValue = item.id
                            } label: {
                                Label(item.title, systemImage: item.systemImage ?? "wrench.and.screwdriver")
                            }
                        }
                    }
                }
            }
        } label: {
            Image(systemName: "square.grid.2x2")
        }
        .menuIndicator(.hidden)
        .help("工具与服务（也可用 ⌘K）")
        .accessibilityLabel("工具与服务")
        .accessibilityIdentifier("workspace.more-tools")
    }

    private var navigationSelection: Binding<String?> {
        Binding(get: { selection }, set: {
            selection = $0
            navigationEpoch &+= 1 // Reselecting the current task also leaves preview.
        })
    }

    private var expandedProjects: Binding<Set<String>> {
        Binding(
            get: {
                guard let data = expandedProjectJSON.data(using: .utf8),
                      let ids = try? JSONDecoder().decode([String].self, from: data) else { return [] }
                return Set(ids)
            },
            set: { ids in
                if let data = try? JSONEncoder().encode(ids.sorted()),
                   let json = String(data: data, encoding: .utf8) { expandedProjectJSON = json }
            }
        )
    }

    private func openDraft() {
        navigationSelection.wrappedValue = SessionNavigation.newDraftSelection(from: selection, items: state.sidebarItems)
    }

    private func moveSidebarSelection(_ direction: MoveCommandDirection) {
        // `SidebarOrdering.rendered`, not `state.sidebarItems`: the app
        // supplies rows grouped arbitrarily (workspace surfaces then
        // projects) while the sidebar draws them bucketed. Walking the
        // supplied order moved the highlight to a row that is not visually
        // adjacent — measured 2026-10-13, ↓ from 任务 landed on Git because
        // Git is third in the supplied array but three groups down on screen.
        guard !palettePresented else { return }
        let navigationItems = state.sidebarItems.filter { $0.group == .work || $0.group == .projects }
        let items = sidebarCollapsed
            ? SidebarOrdering.rendered(navigationItems)
            : SidebarOrdering.rendered(navigationItems, expanded: expandedProjects.wrappedValue)
        guard !items.isEmpty else { return }
        let currentIndex = selection.flatMap { id in items.firstIndex(where: { $0.id == id }) }
        switch direction {
        case .down:
            let next = currentIndex.map { min($0 + 1, items.count - 1) } ?? 0
            navigationSelection.wrappedValue = items[next].id
        case .up:
            let previous = currentIndex.map { max($0 - 1, 0) } ?? 0
            navigationSelection.wrappedValue = items[previous].id
        default:
            break
        }
    }
}

/// Drag handle for the sidebar width. Clamps to the P4-04 range 180–360pt.
struct SidebarResizeHandle: View {
    @Binding var width: Double
    let effectiveWidth: CGFloat
    @State private var dragStartWidth: CGFloat?

    var body: some View {
        // The column hairline sits on the sidebar edge; the strip is the hit area.
        Color.clear
            .overlay(alignment: .leading) { HairlineDivider(.vertical) }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onHover { inside in
                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        if dragStartWidth == nil { dragStartWidth = effectiveWidth }
                        width = min(NativePanelGeometry.mainShell.maximumSidebarWidth,
                                    max(NativePanelGeometry.mainShell.minimumSidebarWidth,
                                        (dragStartWidth ?? effectiveWidth) + value.translation.width))
                    }
                    .onEnded { _ in dragStartWidth = nil }
            )
            .accessibilityIdentifier("sidebar.resize.handle")
            .accessibilityLabel("调整侧栏宽度")
    }
}

// MARK: - Focused values (P4-04 Action 4)

/// The focused workspace window presents the command palette through this
/// binding; the menu command (Cmd+K) flips it. Multiple windows each
/// register their own binding — only the focused window's is seen here.
public struct PalettePresentationKey: FocusedValueKey {
    public typealias Value = Binding<Bool>
}

/// The focused window's sidebar selection: commands that target "the project
/// the user is looking at" bind through this (P4-04 Action 4's "focused
/// workspace decides the command target"). `focusedSceneValue` ensures the
/// value follows window focus, not just view focus.
public struct WorkspaceSelectionKey: FocusedValueKey {
    public typealias Value = Binding<String?>
}

/// Whether the focused scene is a workspace (settings/about windows register
/// nothing, so commands that need a workspace disable themselves).
public struct WorkspaceActivityKey: FocusedValueKey {
    public typealias Value = Bool
}

/// The focused workspace window's sidebar collapse state. The View-menu
/// command flips this binding; multiple windows each register their own, so
/// the toggle lands on the window the user is looking at.
public struct SidebarCollapseKey: FocusedValueKey {
    public typealias Value = Binding<Bool>
}

public struct NewSessionActionKey: FocusedValueKey {
    public typealias Value = () -> Void
}

public extension FocusedValues {
    var newSessionAction: (() -> Void)? {
        get { self[NewSessionActionKey.self] }
        set { self[NewSessionActionKey.self] = newValue }
    }
    var palettePresentation: Binding<Bool>? {
        get { self[PalettePresentationKey.self] }
        set { self[PalettePresentationKey.self] = newValue }
    }

    var sidebarCollapse: Binding<Bool>? {
        get { self[SidebarCollapseKey.self] }
        set { self[SidebarCollapseKey.self] = newValue }
    }

    var workspaceSelection: Binding<String?>? {
        get { self[WorkspaceSelectionKey.self] }
        set { self[WorkspaceSelectionKey.self] = newValue }
    }

    var workspaceActivity: Bool? {
        get { self[WorkspaceActivityKey.self] }
        set { self[WorkspaceActivityKey.self] = newValue }
    }
}
