import SwiftUI

/// Board or list of the current tasks (orca task UI, D1). Everything shown
/// comes from the task snapshot; actions are injected and only use what the
/// host already offers (open, cancel, star, archive). Resume is "即将支持" (D3).
public struct TaskBoardView: View {
    public enum Mode: String, CaseIterable, Identifiable, Sendable {
        case board, list
        public var id: String { rawValue }
    }

    let tasks: [TaskStore.TaskEntry]
    @Binding var filterText: String
    @Binding var selectedTaskID: String?
    let projectNames: [String: String]
    let onOpenDraft: () -> Void
    let onCancel: (String) -> Void
    let onStar: (String) -> Void
    let onArchive: (String) -> Void
    @AppStorage("tasks.viewMode") private var modeRawValue = Mode.board.rawValue
    @State private var cancelling: TaskStore.TaskEntry?

    public init(tasks: [TaskStore.TaskEntry], filterText: Binding<String>, selectedTaskID: Binding<String?>,
                projectNames: [String: String], onOpenDraft: @escaping () -> Void,
                onCancel: @escaping (String) -> Void, onStar: @escaping (String) -> Void,
                onArchive: @escaping (String) -> Void) {
        self.tasks = tasks
        _filterText = filterText
        _selectedTaskID = selectedTaskID
        self.projectNames = projectNames
        self.onOpenDraft = onOpenDraft
        self.onCancel = onCancel
        self.onStar = onStar
        self.onArchive = onArchive
    }

    private var mode: Mode { Mode(rawValue: modeRawValue) ?? .board }
    private var hasFilter: Bool { !filterText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    public var body: some View {
        VStack(spacing: 0) {
            toolbar
            HairlineDivider()
            if tasks.filter({ !$0.archived }).isEmpty {
                AeroricEmptyState(
                    systemImage: hasFilter ? "magnifyingglass" : "tray",
                    title: hasFilter ? "没有匹配的任务" : "还没有任务",
                    message: hasFilter ? "换个关键词，或清空过滤条件" : "从“新会话”开始一个任务"
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("tasks.empty")
            } else if mode == .board {
                board
            } else {
                list
            }
        }
        .background(AeroricColor.canvas)
        .confirmationDialog("停止这个任务？", isPresented: Binding(
            get: { cancelling != nil }, set: { if !$0 { cancelling = nil } }
        ), presenting: cancelling) { task in
            Button("停止任务", role: .destructive) { onCancel(task.id) }
            Button("取消", role: .cancel) {}
        } message: { task in
            Text("「\(task.title)」会结束当前运行；会话记录保留。")
        }
        .accessibilityIdentifier("tasks.board")
    }

    private var toolbar: some View {
        HStack(spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "magnifyingglass").foregroundStyle(AeroricColor.textTertiary)
                TextField("过滤任务…", text: $filterText)
                    .textFieldStyle(.plain)
                    .accessibilityIdentifier("tasks.filter")
            }
            .font(.system(size: AeroricType.body))
            .padding(.horizontal, AeroricSpace.l)
            .frame(width: 260, height: 28)
            .background(AeroricColor.layer1, in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            CapsuleSlider(selection: $modeRawValue, groupTitle: "视图", identifier: "tasks.view-mode", segments: [
                .init(value: Mode.board.rawValue, title: "看板", systemImage: "rectangle.split.3x1"),
                .init(value: Mode.list.rawValue, title: "列表", systemImage: "list.bullet"),
            ])
            .fixedSize()
            Spacer()
            Button(action: onOpenDraft) {
                Label("新会话", systemImage: "square.and.pencil")
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
            .accessibilityIdentifier("tasks.new")
        }
        .padding(.horizontal, AeroricSpace.xl)
        .frame(height: 48)
    }

    private var board: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: AeroricSpace.l) {
                ForEach(TaskBoard.columns(tasks)) { column in
                    BoardColumn(column: column, selectedTaskID: selectedTaskID, projectNames: projectNames,
                                onOpen: { selectedTaskID = $0 }, menu: menu)
                }
            }
            .padding(AeroricSpace.xl)
            .frame(maxHeight: .infinity, alignment: .top)
        }
    }

    private var list: some View {
        List(selection: $selectedTaskID) {
            ForEach(TaskBoard.sorted(tasks.filter { !$0.archived })) { task in
                TaskRow(task: task, projectName: projectNames[task.projectID])
                    .tag(Optional(task.id))
                    .contextMenu { menu(task) }
            }
        }
        .listStyle(.inset)
        .scrollContentBackground(.hidden)
        .accessibilityIdentifier("tasks.list")
    }

    @ViewBuilder private func menu(_ task: TaskStore.TaskEntry) -> some View {
        Button("打开", systemImage: "arrow.up.forward.app") { selectedTaskID = task.id }
        if TaskBoardColumn(status: task.status).allowsCancel {
            Button("停止任务…", systemImage: "stop.circle") { cancelling = task }
        }
        Button("恢复（即将支持）", systemImage: "arrow.clockwise") {}.disabled(true)
        Divider()
        Button(task.starred ? "取消星标" : "加星标", systemImage: task.starred ? "star.slash" : "star") { onStar(task.id) }
        Button(task.archived ? "取消归档" : "归档", systemImage: "archivebox") { onArchive(task.id) }
    }
}

