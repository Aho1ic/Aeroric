import SwiftUI

/// Redesign 01 navigation spine (codeg "New chat" row + Agents-Anywhere
/// project → session tree + DSH sizing). v3 draws codeg capsule rows; the
/// `selection` binding owns the selected row; the column sits on the system sidebar material. Tool
/// surfaces (Git, 数据库, IDE …) live in the bottom tools menu and ⌘K, never
/// as permanent rows.
///
/// Width policy (DSH `columns.ts`): 280pt default, 264–420pt drag range,
/// 56pt icon rail when collapsed or when the window is narrower than 1024pt.
public struct ProjectSidebar: View {
    @Binding var selection: String?
    let items: [SidebarItem]
    let width: CGFloat
    /// Collapsed = the 56pt icon rail, never a hidden column.
    let collapsed: Bool
    @Binding private var expandedProjects: Set<String>
    private let onNewProjectTask: (String) -> Void
    private let onMoveSelection: (MoveCommandDirection) -> Void
    /// Window-local status filter. Presentation only; never a host query.
    @State private var filter: SidebarStatusFilter = .all

    public static let collapsedWidth: CGFloat = 56

    public init(
        selection: Binding<String?>,
        items: [SidebarItem],
        width: CGFloat,
        collapsed: Bool = false,
        expandedProjects: Binding<Set<String>> = .constant([]),
        onNewProjectTask: @escaping (String) -> Void = { _ in },
        onMoveSelection: @escaping (MoveCommandDirection) -> Void = { _ in }
    ) {
        self._selection = selection
        self.items = items
        self.width = width
        self.collapsed = collapsed
        _expandedProjects = expandedProjects
        self.onNewProjectTask = onNewProjectTask
        self.onMoveSelection = onMoveSelection
    }

    private var workItems: [SidebarItem] {
        items.filter { $0.group == .work && $0.id != SessionNavigation.globalDraft }
    }
    private var projectItems: [SidebarItem] { items.filter { $0.group == .projects } }
    private var toolItems: [SidebarItem] { items.filter { $0.group == .tools || $0.group == .system } }

    public var body: some View {
        VStack(spacing: 0) {
            if collapsed {
                railContent
            } else {
                expandedContent
            }
        }
        .frame(width: collapsed ? Self.collapsedWidth : width)
        .frame(maxHeight: .infinity)
        .background(SidebarBackdrop().ignoresSafeArea())
    }

    // MARK: Expanded

