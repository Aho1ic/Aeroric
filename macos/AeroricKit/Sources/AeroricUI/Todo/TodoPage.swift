import AeroricProtocol
import SwiftUI

/// 2026-11-15 工作安排第一批: the 待办 page — what the user wants done in a
/// project, handed to an Agent by hand and accepted by the user. Plain
/// capsule buttons, no `Menu` (a `Menu` does not take accessibility clicks
/// while the App is in the background).
///
/// 第二批: Agent proposals on top (收下 / 不要 / 来自会话); each row shows its
/// 验收检查 (检查中 / 检查通过 / 检查没过, the output, 重新检查). While a
/// session works or a check runs, the page looks again every few seconds.
public struct TodoPage: View {
    @Bindable var store: TodoStore
    let projects: [(id: String, name: String)]
    let agents: [AgentChoice]
    let onOpenTask: (String) -> Void
    @State private var editing: LiveTodo?
    @State private var assigning: LiveTodo?
    @State private var deleting: LiveTodo?
    @State private var showClosed = false
    /// Rows whose check output is open.
    @State private var showOutput: Set<String> = []

    public init(store: TodoStore, projects: [(id: String, name: String)], agents: [AgentChoice],
                onOpenTask: @escaping (String) -> Void) {
        self.store = store
        self.projects = projects
        self.agents = agents
        self.onOpenTask = onOpenTask
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text(TodoText.intro)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(2)
                Spacer(minLength: AeroricSpace.m)
                Button {
                    editing = LiveTodo(projectId: projects.first?.id ?? "")
                } label: {
                    Label("新建", systemImage: "plus")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                .fixedSize()
                .disabled(projects.isEmpty)
                .accessibilityIdentifier("todo.new")
            }
            if let error = store.errorText {
                CapsuleErrorLine(error, identifier: "todo.error") { store.errorText = nil }
            }
            if store.todos.isEmpty {
                ContentUnavailableView("还没有待办", systemImage: "checkmark.circle",
                                       description: Text(projects.isEmpty ? "先打开一个项目。" : "比如“给数据加载加单元测试”“把训练脚本的学习率改成余弦退火”。"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.s) {
                        if !store.proposals.isEmpty {
                            Text("Agent 提议（\(store.proposals.count)）")
                                .font(.system(size: AeroricType.secondary, weight: .medium))
                                .foregroundStyle(AeroricColor.textSecondary)
                                .accessibilityIdentifier("todo.proposals")
                            Text(TodoText.proposalsIntro)
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                            ForEach(store.proposals) { todo in proposalRow(todo) }
                            if !store.open.isEmpty {
                                Text("在办")
                                    .font(.system(size: AeroricType.secondary, weight: .medium))
                                    .foregroundStyle(AeroricColor.textSecondary)
                                    .padding(.top, AeroricSpace.s)
                            }
                        }
                        ForEach(store.open) { todo in row(todo) }
                        if !store.closed.isEmpty {
                            Button(showClosed ? "收起已完成和已取消" : "已完成和已取消（\(store.closed.count)）") {
                                showClosed.toggle()
                            }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .fixedSize()
                            .padding(.top, AeroricSpace.s)
                            .accessibilityIdentifier("todo.closed")
                            if showClosed {
                                ForEach(store.closed) { todo in row(todo) }
                            }
                        }
                    }
                }
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: NativePanelGeometry.mainShell.readingColumnWidth, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        .windowTitle(WindowTitle("待办", subtitle: store.open.isEmpty ? nil : "\(store.open.count) 条在办",
                                 systemImage: "checkmark.circle"))
        .task {
            // Sooner while something moves; `.task` stops when the page goes.
            while !Task.isCancelled {
                await store.reload()
                try? await Task.sleep(for: .seconds(store.inFlight ? 5 : 30))
            }
        }
        .sheet(item: $editing) { todo in
            TodoEditorSheet(store: store, todo: todo, projects: projects) { editing = nil }
        }
        .sheet(item: $assigning) { todo in
            TodoAssignSheet(store: store, todo: todo, agents: agents, onOpenTask: onOpenTask) { assigning = nil }
        }
        .confirmationDialog("删除这个待办？", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                            presenting: deleting) { todo in
            Button("删除“\(todo.title)”（它开过的会话会留着）", role: .destructive) { Task { await store.delete(todo) } }
        }
        .accessibilityIdentifier("todo.page")
    }

    private func projectName(_ id: String) -> String {
        projects.first { $0.id == id }?.name ?? "项目已移除"
    }

    private func agentName(_ id: String) -> String {
        agents.first { $0.id == id }?.title ?? id
    }

    private func tone(_ todo: LiveTodo) -> Color {
        if todo.isProposed || todo.progress == "review" || todo.progress == "waiting" { return AeroricColor.accent }
        if TodoText.needsRedo(todo) { return AeroricColor.warning }
        return AeroricColor.textSecondary
    }

    private func checkTone(_ todo: LiveTodo) -> Color {
        switch todo.lastCheck?.state {
        case "passed": return AeroricColor.success
        case "failed": return AeroricColor.warning
        default: return AeroricColor.textSecondary
        }
    }

    private func pill(_ text: String, _ color: Color, _ id: String, help: String = "") -> some View {
        Text(text)
            .font(.system(size: AeroricType.caption, weight: .medium))
            .foregroundStyle(color)
            .padding(.horizontal, AeroricSpace.m)
            .padding(.vertical, 3)
            .background(Capsule(style: .continuous).fill(color.opacity(0.12)))
            .help(help)
            .fixedSize()
            .accessibilityIdentifier(id)
    }

    /// An Agent's proposal: take it, drop it, or see where it came from.
    @ViewBuilder private func proposalRow(_ todo: LiveTodo) -> some View {
        let busy = store.busy == todo.id
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(todo.title)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .lineLimit(2)
                    Text(projectName(todo.projectId))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: AeroricSpace.m)
                pill(TodoText.status(todo), tone(todo), "todo.status.\(todo.id)")
            }
            if !todo.notes.isEmpty {
                Text(todo.notes)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(3)
            }
            HStack(spacing: AeroricSpace.s) {
                capsule(busy ? "更新中…" : "收下", .primary, "todo.take.\(todo.id)") {
                    Task { await store.decide(todo, "take") }
                }
                capsule("不要", .ghost, "todo.drop.\(todo.id)") { Task { await store.delete(todo) } }
                if let task = todo.proposedBy {
                    capsule("来自会话", .ghost, "todo.source.\(todo.id)") { onOpenTask(task) }
                }
                Spacer(minLength: 0)
                Button { editing = todo } label: { Image(systemName: "pencil") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .help("编辑")
                    .fixedSize()
                    .accessibilityIdentifier("todo.edit.\(todo.id)")
            }
            .disabled(busy)
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.layer1))
        .accessibilityIdentifier("todo.row.\(todo.id)")
    }

    /// The 验收检查 under a row: how it went, how often it went back, the output.
    @ViewBuilder private func checkLine(_ todo: LiveTodo) -> some View {
        if let words = TodoText.checkStatus(todo), let run = todo.lastCheck {
            let open = showOutput.contains(todo.id)
            HStack(spacing: AeroricSpace.s) {
                pill(words, checkTone(todo), "todo.checkStatus.\(todo.id)", help: todo.check)
                if let fixes = TodoText.fixes(todo) {
                    Text(fixes)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1)
                }
                if run.state != "running", !run.output.isEmpty {
                    capsule(open ? "收起输出" : "看输出", .ghost, "todo.output.\(todo.id)") {
                        if open { showOutput.remove(todo.id) } else { showOutput.insert(todo.id) }
                    }
                }
                Spacer(minLength: 0)
            }
            if open, run.state != "running" {
                ScrollView {
                    Text(run.output)
                        .font(.system(size: AeroricType.caption, design: .monospaced))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 160)
                .padding(AeroricSpace.m)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer2))
                .accessibilityIdentifier("todo.outputText.\(todo.id)")
            }
        }
    }

    @ViewBuilder private func row(_ todo: LiveTodo) -> some View {
        let busy = store.busy == todo.id
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(todo.title)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .strikethrough(todo.state == "cancelled")
                        .lineLimit(2)
                    Text([projectName(todo.projectId), TodoText.assignments(todo, agentName: agentName)]
                        .compactMap { $0 }.joined(separator: " · "))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: AeroricSpace.m)
                pill(TodoText.status(todo), tone(todo), "todo.status.\(todo.id)", help: todo.progressDetail ?? "")
            }
            if !todo.notes.isEmpty {
                Text(todo.notes)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(2)
            }
            if let detail = todo.progressDetail, TodoText.needsRedo(todo) {
                Text(detail)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
                    .lineLimit(2)
            }
            checkLine(todo)
            HStack(spacing: AeroricSpace.s) {
                actions(todo, busy: busy)
                Spacer(minLength: 0)
                Button { editing = todo } label: { Image(systemName: "pencil") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .help("编辑")
                    .fixedSize()
                    .accessibilityIdentifier("todo.edit.\(todo.id)")
                Button { deleting = todo } label: { Image(systemName: "trash") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .help("删除")
                    .fixedSize()
                    .accessibilityIdentifier("todo.delete.\(todo.id)")
            }
            .disabled(busy)
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.layer1))
        .accessibilityIdentifier("todo.row.\(todo.id)")
    }

    @ViewBuilder private func actions(_ todo: LiveTodo, busy: Bool) -> some View {
        switch todo.state {
        case "open":
            capsule("交办…", .tinted, "todo.assign.\(todo.id)") { assigning = todo }
            capsule("取消", .ghost, "todo.cancel.\(todo.id)") { Task { await store.decide(todo, "cancel") } }
        case "assigned":
            if todo.progress == "review" {
                capsule(busy ? "更新中…" : "验收通过", .primary, "todo.accept.\(todo.id)") {
                    Task { await store.decide(todo, "accept") }
                }
                if !todo.check.isEmpty {
                    capsule(todo.lastCheck == nil ? "运行检查" : "重新检查", .ghost, "todo.check.\(todo.id)") {
                        Task { await store.recheck(todo) }
                    }
                    .disabled(todo.lastCheck?.state == "running")
                }
            }
            capsule("重新交办…", TodoText.needsRedo(todo) ? .tinted : .ghost, "todo.redo.\(todo.id)") { assigning = todo }
            if let task = todo.latestTaskId, todo.progress != "missing" {
                capsule("打开会话", .ghost, "todo.open.\(todo.id)") { onOpenTask(task) }
            }
            capsule("取消", .ghost, "todo.cancel.\(todo.id)") { Task { await store.decide(todo, "cancel") } }
        default:
            capsule("重新打开", .ghost, "todo.reopen.\(todo.id)") { Task { await store.decide(todo, "reopen") } }
            if let task = todo.latestTaskId {
                capsule("打开会话", .ghost, "todo.open.\(todo.id)") { onOpenTask(task) }
            }
        }
    }

    private func capsule(_ title: String, _ kind: AeroricCapsuleButtonStyle.Kind, _ id: String,
                         action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(AeroricCapsuleButtonStyle(kind, compact: true))
            .fixedSize()
            .accessibilityIdentifier(id)
    }
}

