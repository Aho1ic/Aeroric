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
    private let onOpenRemote: () -> Void
    private let onOpenSettings: () -> Void
    private let onAddProject: (String) -> Void
    private let onSessionAction: (String, SessionRowAction) -> Void
    private let onProjectAction: (String, ProjectRowAction) -> Void
    private let projectGroupNames: [String]
    private let agentChoices: (String) -> [AgentChoice]
    private let allSessions: (String) -> [SidebarItem]
    /// The session waiting for 移除 to be confirmed.
    @State private var removing: SidebarItem?
    @State private var removingProject: SidebarItem?
    @State private var renamingProject: SidebarItem?
    @State private var renamingSession: SidebarItem?
    @State private var groupingProject: SidebarItem?
    @State private var managingProject: SidebarItem?
    @State private var renameText = ""
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Window-local status filter. Presentation only; never a host query.
    @State private var filter: SidebarStatusFilter = .all
    /// Tags (2026-10-29): the library, the window-local tag filter (one tag,
    /// Q1-a) and the two tag sheets.
    @Environment(TagBook.self) private var tagBook: TagBook?
    @State private var tagFilter: String?
    @State private var draftTag: TagDraftTarget?
    @State private var managingTags = false

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
        onOpenRemote: @escaping () -> Void = {},
        onOpenSettings: @escaping () -> Void = {},
        onAddProject: @escaping (String) -> Void = { _ in },
        onSessionAction: @escaping (String, SessionRowAction) -> Void = { _, _ in },
        onProjectAction: @escaping (String, ProjectRowAction) -> Void = { _, _ in },
        projectGroupNames: [String] = [],
        agentChoices: @escaping (String) -> [AgentChoice] = { _ in [] },
        allSessions: @escaping (String) -> [SidebarItem] = { _ in [] }
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
        self.onOpenRemote = onOpenRemote
        self.onOpenSettings = onOpenSettings
        self.onAddProject = onAddProject
        self.onSessionAction = onSessionAction
        self.onProjectAction = onProjectAction
        self.projectGroupNames = projectGroupNames
        self.agentChoices = agentChoices
        self.allSessions = allSessions
    }

    private var workItems: [SidebarItem] {
        items.filter { $0.group == .work && $0.id != SessionNavigation.globalDraft }
    }
    private var projectItems: [SidebarItem] { items.filter { $0.group == .projects } }
    private var importItems: [SidebarItem] { items.filter { $0.group == .imports } }
    @State private var importsExpanded = true
    private var archivedItems: [SidebarItem] { items.filter { $0.group == .archived } }
    @State private var archivedExpanded = false
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
        .confirmationDialog("从 Aeroric 移除这个会话？", isPresented: Binding(
            get: { removing != nil }, set: { if !$0 { removing = nil } }
        ), presenting: removing) { item in
            Button("移除", role: .destructive) { onSessionAction(item.id, .remove) }
            Button("取消", role: .cancel) {}
        } message: { item in
            Text(item.imported
                 ? "“\(item.title)”将不再出现在 Aeroric 中。Agent 自己的会话文件不会被删除，以后可在 设置 → 导入 中重新勾选导入。"
                 : "“\(item.title)”会从 Aeroric 的会话列表里删除。Agent 自己的会话文件不会被删除。已置顶的会话需要先取消置顶。")
        }
        .confirmationDialog("从 Aeroric 移除这个工作区？", isPresented: Binding(
            get: { removingProject != nil }, set: { if !$0 { removingProject = nil } }
        ), presenting: removingProject) { project in
            Button("移除", role: .destructive) { onProjectAction(project.id, .remove) }
            Button("取消", role: .cancel) {}
        } message: { project in
            Text("“\(project.title)”会从侧栏移走。磁盘上的文件和 Agent 的会话记录都不动；以后重新打开这个文件夹，它的会话会回来。")
        }
        .alert("重命名工作区", isPresented: Binding(
            get: { renamingProject != nil }, set: { if !$0 { renamingProject = nil } }
        ), presenting: renamingProject) { project in
            TextField("留空则恢复为文件夹名", text: $renameText)
            Button("保存") { onProjectAction(project.id, .rename(renameText)) }
            Button("取消", role: .cancel) {}
        } message: { _ in
            Text("只改 Aeroric 里显示的名字，不改磁盘上的文件夹名。")
        }
        .alert("重命名会话", isPresented: Binding(
            get: { renamingSession != nil }, set: { if !$0 { renamingSession = nil } }
        ), presenting: renamingSession) { item in
            TextField("会话标题", text: $renameText)
            Button("保存") { onSessionAction(item.id, .rename(renameText)) }
            Button("取消", role: .cancel) {}
        }
        .alert("新建分组", isPresented: Binding(
            get: { groupingProject != nil }, set: { if !$0 { groupingProject = nil } }
        ), presenting: groupingProject) { project in
            TextField("分组名称", text: $renameText)
            Button("新建并移入") {
                let name = renameText.trimmingCharacters(in: .whitespaces)
                if !name.isEmpty { onProjectAction(project.id, .group(name)) }
            }
            Button("取消", role: .cancel) {}
        }
        .sheet(item: $draftTag) { target in
            if let tagBook { TagEditorSheet(target: target, book: tagBook, onClose: { draftTag = nil }) }
        }
        .sheet(isPresented: $managingTags) {
            if let tagBook {
                TagManagerSheet(book: tagBook, projects: projectItems.map { (id: $0.id, title: $0.title) },
                                onClose: { managingTags = false })
            }
        }
        .onChange(of: tagBook?.tags.map(\.id) ?? []) { _, ids in
            if let tagFilter, !ids.contains(tagFilter) { self.tagFilter = nil }
        }
        .sheet(item: $managingProject) { project in
            SessionManagerSheet(project: project, sessions: allSessions(project.id),
                                onAction: onSessionAction, onClose: { managingProject = nil })
        }
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
                        ForEach(SidebarItem.groupedProjects(projectItems, order: projectGroupNames),
                                id: \.name) { section in
                            if let name = section.name.isEmpty ? nil : section.name {
                                Text(name)
                                    .font(.system(size: AeroricType.caption, weight: .medium))
                                    .foregroundStyle(AeroricColor.textTertiary)
                                    .padding(.horizontal, AeroricSpace.m)
                                    .padding(.top, AeroricSpace.s)
                                    .accessibilityIdentifier("sidebar.project-group.\(name)")
                            }
                            ForEach(section.projects) { project in
                                projectGroup(project)
                            }
                        }
                        if !visibleImports.isEmpty { importsGroup }
                        if !archivedItems.isEmpty { archivedGroup }
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
        let rows = (project.children ?? []).filter {
            (SessionNavigation.taskID($0.id) != nil && filter.matches($0.status))
                || (SessionNavigation.importedID($0.id) != nil && filter == .all)
        }
        guard let tagFilter, let tagBook else { return rows }
        let kept = Set(TagBook.filter(project: project.id, sessions: rows.map(\.id), by: tagFilter,
                                      in: tagBook.library).sessions)
        return rows.filter { kept.contains($0.id) }
    }

    /// With a tag filter on, a project shows only when it or one of its
    /// sessions carries the tag (Q2-a).
    private func showsProject(_ project: SidebarItem) -> Bool {
        guard let tagFilter, let tagBook else { return true }
        return TagBook.filter(project: project.id, sessions: (project.children ?? []).map(\.id),
                              by: tagFilter, in: tagBook.library).showsProject
    }

    private var visibleImports: [SidebarItem] {
        guard let tagFilter, let tagBook else { return importItems }
        return importItems.filter { tagBook.tags(on: $0.id).contains { $0.id == tagFilter } }
    }

    /// The project a session row sits under, for its 仅当前项目 tags.
    private func projectID(of item: SidebarItem) -> String? {
        projectItems.first { $0.children?.contains { $0.id == item.id } == true }?.id
    }

    private func projectTitle(_ id: String?) -> String? {
        id.flatMap { id in projectItems.first { $0.id == id }?.title }
    }

    /// 设置标签 › (Q5): the tags this row may carry with a native tick on
    /// the ones it has (Q4: library order), each with its colour dot; then
    /// 新建标签… (put on this row) and 管理标签….
    @ViewBuilder private func tagMenu(_ item: SidebarItem, isProject: Bool, projectID: String?) -> some View {
        if let tagBook {
            Menu("设置标签") {
                let on = Set(tagBook.tags(on: item.id).map(\.id))
                let offered = tagBook.available(isProject: isProject, projectID: projectID)
                ForEach(offered) { tag in
                    Toggle(isOn: Binding(
                        get: { on.contains(tag.id) },
                        set: { _ in tagBook.toggle(tag, on: item.id, isProject: isProject, projectID: projectID) }
                    )) {
                        Label { Text(tag.projectId == nil ? tag.name : "\(tag.name)（仅本项目）") } icon: {
                            Image(nsImage: TagDot.image(tag.color))
                        }
                    }
                }
                if !offered.isEmpty { Divider() }
                Button("新建标签…", systemImage: "plus") {
                    draftTag = TagDraftTarget(projectID: projectID, projectTitle: projectTitle(projectID),
                                              row: (item.id, isProject, projectID))
                }
                Button("管理标签…", systemImage: "tag") { managingTags = true }
            }
        }
    }

    private var projectsHeader: some View {
        SectionHeader("项目") {
            HStack(spacing: 2) {
                if let tagFilter, let tag = tagBook?.tags.first(where: { $0.id == tagFilter }) {
                    Button { self.tagFilter = nil } label: {
                        HStack(spacing: 3) {
                            Text("标签：\(tag.name)").lineLimit(1)
                            Image(systemName: "xmark").font(.system(size: 8, weight: .bold))
                        }
                        .font(.system(size: AeroricType.caption - 1, weight: .medium))
                        .foregroundStyle(ProjectColor.color(tag.color) ?? AeroricColor.accent)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background((ProjectColor.color(tag.color) ?? AeroricColor.accent).opacity(0.16),
                                    in: Capsule(style: .continuous))
                        .fixedSize()
                    }
                    .buttonStyle(.plain)
                    .help("清除标签筛选")
                    .accessibilityIdentifier("sidebar.tag-filter.clear")
                }
                openFolderButton
                filterMenu
            }
        }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("sidebar.group.projects")
    }

    @ViewBuilder private func projectGroup(_ project: SidebarItem) -> some View {
        if showsProject(project) { projectGroupRows(project) }
    }

    @ViewBuilder private func projectGroupRows(_ project: SidebarItem) -> some View {
        // A tag filter shows its matches without unfolding by hand.
        let expanded = expandedProjects.contains(project.id) || tagFilter != nil
        let rows = sessions(of: project)
        ProjectRow(item: project, sessionCount: rows.count, expanded: expanded,
                   selected: selection == project.id, height: shell.projectRowHeight,
                   // 2026-10-28: a click on the project folds or unfolds its
                   // sessions; the task board stays under 任务.
                   onSelect: {
                       if expanded { expandedProjects.remove(project.id) } else { expandedProjects.insert(project.id) }
                   },
                   onToggle: {
                       if expanded { expandedProjects.remove(project.id) } else { expandedProjects.insert(project.id) }
                   },
                   onNewTask: { onNewProjectTask(project.id) },
                   menu: { projectMenu(project) })
            .contextMenu { projectMenu(project) }
            .accessibilityIdentifier("sidebar.item.\(project.id)")
        if expanded {
            ForEach(rows) { child in
                sessionTreeRow(child)
                    .padding(.leading, AeroricSpace.l)
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
        collapsibleHeader("未归类导入 · \(visibleImports.count)", expanded: $importsExpanded,
                          identifier: "sidebar.group.imports")
        if importsExpanded {
            ForEach(visibleImports) { item in
                sessionTreeRow(item, contextMenu: false)
                .contextMenu {
                    if let path = item.path {
                        Button("将 \(URL(fileURLWithPath: path).lastPathComponent) 添加为项目") { onAddProject(path) }
                        Divider()
                    }
                    sessionMenu(item)
                }
                .help(item.path.map { "\(item.title)\n\($0)" } ?? item.title)
                .accessibilityIdentifier("sidebar.item.\(item.id)")
            }
        }
    }

    /// Archived sessions, collapsed by default; 取消归档 puts a row back.
    @ViewBuilder private var archivedGroup: some View {
        collapsibleHeader("已归档 · \(archivedItems.count)", expanded: $archivedExpanded,
                          identifier: "sidebar.group.archived")
        if archivedExpanded {
            ForEach(archivedItems) { item in
                sessionTreeRow(item, archived: true)
                    .opacity(0.75)
            }
        }
    }

    private func collapsibleHeader(_ title: String, expanded: Binding<Bool>, identifier: String) -> some View {
        Button { withAnimation(.easeOut(duration: AeroricMotion.duration(AeroricMotion.standard, reduceMotion: reduceMotion))) { expanded.wrappedValue.toggle() } } label: {
            HStack(spacing: AeroricSpace.xs) {
                SectionHeader(title)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .rotationEffect(.degrees(expanded.wrappedValue ? 90 : 0))
                    .padding(.trailing, AeroricSpace.m)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.top, AeroricSpace.l)
        .accessibilityIdentifier(identifier)
    }

    // MARK: 更多 menus (2026-10-28)

    /// The full session menu, shared by the row's ⋯ button and its context menu.
    @ViewBuilder private func sessionMenu(_ item: SidebarItem, archived: Bool = false) -> some View {
        let isTask = SessionNavigation.taskID(item.id) != nil
        if isTask {
            Button(item.pinned ? "取消置顶" : "置顶", systemImage: item.pinned ? "pin.slash" : "pin") {
                onSessionAction(item.id, item.pinned ? .unpin : .pin)
            }
            Button("重命名…", systemImage: "pencil") {
                renameText = item.title
                renamingSession = item
            }
            Button("分叉会话", systemImage: "arrow.triangle.branch") { onSessionAction(item.id, .fork) }
                .disabled(SidebarItem.forkUnavailableReason(item) != nil)
                .help(SidebarItem.forkUnavailableReason(item) ?? "从这里复制出一个新会话，原会话不受影响")
        }
        Button("复制 Session ID", systemImage: "doc.on.doc") { onSessionAction(item.id, .copySessionID) }
            .disabled(item.sessionID == nil)
        tagMenu(item, isProject: false, projectID: projectID(of: item))
        Divider()
        if archived {
            Button("取消归档", systemImage: "tray.and.arrow.up") { onSessionAction(item.id, .unarchive) }
        } else {
            Button("归档", systemImage: "archivebox") { onSessionAction(item.id, .archive) }
                .disabled(["pending", "running"].contains(item.status ?? ""))
        }
        Button(item.imported ? "从 Aeroric 移除…" : "移除会话…", systemImage: "trash", role: .destructive) {
            removing = item
        }
        .disabled(isTask && ["pending", "running"].contains(item.status ?? ""))
    }

    @ViewBuilder private func projectMenu(_ project: SidebarItem) -> some View {
        Button("重命名…", systemImage: "pencil") {
            renameText = project.title
            renamingProject = project
        }
        tagMenu(project, isProject: true, projectID: project.id)
        Menu("修改颜色") {
            ForEach(ProjectColor.all) { color in
                Button {
                    onProjectAction(project.id, .color(color.id))
                } label: {
                    Label { Text(color.title) } icon: {
                        Image(systemName: project.color == color.id ? "checkmark.circle" : "folder")
                            .foregroundStyle(color.color)
                    }
                }
            }
            Divider()
            Button("不设置") { onProjectAction(project.id, .color(nil)) }
        }
        Menu("设置默认 Harness") {
            ForEach(agentChoices(project.id)) { choice in
                Button {
                    onProjectAction(project.id, .defaultAgent(choice.id))
                } label: {
                    Label {
                        Text(project.defaultAgent == choice.id ? "\(choice.title)  ✓" : choice.title)
                    } icon: {
                        if let asset = HarnessLogo.asset(harness: choice.id, agent: nil) {
                            Image(asset).renderingMode(.template)
                        }
                    }
                }
            }
            Divider()
            Button("不设置（沿用全局默认）") { onProjectAction(project.id, .defaultAgent(nil)) }
                .disabled(project.defaultAgent == nil)
        }
        Button("会话管理…", systemImage: "list.bullet.rectangle") { managingProject = project }
        Menu("移动到分组") {
            ForEach(projectGroupNames, id: \.self) { name in
                Button(project.projectGroup == name ? "✓ \(name)" : name) { onProjectAction(project.id, .group(name)) }
            }
            if !projectGroupNames.isEmpty { Divider() }
            Button("新建分组…") {
                renameText = ""
                groupingProject = project
            }
            Button("不分组") { onProjectAction(project.id, .group(nil)) }
                .disabled(project.projectGroup == nil)
        }
        Divider()
        Button("从 Aeroric 移除工作区…", systemImage: "trash", role: .destructive) { removingProject = project }
    }

    /// A session row with its ⋯ menu beside the status dot (shown on hover).
    @ViewBuilder
    private func sessionTreeRow(_ item: SidebarItem, archived: Bool = false, contextMenu: Bool = true) -> some View {
        let row = SessionTreeRow(item: item, selected: selection == item.id, height: shell.sessionRowHeight,
                                 onSelect: { selection = item.id }) {
            sessionMenu(item, archived: archived)
        }
        .accessibilityIdentifier("sidebar.item.\(item.id)")
        if contextMenu {
            row.contextMenu { sessionMenu(item, archived: archived) }
        } else {
            row
        }
    }

    /// First choose: a local folder, or a directory on an SSH host (2026-10-27).
    private var openFolderButton: some View {
        Menu {
            Button("打开本地项目…", systemImage: "folder", action: onOpenFolder)
                .accessibilityIdentifier("sidebar.open-local")
            Button("打开远程项目…", systemImage: "globe", action: onOpenRemote)
                .accessibilityIdentifier("sidebar.open-remote")
        } label: {
            Image(systemName: "folder.badge.plus")
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("打开项目…")
        .accessibilityLabel("打开项目")
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
            if let tagBook {
                Divider()
                Picker("标签", selection: $tagFilter) {
                    Text("不按标签").tag(String?.none)
                    ForEach(tagBook.tags) { tag in
                        Text(tag.projectId.map { "\(tag.name) · 仅 \(projectTitle($0) ?? "某项目")" } ?? tag.name)
                            .tag(Optional(tag.id))
                    }
                }
                .pickerStyle(.inline)
                Divider()
                Button("管理标签…") { managingTags = true }
            }
        } label: {
            let active = filter != .all || tagFilter != nil
            Image(systemName: active
                  ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease")
                .foregroundStyle(active ? AeroricColor.accent : AeroricColor.textTertiary)
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
private struct ProjectRow<MenuContent: View>: View {
    let item: SidebarItem
    @Environment(TagBook.self) private var tagBook: TagBook?
    let sessionCount: Int
    let expanded: Bool
    let selected: Bool
    let height: CGFloat
    let onSelect: () -> Void
    let onToggle: () -> Void
    let onNewTask: () -> Void
    @ViewBuilder let menu: () -> MenuContent
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
                        // S5: a chosen colour tints the outlined folder itself.
                        Image(systemName: expanded && item.color == nil ? "folder.fill" : "folder")
                            .font(.system(size: 13))
                            .foregroundStyle(ProjectColor.color(item.color) ?? AeroricColor.textSecondary)
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
                    TagChips(tags: tagBook?.tags(on: item.id) ?? [])
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
                Menu(content: menu) {
                    Image(systemName: "ellipsis")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 22, height: 22)
                        .background(Circle().fill(AeroricColor.hover))
                }
                .menuStyle(.button)
                .buttonStyle(.plain)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("更多")
                .accessibilityLabel("\(item.title) 的更多操作")
                .accessibilityIdentifier("sidebar.project-more.\(item.id)")
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
    var showsMore = false
    @Environment(TagBook.self) private var tagBook: TagBook?

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            HarnessLogo(harness: item.harness, agent: item.agent, size: 14)
            Text(item.title)
                .font(.system(size: AeroricType.body))
                .lineLimit(1)
                .truncationMode(.tail)
            // T2: tags in full after the title; the title truncates first.
            TagChips(tags: tagBook?.tags(on: item.id) ?? [])
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
            if showsMore {
                // The ⋯ button is laid over this slot (SessionTreeRow).
                Color.clear.frame(width: 20, height: 20)
            } else if let updatedAt = item.updatedAt, !item.imported {
                // An imported row leaves the date out (2026-10-28): that room is for its tags.
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
/// A session row in the tree: the row itself selects; on hover its ⋯ menu
/// sits just left of the status dot (outside the row's button, so a click on
/// it never also opens the session).
private struct SessionTreeRow<MenuContent: View>: View {
    let item: SidebarItem
    let selected: Bool
    let height: CGFloat
    let onSelect: () -> Void
    @ViewBuilder let menu: () -> MenuContent
    @State private var hovered = false

    var body: some View {
        HoverCapsuleRow(selected: selected, height: height, action: onSelect) {
            SessionRow(item: item, showsMore: hovered)
        }
        .overlay(alignment: .trailing) {
            if hovered {
                Menu(content: menu) {
                    Image(systemName: "ellipsis")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 20, height: 20)
                        .background(Circle().fill(AeroricColor.hover))
                }
                .menuStyle(.button)
                .buttonStyle(.plain)
                .menuIndicator(.hidden)
                .fixedSize()
                .padding(.trailing, AeroricSpace.m + 6 + AeroricSpace.s)
                .help("更多")
                .accessibilityLabel("\(item.title) 的更多操作")
                .accessibilityIdentifier("sidebar.session-more.\(item.id)")
            }
        }
        .onHover { hovered = $0 }
    }
}

/// The harness's own logo, borderless and tinted like text (S9); an agent
/// without a known harness keeps its symbol.
struct HarnessLogo: View {
    let harness: String?
    let agent: String?
    var size: CGFloat = 14

    static func asset(harness: String?, agent: String?) -> String? {
        let family = (harness ?? agent)?.lowercased()
        switch family {
        case "claude", "claude_gpt55": return "HarnessClaude"
        case "codex": return "HarnessCodex"
        case "omp": return "HarnessOMP"
        case "dsh": return "HarnessDSH"
        case "qoder": return "HarnessQoder"
        case "cursor": return "HarnessCursor"
        case "opencode": return "HarnessOpenCode"
        default: return nil
        }
    }

    var body: some View {
        if let asset = Self.asset(harness: harness, agent: agent) {
            Image(asset)
                .renderingMode(.template)
                .resizable()
                .scaledToFit()
                .frame(width: size, height: size)
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityLabel(AgentGlyph.displayName(for: harness ?? agent))
        } else {
            AgentGlyph(agent: agent, size: size)
        }
    }
}

/// The folder tints 修改颜色 offers (S5).
struct ProjectColor: Identifiable {
    let id: String
    let title: String
    let color: Color

    static let all: [ProjectColor] = [
        ProjectColor(id: "red", title: "红", color: Color(red: 0.89, green: 0.29, blue: 0.29)),
        ProjectColor(id: "orange", title: "橙", color: Color(red: 0.85, green: 0.45, blue: 0.19)),
        ProjectColor(id: "yellow", title: "黄", color: Color(red: 0.84, green: 0.65, blue: 0.13)),
        ProjectColor(id: "green", title: "绿", color: Color(red: 0.39, green: 0.60, blue: 0.13)),
        ProjectColor(id: "teal", title: "青", color: Color(red: 0.11, green: 0.62, blue: 0.46)),
        ProjectColor(id: "blue", title: "蓝", color: Color(red: 0.22, green: 0.54, blue: 0.87)),
        ProjectColor(id: "purple", title: "紫", color: Color(red: 0.50, green: 0.47, blue: 0.87)),
        ProjectColor(id: "pink", title: "粉", color: Color(red: 0.83, green: 0.33, blue: 0.49)),
    ]

    static func color(_ id: String?) -> Color? {
        all.first { $0.id == id }?.color
    }
}

/// 会话管理…: every session of one project, archived ones included, with
/// bulk 归档 / 取消归档 / 移除 (S4).
struct SessionManagerSheet: View {
    let project: SidebarItem
    let sessions: [SidebarItem]
    let onAction: (String, SessionRowAction) -> Void
    let onClose: () -> Void
    @State private var chosen: Set<String> = []
    @State private var harness = "all"
    @State private var confirmingRemove = false

    private var shown: [SidebarItem] {
        sessions.filter { harness == "all" || ($0.harness ?? $0.agent) == harness }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            HStack {
                Label("\(project.title) 的会话", systemImage: "list.bullet.rectangle")
                    .font(.system(size: AeroricType.headline, weight: .semibold))
                Spacer()
                CapsuleSlider(selection: $harness, groupTitle: "harness", identifier: "sessions.manage.harness",
                              segments: [.init(value: "all", title: "全部")]
                                + ["claude", "codex", "omp", "dsh"]
                                .filter { id in sessions.contains { ($0.harness ?? $0.agent) == id } }
                                .map { .init(value: $0, title: AgentGlyph.displayName(for: $0)) })
                    .fixedSize()
            }
            List(shown) { item in
                HStack(spacing: AeroricSpace.s) {
                    Toggle("", isOn: Binding(get: { chosen.contains(item.id) },
                                             set: { if $0 { chosen.insert(item.id) } else { chosen.remove(item.id) } }))
                        .labelsHidden()
                    HarnessLogo(harness: item.harness, agent: item.agent, size: 14)
                    Text(item.title).lineLimit(1)
                    Spacer()
                    if item.group == .archived {
                        Text("已归档").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                    if item.imported {
                        Text("已导入").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                    SessionStatusDot(status: item.status)
                }
            }
            .frame(minHeight: 280)
            .clipShape(AeroricRadius.shape(16))
            HStack {
                Button(chosen.count == shown.count && !shown.isEmpty ? "全不选" : "全选") {
                    chosen = chosen.count == shown.count ? [] : Set(shown.map(\.id))
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Text("已选 \(chosen.count) 个").font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                Spacer()
                Button("归档") { apply(.archive) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Button("取消归档") { apply(.unarchive) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Button("移除…") { confirmingRemove = true }
                    .buttonStyle(AeroricCapsuleButtonStyle(.destructive, compact: true))
                Button("完成", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 620)
        .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
        .confirmationDialog("移除选中的 \(chosen.count) 个会话？", isPresented: $confirmingRemove) {
            Button("移除", role: .destructive) { apply(.remove) }
            Button("取消", role: .cancel) {}
        } message: {
            Text("只从 Aeroric 的列表里移除，Agent 自己的会话文件不会被删除；已置顶和运行中的会话会保留。")
        }
    }

    private func apply(_ action: SessionRowAction) {
        for id in chosen { onAction(id, action) }
        chosen = []
    }
}

/// `StatusDot`; the label is the accessibility value and tooltip.
public struct SessionStatusDot: View {
    let status: String?
    public init(status: String?) { self.status = status }

    /// 2026-10-28: one quiet glyph per state instead of the same flat dot —
    /// a spinning ring while it runs, a soft halo when it needs you or failed,
    /// a small check when done, a hollow ring otherwise.
    public var body: some View {
        Group {
            switch status {
            case "running", "detached":
                StatusDot(ActivityState(wireStatus: status), label: Self.label(status))
            case "input_required":
                halo(AeroricColor.warning)
            case "failed":
                halo(AeroricColor.danger)
            case "done":
                Image(systemName: "checkmark")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(AeroricColor.success.opacity(0.85))
            case "cancelled":
                Image(systemName: "minus")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(AeroricColor.textTertiary)
            default:
                Circle()
                    .strokeBorder(AeroricColor.textTertiary.opacity(0.7), lineWidth: 1.2)
                    .frame(width: 7, height: 7)
            }
        }
        .frame(width: 12, height: 12)
        .help(Self.label(status))
        .accessibilityElement()
        .accessibilityLabel(Self.label(status))
    }

    private func halo(_ color: Color) -> some View {
        ZStack {
            Circle().fill(color.opacity(0.2)).frame(width: 11, height: 11)
            Circle().fill(color).frame(width: 5.5, height: 5.5)
        }
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