    private var expandedContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            newSessionButton
                .padding(.horizontal, 8)
                .padding(.top, 6)
                .padding(.bottom, 4)
            if items.isEmpty {
                emptyContent
            } else {
                // v3: custom capsule rows (codeg `h-8 rounded-full`) instead of
                // the system List highlight. Selection stays in the binding;
                // ↑/↓ still route through `onMoveSelection`.
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 1) {
                        ForEach(workItems) { item in
                            SidebarCapsuleRow(selected: selection == item.id,
                                              height: NativePanelGeometry.mainShell.projectRowHeight,
                                              action: { selection = item.id }) {
                                SidebarRow(item: item)
                            }
                            .accessibilityIdentifier("sidebar.item.\(item.id)")
                        }
                        projectsHeader
                            .padding(.top, 10)
                            .padding(.bottom, 2)
                        if projectItems.isEmpty {
                            Text("尚未打开项目；文件 → 打开项目…")
                                .font(AeroricFont.ui(AeroricFont.meta))
                                .foregroundStyle(SessionPalette.muted)
                                .padding(.horizontal, 10)
                        }
                        ForEach(projectItems) { project in
                            projectGroup(project)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.bottom, 8)
                }
                .scrollIndicators(.never)
                .focusable()
                .focusEffectDisabled()
                .onMoveCommand(perform: onMoveSelection)
                .accessibilityIdentifier("sidebar.list")
            }
            if !toolItems.isEmpty {
                Divider().opacity(0.5)
                toolsMenu
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
            }
        }
    }

    private var newSessionButton: some View {
        SidebarActionRow(systemImage: "square.and.pencil", title: "新建会话", shortcut: "⌘N",
                         identifier: "sidebar.new-session") {
            selection = SessionNavigation.globalDraft
        }
    }

    private func sessions(of project: SidebarItem) -> [SidebarItem] {
        (project.children ?? []).filter {
            SessionNavigation.taskID($0.id) != nil && filter.matches($0.status)
        }
    }

    private var projectsHeader: some View {
        HStack(spacing: 4) {
            Text("项目")
            Spacer(minLength: 0)
            filterMenu
        }
        .font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
        .foregroundStyle(SessionPalette.muted)
        .padding(.horizontal, 10)
        .accessibilityIdentifier("sidebar.group.projects")
    }

    @ViewBuilder private func projectGroup(_ project: SidebarItem) -> some View {
        let expanded = expandedProjects.contains(project.id)
        let rows = sessions(of: project)
        ProjectRow(item: project, sessionCount: rows.count, expanded: expanded,
                   selected: selection == project.id,
                   onSelect: {
                       selection = project.id
                       expandedProjects.insert(project.id)
                   },
                   onToggle: {
                       if expanded { expandedProjects.remove(project.id) } else { expandedProjects.insert(project.id) }
                   },
                   onNewTask: { onNewProjectTask(project.id) })
            .accessibilityIdentifier("sidebar.item.\(project.id)")
        if expanded {
            ForEach(rows) { child in
                SidebarCapsuleRow(selected: selection == child.id, height: 26,
                                  action: { selection = child.id }) {
                    SessionRow(item: child)
                }
                .padding(.leading, 18)
                .accessibilityIdentifier("sidebar.item.\(child.id)")
            }
            if rows.isEmpty {
                Text(filter == .all ? "还没有会话" : "没有符合筛选的会话")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.muted)
                    .padding(.leading, 38)
                    .frame(height: 24)
            }
        }
    }

    private var filterMenu: some View {
        Menu {
            Picker("筛选会话", selection: $filter) {
                ForEach(SidebarStatusFilter.allCases, id: \.self) { option in
                    Text(option.title).tag(option)
                }
            }
            .pickerStyle(.inline)
        } label: {
            Image(systemName: filter == .all
                  ? "line.3.horizontal.decrease" : "line.3.horizontal.decrease.circle.fill")
                .foregroundStyle(filter == .all ? SessionPalette.muted : SessionPalette.accent)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("按状态筛选会话")
        .accessibilityLabel("按状态筛选会话：\(filter.title)")
        .accessibilityIdentifier("sidebar.filter")
    }

    private var toolsMenu: some View {
        Menu {
            Picker("工具与服务", selection: $selection) {
                ForEach(toolItems) { item in
                    Label(item.title, systemImage: item.systemImage ?? "wrench.and.screwdriver")
                        .tag(Optional(item.id))
                }
            }
            .pickerStyle(.inline)
        } label: {
            if collapsed {
                Image(systemName: "square.grid.2x2")
                    .frame(maxWidth: .infinity, minHeight: 28)
            } else {
                Label("工具", systemImage: "square.grid.2x2")
                    .font(AeroricFont.ui(AeroricFont.project))
                    .foregroundStyle(SessionPalette.secondary)
                    .frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
            }
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(collapsed ? .hidden : .visible)
        .help("工具与服务（也可用 ⌘K）")
        .accessibilityLabel("工具与服务")
        .accessibilityIdentifier("sidebar.tools-menu")
    }

    // MARK: Rail

    private var railContent: some View {
        VStack(spacing: 0) {
            NativeIconButton(systemImage: "square.and.pencil", label: "新建会话",
                             identifier: "sidebar.new-session") {
                selection = SessionNavigation.globalDraft
            }
            .padding(.top, 6)
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(workItems + projectItems) { item in
                        SidebarCapsuleRow(selected: selection == item.id, height: 30, horizontalPadding: 0,
                                          action: { selection = item.id }) {
                            RailRow(item: item)
                        }
                        .accessibilityIdentifier("sidebar.item.\(item.id)")
                    }
                }
                .padding(.horizontal, 8)
                .padding(.top, 6)
            }
            .scrollIndicators(.never)
            .focusable()
            .focusEffectDisabled()
            .onMoveCommand(perform: onMoveSelection)
            .accessibilityIdentifier("sidebar.rail")
            if !toolItems.isEmpty {
                toolsMenu.padding(.horizontal, 4).padding(.vertical, 6)
            }
        }
    }

    private var emptyContent: some View {
        PaneEmptyState(
            systemImage: "square.stack.3d.up",
            title: "暂无项目",
            hint: "通过菜单 文件 → 打开项目… 选择一个目录"
        )
        .accessibilityIdentifier("sidebar.empty")
    }
}

