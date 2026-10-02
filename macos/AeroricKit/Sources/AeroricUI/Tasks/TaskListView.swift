import SwiftUI

/// P5-03 Action 1/2：项目列表（侧栏）＋任务列表。列表只渲染 store 的
/// 镜像；时间/状态/agent 文案来自宿主快照，客户端不推断。
public struct ProjectListView: View {
    @Binding var projects: [TaskStore.ProjectEntry]
    @Binding var selectedProjectID: String?

    public init(projects: Binding<[TaskStore.ProjectEntry]>, selectedProjectID: Binding<String?>) {
        self._projects = projects
        self._selectedProjectID = selectedProjectID
    }

    public var body: some View {
        List(selection: $selectedProjectID) {
            ForEach(projects) { project in
                Text(project.name)
                    .tag(Optional(project.id))
                    .accessibilityIdentifier("project.item.\(project.id)")
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .accessibilityIdentifier("project.list")
    }
}

/// 任务列表：窗口局部过滤（filterText）作用于可见集；宿主 archive
/// 是另一层（权威），两层边界在 TaskStore 的注释里钉死。
public struct TaskListView: View {
    @Binding var visibleTasks: [TaskStore.TaskEntry]
    @Binding var filterText: String
    var onOpenDraft: (() -> Void)?

    public init(
        visibleTasks: Binding<[TaskStore.TaskEntry]>,
        filterText: Binding<String>,
        onOpenDraft: (() -> Void)? = nil
    ) {
        self._visibleTasks = visibleTasks
        self._filterText = filterText
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
            List {
                ForEach(visibleTasks) { task in
                    TaskRow(task: task)
                }
            }
            .listStyle(.plain)
            .accessibilityIdentifier("tasks.list")
        }
    }
}

/// 单行：状态/时间/agent 来自快照。
public struct TaskRow: View {
    let task: TaskStore.TaskEntry

    public init(task: TaskStore.TaskEntry) {
        self.task = task
    }

    public var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(TaskRow.statusColor(task.status))
                .frame(width: 7, height: 7)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(task.title)
                    .lineLimit(1)
                Text("\(task.status) · \(task.agent ?? "—")")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
            }
            Spacer()
            if task.starred {
                Image(systemName: "star.fill")
                    .font(.caption)
                    .foregroundStyle(Color.dsWarning)
            }
            if task.archived {
                Text("已归档")
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
            }
        }
        .accessibilityIdentifier("task.row.\(task.id)")
    }

    static func statusColor(_ status: String) -> Color {
        switch status {
        case "running": Color.dsSuccess
        case "input_required": Color.dsWarning
        case "failed", "cancelled": Color.dsDanger
        case "done": Color.dsTextMuted
        default: Color.dsTextSecondary
        }
    }
}
