import SwiftUI

/// 任务列表：窗口局部过滤（filterText）作用于可见集；宿主 archive
/// 是另一层（权威），两层边界在 TaskStore 的注释里钉死。
///
/// 项目作用域由调用方算好（`TaskStore.visibleTasks(inProject:)`）——
/// 侧栏的「项目」组才是项目选择器，`ProjectListView`（一个只渲染项目名的
/// 独立列表）在导航重做后已无调用点，随本轮删除。
public struct TaskListView: View {
    @Binding var visibleTasks: [TaskStore.TaskEntry]
    @Binding var filterText: String
    @Binding var selectedTaskID: String?
    var onOpenDraft: (() -> Void)?

    public init(
        visibleTasks: Binding<[TaskStore.TaskEntry]>,
        filterText: Binding<String>,
        selectedTaskID: Binding<String?> = .constant(nil),
        onOpenDraft: (() -> Void)? = nil
    ) {
        self._visibleTasks = visibleTasks
        self._filterText = filterText
        self._selectedTaskID = selectedTaskID
        self.onOpenDraft = onOpenDraft
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("过滤任务…", text: $filterText)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("tasks.filter")
                Button("新建任务") { onOpenDraft?() }
                    .accessibilityIdentifier("tasks.new")
            }
            .padding(8)
            if visibleTasks.isEmpty {
                // 空列表必须说清"为什么空"，否则与渲染失败无法区分
                // （参考实现里没有一个是空白面板：opencode "Nothing here
                // yet"/"Create a session to get started"、deepseek "No
                // sessions yet"/"No matches"）。两种空法文案不同：有过滤词
                // ⇒ 是过滤没命中；没有过滤词 ⇒ 这个项目还没有任务。
                PaneEmptyState(
                    systemImage: hasFilter ? "magnifyingglass" : "tray",
                    title: hasFilter ? "没有匹配的任务" : "还没有任务",
                    hint: hasFilter ? "换个关键词，或清空过滤条件" : "点右上角「新建任务」开始一个"
                )
                .accessibilityIdentifier("tasks.empty")
            } else {
                List(selection: $selectedTaskID) {
                    ForEach(visibleTasks) { task in
                        TaskRow(task: task)
                            .tag(Optional(task.id))
                    }
                }
                .listStyle(.plain)
                .accessibilityIdentifier("tasks.list")
            }
        }
    }

    private var hasFilter: Bool {
        !filterText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

/// 单行：状态点 + 标题 + 项目 · Agent · 相对时间（全部来自快照）。
public struct TaskRow: View {
    let task: TaskStore.TaskEntry
    let projectName: String?

    public init(task: TaskStore.TaskEntry, projectName: String? = nil) {
        self.task = task
        self.projectName = projectName
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.m) {
            StatusDot(ActivityState(wireStatus: task.status), label: task.status)
            VStack(alignment: .leading, spacing: 2) {
                Text(task.title)
                    .font(.system(size: AeroricType.body))
                    .lineLimit(1)
                TaskMetaLine(task: task, projectName: projectName)
            }
            Spacer()
            if task.starred {
                Image(systemName: "star.fill")
                    .font(.system(size: 10))
                    .foregroundStyle(AeroricColor.warning)
            }
            if task.archived {
                Text("已归档")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .padding(.vertical, AeroricSpace.xs)
        .accessibilityIdentifier("task.row.\(task.id)")
    }
}