/// Window-local status filter over the host's wire vocabulary
/// (`pending`/`running`/`detached`/`input_required`/`done`/`failed`/`cancelled`).
public enum SidebarStatusFilter: String, CaseIterable, Hashable, Sendable {
    case all, active, waiting, finished

    public var title: String {
        switch self {
        case .all: "全部会话"
        case .active: "运行中"
        case .waiting: "等待输入"
        case .finished: "已结束"
        }
    }

    public func matches(_ status: String?) -> Bool {
        switch self {
        case .all: true
        case .active: ["pending", "running", "detached"].contains(status ?? "")
        case .waiting: status == "input_required"
        case .finished: ["done", "failed", "cancelled"].contains(status ?? "")
        }
    }
}

/// codeg-style action row at the top of the sidebar. A standalone button,
/// not a selection state: hover paints a subdued fill only.
struct SidebarActionRow: View {
    let systemImage: String
    let title: String
    let shortcut: String?
    let identifier: String
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: systemImage)
                    .frame(width: 16)
                    .foregroundStyle(SessionPalette.secondary)
                Text(title)
                    .foregroundStyle(SessionPalette.text)
                Spacer(minLength: 4)
                if let shortcut {
                    Text(shortcut)
                        .font(AeroricFont.ui(AeroricFont.meta))
                        .foregroundStyle(SessionPalette.muted)
                }
            }
            .font(AeroricFont.ui(AeroricFont.body, weight: .medium))
            .padding(.horizontal, 8)
            .frame(height: 28)
            .contentShape(Capsule(style: .continuous))
            .background {
                if hovered {
                    Capsule(style: .continuous)
                        .fill(SessionPalette.hover)
                }
            }
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help("\(title)\(shortcut.map { "（\($0)）" } ?? "")")
        .accessibilityIdentifier(identifier)
    }
}

/// Selectable capsule row (codeg sidebar `h-8 rounded-full`): no fill at
/// rest, hover fill, stronger fill when selected.
struct SidebarCapsuleRow<Label: View>: View {
    let selected: Bool
    let height: CGFloat
    var horizontalPadding: CGFloat = 10
    let action: () -> Void
    @ViewBuilder let label: () -> Label
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            label()
                .padding(.horizontal, horizontalPadding)
                .frame(maxWidth: .infinity, minHeight: height, maxHeight: height, alignment: .leading)
                .background {
                    if selected || hovered {
                        Capsule(style: .continuous)
                            .fill(selected ? SessionPalette.selection : SessionPalette.hover)
                    }
                }
                .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Project row: disclosure chevron, folder glyph, name, session count and a
/// hover "新建会话" button, all inside one flat capsule.
private struct ProjectRow: View {
    let item: SidebarItem
    let sessionCount: Int
    let expanded: Bool
    let selected: Bool
    let onSelect: () -> Void
    let onToggle: () -> Void
    let onNewTask: () -> Void
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 2) {
            Button(action: onToggle) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(SessionPalette.muted)
                    .rotationEffect(.degrees(expanded ? 90 : 0))
                    .frame(width: 16, height: 24)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(expanded ? "收起 \(item.title)" : "展开 \(item.title)")
            Button(action: onSelect) {
                HStack(spacing: 6) {
                    Image(systemName: expanded ? "folder.fill" : "folder")
                        .font(AeroricFont.ui(AeroricFont.project))
                        .foregroundStyle(SessionPalette.secondary)
                        .frame(width: 16)
                    Text(item.title)
                        .font(AeroricFont.ui(AeroricFont.project, weight: .medium))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 2)
                    if sessionCount > 0 && !hovered {
                        Text("\(sessionCount)")
                            .font(AeroricFont.ui(AeroricFont.meta))
                            .monospacedDigit()
                            .foregroundStyle(SessionPalette.muted)
                    }
                }
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(selected ? .isSelected : [])
            if hovered {
                Button(action: onNewTask) {
                    Image(systemName: "plus")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(SessionPalette.secondary)
                        .frame(width: 22, height: 22)
                        .background(Circle().fill(SessionPalette.selection))
                }
                .buttonStyle(.plain)
                .help("在\(item.title)中新建会话")
                .accessibilityLabel("在\(item.title)中新建会话")
                .accessibilityIdentifier("sidebar.new-task.\(item.id)")
            }
        }
        .padding(.leading, 4)
        .padding(.trailing, 4)
        .frame(height: NativePanelGeometry.mainShell.projectRowHeight)
        .background {
            if selected || hovered {
                Capsule(style: .continuous)
                    .fill(selected ? SessionPalette.selection : SessionPalette.hover)
            }
        }
        .onHover { hovered = $0 }
        .help(item.title)
    }
}