/// 新建 / 编辑 a todo: its words, and (new only) its project.
struct TodoEditorSheet: View {
    let store: TodoStore
    let projects: [(id: String, name: String)]
    let onClose: () -> Void
    @State private var draft: LiveTodo

    init(store: TodoStore, todo: LiveTodo, projects: [(id: String, name: String)], onClose: @escaping () -> Void) {
        self.store = store
        self.projects = projects
        self.onClose = onClose
        _draft = State(initialValue: todo)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text(draft.id.isEmpty ? "新建待办" : draft.isProposed ? "编辑提议" : "编辑待办")
                .font(.system(size: AeroricType.title, weight: .semibold))
            CapsuleField(placeholder: "要做什么，比如：给数据加载加单元测试", text: $draft.title)
                .accessibilityIdentifier("todo.edit.title")
            if draft.id.isEmpty {
                HStack(spacing: AeroricSpace.m) {
                    Text("项目")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 64, alignment: .leading)
                    CapsulePicker(title: projects.first { $0.id == draft.projectId }?.name ?? "选一个项目",
                                  systemImage: "folder") {
                        ForEach(projects, id: \.id) { project in
                            Button(project.name) { draft.projectId = project.id }
                        }
                    }
                    Spacer(minLength: 0)
                }
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                Text("说明（可选）：背景、要改的地方、怎么算做完")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                TextEditor(text: $draft.notes)
                    .font(.system(size: AeroricType.body))
                    .scrollIndicators(.never)
                    .scrollContentBackground(.hidden)
                    .padding(AeroricSpace.m)
                    .frame(height: 120)
                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                    .accessibilityIdentifier("todo.edit.notes")
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                CapsuleField(systemImage: "checkmark.seal", placeholder: "可选的验收检查，例如 cargo test -p auth 或 pytest tests/test_loader.py",
                             text: $draft.check)
                    .accessibilityIdentifier("todo.edit.check")
                Toggle("检查没过时让 Agent 接着修（最多 \(TodoText.fixRounds) 次）", isOn: $draft.fixOnFail)
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .disabled(draft.check.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("todo.edit.fix")
                Text("会话这一轮结束后，在项目目录里用你的登录 shell 运行，10 分钟内退出码为 0 算通过，结果显示在待办上。通过了也要你点“验收通过”。打开“接着修”后，没过时把输出发回原来的会话让 Agent 修，修完再检查。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error = store.errorText {
                CapsuleErrorLine(error, identifier: "todo.edit.error") { store.errorText = nil }
            }
            HStack {
                Spacer()
                Button("取消", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    Task {
                        store.errorText = nil
                        if await store.save(draft) { onClose() }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .disabled(draft.title.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("todo.edit.save")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520)
    }
}

/// 交办: pick the Agent; the session gets the todo as its prompt, shown here.
struct TodoAssignSheet: View {
    let store: TodoStore
    let todo: LiveTodo
    let agents: [AgentChoice]
    let onOpenTask: (String) -> Void
    let onClose: () -> Void
    @State private var agent: String

    init(store: TodoStore, todo: LiveTodo, agents: [AgentChoice], onOpenTask: @escaping (String) -> Void,
         onClose: @escaping () -> Void) {
        self.store = store
        self.todo = todo
        self.agents = agents
        self.onOpenTask = onOpenTask
        self.onClose = onClose
        let last = todo.assignments.last?.agent
        _agent = State(initialValue: agents.first { $0.id == last }?.id ?? agents.first?.id ?? "")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text(todo.assignments.isEmpty ? "交办给 Agent" : "重新交办")
                .font(.system(size: AeroricType.title, weight: .semibold))
            HStack(spacing: AeroricSpace.m) {
                Text("Agent")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(width: 64, alignment: .leading)
                CapsulePicker(title: agents.first { $0.id == agent }?.title ?? "选一个 Agent", systemImage: "cpu") {
                    ForEach(agents) { choice in
                        Button(choice.title) { agent = choice.id }
                    }
                }
                Spacer(minLength: 0)
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                Text("会开一个新会话，名字是“\(todo.title)（待办）”，发出去的消息：")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                Text(TodoText.prompt(todo))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .padding(AeroricSpace.m)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                Text(todo.check.isEmpty
                     ? "会话用这个 Agent 的默认权限模式和模型。会话结束后待办变成“待验收”，你点“验收通过”才算完成。"
                     : "会话用这个 Agent 的默认权限模式和模型。这一轮结束后会运行验收检查；待办变成“待验收”，你点“验收通过”才算完成。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error = store.errorText {
                CapsuleErrorLine(error, identifier: "todo.assign.error") { store.errorText = nil }
            }
            HStack {
                Spacer()
                Button("取消", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button(store.busy == todo.id ? "交办中…" : "交办") {
                    Task {
                        store.errorText = nil
                        if let task = await store.assign(todo, agent: agent) {
                            onClose()
                            onOpenTask(task)
                        }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .disabled(agent.isEmpty || store.busy != nil)
                .accessibilityIdentifier("todo.assign.go")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520)
    }
}