private struct BoardColumn<Menu: View>: View {
    let column: TaskBoard.Column
    let selectedTaskID: String?
    let projectNames: [String: String]
    let onOpen: (String) -> Void
    let menu: (TaskStore.TaskEntry) -> Menu

    static var width: CGFloat { 288 }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.s) {
                Text(column.id.title).font(.system(size: AeroricType.body, weight: .semibold))
                Text("\(column.tasks.count)")
                    .font(.system(size: AeroricType.caption, weight: .medium))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.horizontal, AeroricSpace.s)
                    .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                Spacer()
            }
            .padding(.horizontal, AeroricSpace.xs)
            ScrollView(.vertical) {
                LazyVStack(spacing: AeroricSpace.m) {
                    if column.tasks.isEmpty {
                        Text(column.id.emptyText)
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .frame(maxWidth: .infinity, minHeight: 64)
                            .background(AeroricRadius.shape(AeroricRadius.m)
                                .strokeBorder(AeroricColor.separator2, style: StrokeStyle(lineWidth: 1, dash: [4, 4])))
                    }
                    ForEach(column.tasks) { task in
                        TaskCard(task: task, projectName: projectNames[task.projectID],
                                 selected: task.id == selectedTaskID) { onOpen(task.id) }
                            .contextMenu { menu(task) }
                    }
                }
            }
        }
        .frame(width: Self.width)
        .frame(maxHeight: .infinity, alignment: .top)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("tasks.board.column.\(column.id.rawValue)")
    }
}

/// Status dot + title; project tint + name · Agent · relative time.
struct TaskCard: View {
    let task: TaskStore.TaskEntry
    let projectName: String?
    let selected: Bool
    let onOpen: () -> Void
    @State private var hovered = false

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.m)
        Button(action: onOpen) {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                    StatusDot(ActivityState(wireStatus: task.status), label: task.status)
                    Text(task.title)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Spacer(minLength: 0)
                    if task.starred {
                        Image(systemName: "star.fill")
                            .font(.system(size: 10))
                            .foregroundStyle(AeroricColor.warning)
                    }
                }
                TaskMetaLine(task: task, projectName: projectName)
            }
            .padding(AeroricSpace.l)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(selected ? AeroricColor.selection : (hovered ? AeroricColor.hover : AeroricColor.layer1), in: shape)
            .overlay { shape.strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("tasks.card.\(task.id)")
    }
}

/// Project tint dot + name · Agent glyph · relative time.
struct TaskMetaLine: View {
    let task: TaskStore.TaskEntry
    let projectName: String?

    static let tints: [Color] = [
        Color(red: 0.42, green: 0.56, blue: 0.86), Color(red: 0.36, green: 0.68, blue: 0.56),
        Color(red: 0.80, green: 0.58, blue: 0.36), Color(red: 0.66, green: 0.50, blue: 0.82),
        Color(red: 0.82, green: 0.46, blue: 0.52), Color(red: 0.46, green: 0.66, blue: 0.74),
    ]

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            Circle().fill(Self.tints[ProjectTint.index(for: task.projectID)]).frame(width: 7, height: 7)
            Text(projectName ?? "未知项目").lineLimit(1)
            Text("·")
            AgentGlyph(agent: task.agent, size: 12)
            if let date = TaskBoard.date(of: task) {
                Text("·")
                Text(RelativeTime.short(date))
            }
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.textTertiary)
    }
}