/// Session row: status dot, title, pin marker.
private struct SessionRow: View {
    let item: SidebarItem

    var body: some View {
        HStack(spacing: 8) {
            SessionStatusDot(status: item.status)
            Text(item.title)
                .font(AeroricFont.ui(AeroricFont.project))
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 2)
            if item.pinned {
                Image(systemName: "pin.fill")
                    .font(.system(size: 9))
                    .foregroundStyle(SessionPalette.muted)
                    .accessibilityLabel("已置顶")
            }
        }
        .help("\(item.title) · \(SessionStatusDot.label(item.status))")
    }
}

/// Small coloured dot for a host task status. Colour is never the only
/// signal: the label is the accessibility value and tooltip.
public struct SessionStatusDot: View {
    let status: String?
    public init(status: String?) { self.status = status }

    public var body: some View {
        Circle()
            .fill(Self.color(status))
            .frame(width: 6, height: 6)
            .frame(width: 12)
            .accessibilityLabel(Self.label(status))
    }

    public static func color(_ status: String?) -> Color {
        switch status {
        case "running", "pending", "detached": SessionPalette.teal
        case "input_required": SessionPalette.amber
        case "failed": Color.dsDanger
        case "cancelled", "done": SessionPalette.muted
        default: SessionPalette.edge
        }
    }

    public static func label(_ status: String?) -> String {
        switch status {
        case "pending": "排队中"
        case "running": "运行中"
        case "detached": "后台运行"
        case "input_required": "等待输入"
        case "done": "已完成"
        case "failed": "失败"
        case "cancelled": "已取消"
        default: status ?? "未知状态"
        }
    }
}

/// One plain navigation row: leading glyph, truncating title, trailing count.
struct SidebarRow: View {
    let item: SidebarItem

    var body: some View {
        HStack(spacing: 8) {
            if let systemImage = item.systemImage {
                Image(systemName: systemImage)
                    .font(AeroricFont.ui(AeroricFont.project))
                    .frame(width: 16)
                    .foregroundStyle(SessionPalette.secondary)
            }
            Text(item.title)
                .font(AeroricFont.ui(AeroricFont.project))
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 4)
            if let badge = item.badge, badge > 0 {
                Text("\(badge)")
                    .font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                    .monospacedDigit()
                    .foregroundStyle(SessionPalette.muted)
            }
        }
        .help(item.title)
    }
}

/// One rail row: centred glyph with its title as tooltip and AX label.
struct RailRow: View {
    let item: SidebarItem

    var body: some View {
        Image(systemName: item.systemImage ?? "circle")
            .font(.system(size: 15))
            .foregroundStyle(SessionPalette.secondary)
            .frame(maxWidth: .infinity, alignment: .center)
            .help(item.title)
            .accessibilityLabel(item.title)
    }
}

/// Shared empty-state anatomy: centred glyph, a title, and a next-step hint.
public struct PaneEmptyState: View {
    let systemImage: String
    let title: String
    let hint: String

    public init(systemImage: String, title: String, hint: String) {
        self.systemImage = systemImage
        self.title = title
        self.hint = hint
    }

    public var body: some View {
        VStack(spacing: 6) {
            Image(systemName: systemImage)
                .font(.system(size: 22))
                .foregroundStyle(SessionPalette.muted.opacity(0.7))
                .padding(.bottom, 2)
            Text(title)
                .font(AeroricFont.ui(AeroricFont.body, weight: .medium))
                .foregroundStyle(SessionPalette.secondary)
            Text(hint)
                .font(AeroricFont.ui(AeroricFont.meta))
                .foregroundStyle(SessionPalette.muted)
                .multilineTextAlignment(.center)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
