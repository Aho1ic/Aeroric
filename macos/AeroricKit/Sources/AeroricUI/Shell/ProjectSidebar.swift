import SwiftUI

/// Overhaul M1 navigation column: glass sidebar with codeg-style navigation
/// (新会话 / 搜索 / 自动化 / 任务), the project → session tree with capsule
/// hover, and a footer with settings and the connection state. Tool surfaces
/// (Git, 数据库, IDE …) live in the toolbar's “更多” menu and ⌘K.
///
/// Width policy: 260pt default, 220–420pt drag range, 56pt icon rail when
/// collapsed or when the window is narrower than 1000pt.
public struct ProjectSidebar: View {
    @Binding var selection: String?
    let items: [SidebarItem]
    let width: CGFloat
    /// Collapsed = the 56pt icon rail, never a hidden column.
    let collapsed: Bool
    @Binding private var expandedProjects: Set<String>
    private let phase: ConnectionPhaseSnapshot?
    private let phaseText: String?
    private let onNewProjectTask: (String) -> Void
    private let onMoveSelection: (MoveCommandDirection) -> Void
    private let onOpenFolder: () -> Void
    private let onOpenSettings: () -> Void
    private let onAddProject: (String) -> Void
    /// Window-local status filter. Presentation only; never a host query.
    @State private var filter: SidebarStatusFilter = .all

    public static let collapsedWidth: CGFloat = NativePanelGeometry.mainShell.railWidth

    public init(
        selection: Binding<String?>,
        items: [SidebarItem],
        width: CGFloat,
        collapsed: Bool = false,
        expandedProjects: Binding<Set<String>> = .constant([]),
        phase: ConnectionPhaseSnapshot? = nil,
        phaseText: String? = nil,
        onNewProjectTask: @escaping (String) -> Void = { _ in },
        onMoveSelection: @escaping (MoveCommandDirection) -> Void = { _ in },
        onOpenFolder: @escaping () -> Void = {},
        onOpenSettings: @escaping () -> Void = {},
        onAddProject: @escaping (String) -> Void = { _ in }
    ) {
        self._selection = selection
        self.items = items
        self.width = width
        self.collapsed = collapsed
        _expandedProjects = expandedProjects
        self.phase = phase
        self.phaseText = phaseText
        self.onNewProjectTask = onNewProjectTask
        self.onMoveSelection = onMoveSelection
        self.onOpenFolder = onOpenFolder
        self.onOpenSettings = onOpenSettings
        self.onAddProject = onAddProject
    }

    private var workItems: [SidebarItem] {
        items.filter { $0.group == .work && $0.id != SessionNavigation.globalDraft }
    }
    private var projectItems: [SidebarItem] { items.filter { $0.group == .projects } }
    private var importItems: [SidebarItem] { items.filter { $0.group == .imports } }
    @State private var importsExpanded = true
    private var shell: NativePanelGeometry.MainShell { NativePanelGeometry.mainShell }

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
        .foregroundStyle(AeroricColor.text)
        .background(Color.clear.glassSurface(.sidebar).ignoresSafeArea())
    }

    // MARK: Expanded

    private var expandedContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            navigation
                .padding(.horizontal, AeroricSpace.m)
                .padding(.top, AeroricSpace.s)
                .padding(.bottom, AeroricSpace.xs)
            if items.isEmpty {
                emptyContent
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 1) {
                        projectsHeader
                            .padding(.top, AeroricSpace.m)
                        if projectItems.isEmpty {
                            Text("尚未打开项目；文件 → 打开项目…")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .padding(.horizontal, AeroricSpace.m)
                                .padding(.vertical, AeroricSpace.xs)
                        }
                        ForEach(projectItems) { project in
                            projectGroup(project)
                        }
                        if !importItems.isEmpty { importsGroup }
                    }
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.bottom, AeroricSpace.m)
                }
                .scrollIndicators(.never)
                .focusable()
                .focusEffectDisabled()
                .onMoveCommand(perform: onMoveSelection)
                .accessibilityIdentifier("sidebar.list")
            }
            footer
        }
    }

    private var navigation: some View {
        VStack(alignment: .leading, spacing: 1) {
            HoverCapsuleRow(selected: selection == SessionNavigation.globalDraft,
                            height: shell.navigationRowHeight,
                            action: { selection = SessionNavigation.globalDraft }) {
                NavigationLabel(systemImage: "square.and.pencil", title: "新会话") { KeyChip("⌘N") }
            }
            .help("新会话（⌘N）")
            .accessibilityIdentifier("sidebar.new-session")
            HoverCapsuleRow(selected: false, height: shell.navigationRowHeight, action: {}) {
                NavigationLabel(systemImage: "bolt", title: "自动化") { ComingSoonBadge() }
                    .opacity(0.55)
            }
            .disabled(true)
            .help("自动化：即将支持")
            .accessibilityIdentifier("sidebar.automations")
            ForEach(workItems) { item in
                HoverCapsuleRow(selected: selection == item.id, height: shell.navigationRowHeight,
                                action: { selection = item.id }) {
                    SidebarRow(item: item)
                }
                .accessibilityIdentifier("sidebar.item.\(item.id)")
            }
        }
    }

    private func sessions(of project: SidebarItem) -> [SidebarItem] {
        (project.children ?? []).filter {
            (SessionNavigation.taskID($0.id) != nil && filter.matches($0.status))
                || (SessionNavigation.importedID($0.id) != nil && filter == .all)
        }
    }

    private var projectsHeader: some View {
        SectionHeader("项目") {
            HStack(spacing: 2) {
                openFolderButton
                filterMenu
            }
        }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("sidebar.group.projects")
    }

    @ViewBuilder private func projectGroup(_ project: SidebarItem) -> some View {
        let expanded = expandedProjects.contains(project.id)
        let rows = sessions(of: project)
        ProjectRow(item: project, sessionCount: rows.count, expanded: expanded,
                   selected: selection == project.id, height: shell.projectRowHeight,
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
                HoverCapsuleRow(selected: selection == child.id, height: shell.sessionRowHeight,
                                action: { selection = child.id }) {
                    SessionRow(item: child)
                }
                .padding(.leading, AeroricSpace.l)
                .accessibilityIdentifier("sidebar.item.\(child.id)")
            }
            if rows.isEmpty {
                Text(filter == .all ? "还没有会话" : "没有符合筛选的会话")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.leading, 36)
                    .frame(height: 24)
            }
        }
    }

    /// Imported sessions whose cwd matches no open project. Each row can turn
    /// its directory into a project, which moves the sessions under it.
    @ViewBuilder private var importsGroup: some View {
        Button { importsExpanded.toggle() } label: {
            HStack(spacing: AeroricSpace.xs) {
                SectionHeader("未归类导入 · \(importItems.count)")
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .rotationEffect(.degrees(importsExpanded ? 90 : 0))
                    .padding(.trailing, AeroricSpace.m)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.top, AeroricSpace.l)
        .accessibilityIdentifier("sidebar.group.imports")
        if importsExpanded {
            ForEach(importItems) { item in
                HoverCapsuleRow(selected: selection == item.id, height: shell.sessionRowHeight,
                                action: { selection = item.id }) {
                    SessionRow(item: item)
                }
                .contextMenu {
                    if let path = item.path {
                        Button("将 \(URL(fileURLWithPath: path).lastPathComponent) 添加为项目") { onAddProject(path) }
                    }
                }
                .help(item.path.map { "\(item.title)\n\($0)" } ?? item.title)
                .accessibilityIdentifier("sidebar.item.\(item.id)")
            }
        }
    }

    /// Folder picker → open (or re-open) as a project and select it.
    private var openFolderButton: some View {
        Button(action: onOpenFolder) {
            Image(systemName: "folder.badge.plus")
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .buttonStyle(.borderless)
        .help("打开文件夹为项目…")
        .accessibilityLabel("打开文件夹为项目")
        .accessibilityIdentifier("sidebar.open-folder")
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
                .foregroundStyle(filter == .all ? AeroricColor.textTertiary : AeroricColor.accent)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("按状态筛选会话")
        .accessibilityLabel("按状态筛选会话：\(filter.title)")
        .accessibilityIdentifier("sidebar.filter")
    }

    private var footer: some View {
        HStack(spacing: AeroricSpace.xs) {
            Button(action: onOpenSettings) {
                Label("设置", systemImage: "gearshape")
                    .labelStyle(.titleAndIcon)
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .help("设置（⌘,）")
            .accessibilityIdentifier("sidebar.settings")
            Spacer(minLength: AeroricSpace.xs)
            if let phase {
                ConnectionIndicator(phase: phase, text: phaseText)
            }
        }
        .padding(.horizontal, AeroricSpace.m)
        .frame(height: 40)
        .overlay(alignment: .top) { HairlineDivider() }
    }

    // MARK: Rail

    private var railContent: some View {
        VStack(spacing: 0) {
            VStack(spacing: 2) {
                railButton("square.and.pencil", "新会话", "sidebar.new-session") {
                    selection = SessionNavigation.globalDraft
                }
            }
            .padding(.top, AeroricSpace.s)
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(workItems + projectItems) { item in
                        HoverCapsuleRow(selected: selection == item.id, height: 32, horizontalPadding: 0,
                                        action: { selection = item.id }) {
                            RailRow(item: item)
                        }
                        .accessibilityIdentifier("sidebar.item.\(item.id)")
                    }
                }
                .padding(.horizontal, AeroricSpace.m)
                .padding(.top, AeroricSpace.s)
            }
            .scrollIndicators(.never)
            .focusable()
            .focusEffectDisabled()
            .onMoveCommand(perform: onMoveSelection)
            .accessibilityIdentifier("sidebar.rail")
            railButton("gearshape", "设置", "sidebar.settings", action: onOpenSettings)
                .padding(.bottom, AeroricSpace.s)
        }
    }

    private func railButton(_ systemImage: String, _ label: String, _ identifier: String,
                            action: @escaping () -> Void) -> some View {
        HoverCapsuleRow(selected: false, height: 32, horizontalPadding: 0, action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 14))
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(maxWidth: .infinity)
        }
        .frame(width: 40)
        .help(label)
        .accessibilityLabel(label)
        .accessibilityIdentifier(identifier)
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

/// Navigation row label: glyph, title, trailing accessory (key chip / badge).
struct NavigationLabel<Trailing: View>: View {
    let systemImage: String
    let title: String
    @ViewBuilder let trailing: () -> Trailing

    var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 13))
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: 16)
            Text(title)
                .font(.system(size: AeroricType.body))
                .foregroundStyle(AeroricColor.text)
            Spacer(minLength: AeroricSpace.xs)
            trailing()
        }
    }
}

/// Project row (DSH): the folder glyph turns into the disclosure chevron on
/// hover; the session count gives way to a "新建会话" button.
private struct ProjectRow: View {
    let item: SidebarItem
    let sessionCount: Int
    let expanded: Bool
    let selected: Bool
    let height: CGFloat
    let onSelect: () -> Void
    let onToggle: () -> Void
    let onNewTask: () -> Void
    @State private var hovered = false

    var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            Button(action: onToggle) {
                Group {
                    if hovered {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10, weight: .semibold))
                            .rotationEffect(.degrees(expanded ? 90 : 0))
                    } else {
                        Image(systemName: expanded ? "folder.fill" : "folder")
                            .font(.system(size: 13))
                    }
                }
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: 18, height: 22)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(expanded ? "收起 \(item.title)" : "展开 \(item.title)")
            Button(action: onSelect) {
                HStack(spacing: AeroricSpace.s) {
                    Text(item.title)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 2)
                    if sessionCount > 0 && !hovered {
                        Text("\(sessionCount)")
                            .font(.system(size: AeroricType.caption).monospacedDigit())
                            .foregroundStyle(AeroricColor.textTertiary)
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
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 22, height: 22)
                        .background(Circle().fill(AeroricColor.hover))
                }
                .buttonStyle(.plain)
                .help("在\(item.title)中新建会话")
                .accessibilityLabel("在\(item.title)中新建会话")
                .accessibilityIdentifier("sidebar.new-task.\(item.id)")
            }
        }
        .padding(.horizontal, AeroricSpace.xs)
        .frame(height: height)
        .background {
            if selected || hovered {
                Capsule(style: .continuous)
                    .fill(selected ? AeroricColor.selection : AeroricColor.hover)
            }
        }
        .onHover { hovered = $0 }
        .help(item.title)
    }
}

/// Session row: agent glyph (who), title, relative time, status dot (what).
private struct SessionRow: View {
    let item: SidebarItem

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            AgentGlyph(agent: item.agent, size: 14)
            Text(item.title)
                .font(.system(size: AeroricType.body))
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 2)
            if item.imported {
                Text("已导入")
                    .font(.system(size: AeroricType.caption - 1, weight: .medium))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.horizontal, AeroricSpace.xs)
                    .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                    .accessibilityLabel("已导入，只读")
            }
            if item.pinned {
                Image(systemName: "pin.fill")
                    .font(.system(size: 9))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityLabel("已置顶")
            }
            if let updatedAt = item.updatedAt {
                Text(RelativeTime.short(updatedAt))
                    .font(.system(size: AeroricType.caption).monospacedDigit())
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            SessionStatusDot(status: item.status)
        }
        .help("\(item.title) · \(SessionStatusDot.label(item.status))")
    }
}

/// Status dot for a host task status (wire vocabulary). Forwards to the shared
/// `StatusDot`; the label is the accessibility value and tooltip.
public struct SessionStatusDot: View {
    let status: String?
    public init(status: String?) { self.status = status }

    public var body: some View {
        StatusDot(ActivityState(wireStatus: status), label: Self.label(status))
    }

    public static func color(_ status: String?) -> Color {
        ActivityState(wireStatus: status).color
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

/// Connection state in the sidebar footer: dot + short text.
struct ConnectionIndicator: View {
    let phase: ConnectionPhaseSnapshot
    let text: String?

    var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            Circle().fill(Self.color(phase)).frame(width: 6, height: 6)
            Text(Self.shortLabel(phase))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .help(text ?? Self.shortLabel(phase))
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("sidebar.connection")
    }

    static func color(_ phase: ConnectionPhaseSnapshot) -> Color {
        switch DetailState.resolve(phase) {
        case .loading: AeroricColor.warning
        case .empty: AeroricColor.success
        case .disconnected, .incompatible: AeroricColor.danger
        }
    }

    static func shortLabel(_ phase: ConnectionPhaseSnapshot) -> String {
        switch DetailState.resolve(phase) {
        case .loading: "连接中"
        case .empty: "已连接"
        case .disconnected: "已断开"
        case .incompatible: "不兼容"
        }
    }
}

/// One plain navigation row: leading glyph, truncating title, trailing count.
struct SidebarRow: View {
    let item: SidebarItem

    var body: some View {
        HStack(spacing: AeroricSpace.m) {
            if let systemImage = item.systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: 13))
                    .frame(width: 16)
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Text(item.title)
                .font(.system(size: AeroricType.body))
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 4)
            if let badge = item.badge, badge > 0 {
                Text("\(badge)")
                    .font(.system(size: AeroricType.caption, weight: .medium).monospacedDigit())
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(.horizontal, 6)
                    .frame(height: 16)
                    .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
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
            .font(.system(size: 14))
            .foregroundStyle(AeroricColor.textSecondary)
            .frame(maxWidth: .infinity, alignment: .center)
            .help(item.title)
            .accessibilityLabel(item.title)
    }
}

/// Shared empty-state anatomy for panes. Forwards to `AeroricEmptyState`.
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
        AeroricEmptyState(systemImage: systemImage, title: title, message: hint)
    }
}
