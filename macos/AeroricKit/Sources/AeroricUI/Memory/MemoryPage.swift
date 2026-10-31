import SwiftUI
import AeroricProtocol

/// B5-1 (2026-10-31): the "记忆" page in the sidebar (Q-M7 b): a scope
/// (global or a project), five tabs, capsule rows. Proposals wait on top;
/// old versions and the archive fold under 历史版本.
public struct MemoryPage: View {
    @Bindable var store: MemoryStore
    let projects: [(id: String, name: String)]
    @State private var draft = ""
    @State private var category = "preference"
    @State private var showHistory = false
    @State private var editing: LiveMemoryItem?
    @State private var editText = ""
    @State private var forgetting: LiveMemoryItem?
    @State private var showSettings = false
    /// B5-2: a session clue opens its session.
    let onOpenTask: (String) -> Void

    public init(store: MemoryStore, projects: [(id: String, name: String)],
                onOpenTask: @escaping (String) -> Void = { _ in }) {
        self.store = store
        self.projects = projects
        self.onOpenTask = onOpenTask
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            header
            tabs
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    if let later = store.tab.later {
                        PaneEmptyState(systemImage: "hourglass", title: store.tab.title, hint: later)
                            .frame(maxWidth: .infinity)
                            .padding(.top, AeroricSpace.xxl)
                    } else if store.tab == .log {
                        runsContent
                    } else {
                        content
                    }
                }
                .padding(AeroricSpace.xl)
                .frame(maxWidth: 760, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollIndicators(.never)
            .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.top, AeroricSpace.l)
        .padding(.bottom, AeroricSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(AeroricColor.canvas)
        .task(id: "\(store.scope ?? "")|\(store.tab.rawValue)") { await store.reload() }
        .sheet(item: $editing) { item in editSheet(item) }
        .sheet(isPresented: $showSettings) {
            MemoryExtractSettingsSheet(store: store, onClose: { showSettings = false })
        }
        .confirmationDialog("永久遗忘这条记忆？", isPresented: Binding(
            get: { forgetting != nil }, set: { if !$0 { forgetting = nil } }
        ), presenting: forgetting) { item in
            Button("永久遗忘", role: .destructive) { Task { await store.forget(item) } }
        } message: { _ in
            Text("原文和它的旧版本都会删除，只留一个不含原文的签名，Agent 以后不能再把它提回来。")
        }
        .alert("操作失败", isPresented: Binding(get: { store.errorText != nil }, set: { if !$0 { store.errorText = nil } })) {
            Button("好") { store.errorText = nil }
        } message: {
            Text(store.errorText ?? "")
        }
        .accessibilityIdentifier("memory.page")
    }

    // MARK: Header

    private var scopeTitle: String {
        guard let scope = store.scope else { return "全局" }
        return projects.first { $0.id == scope }?.name ?? "项目"
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "brain")
                .font(.system(size: 17, weight: .medium))
                .foregroundStyle(AeroricColor.textSecondary)
            VStack(alignment: .leading, spacing: 1) {
                Text("记忆").font(.system(size: AeroricType.title, weight: .semibold))
                Text(store.pending.isEmpty ? "你确认过的长期信息，新会话启动时自动带上"
                     : "\(store.pending.count) 条待确认")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(store.pending.isEmpty ? AeroricColor.textTertiary : AeroricColor.warning)
                    .lineLimit(1)
            }
            .fixedSize()
            Spacer(minLength: AeroricSpace.m)
            CapsulePicker(title: scopeTitle, systemImage: store.scope == nil ? "globe" : "folder") {
                Button("全局") { store.scope = nil }
                if !projects.isEmpty { Divider() }
                ForEach(projects, id: \.id) { project in
                    Button(project.name) { store.scope = project.id }
                }
            }
            .accessibilityIdentifier("memory.scope")
            if store.scope != nil {
                Toggle("这个项目用记忆", isOn: Binding(
                    get: { store.enabled ?? false },
                    set: { on in Task { await store.setEnabled(on) } }
                ))
                .toggleStyle(CapsuleToggleStyle())
                .help("开启后，这个项目的会话会带上项目事实，并能用 memory_recall、memory_propose")
                .accessibilityIdentifier("memory.enabled")
            }
            CapsuleField(systemImage: "magnifyingglass", placeholder: "搜索记忆", text: $store.query, height: 30)
                .onSubmit { Task { await store.reload() } }
                .frame(width: 200)
                .accessibilityIdentifier("memory.search")
        }
    }

    private var tabs: some View {
        HStack(spacing: AeroricSpace.s) {
            tabButtons
            Spacer(minLength: AeroricSpace.m)
            Button {
                showSettings = true
            } label: {
                Label(store.settings.auto ? "自动整理 · 开" : "整理设置", systemImage: "slider.horizontal.3")
                    .lineLimit(1)
                    .fixedSize()
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .accessibilityIdentifier("memory.extract.settings")
        }
    }

    @ViewBuilder private var tabButtons: some View {
            ForEach(MemoryTab.allCases) { tab in
                let count = store.count(tab)
                Button {
                    store.tab = tab
                } label: {
                    Text(count > 0 ? "\(tab.title) · \(count)" : tab.title)
                        .lineLimit(1)
                        .fixedSize()
                }
                .buttonStyle(AeroricCapsuleButtonStyle(store.tab == tab ? .tinted : .ghost, compact: true))
                .opacity(tab.later == nil || store.tab == tab ? 1 : 0.6)
                .accessibilityIdentifier("memory.tab.\(tab.rawValue)")
            }
    }

    // MARK: Content

    @ViewBuilder private var content: some View {
        let shown = store.shown
        if !shown.pending.isEmpty {
            sectionLabel("待确认 · \(shown.pending.count)", tint: AeroricColor.warning)
            ForEach(shown.pending) { item in row(item) }
        }
        if store.tab == .experience {
            sectionLabel(shown.active.isEmpty ? "还没有学会的经验"
                         : "只在 Agent 用 memory_recall 检索时出现，不会自动带进新会话 · 按置信度和有用次数排序")
        } else if store.tab == .history {
            sectionLabel(shown.active.isEmpty ? "还没有历史线索" : "每个结束的会话一条 · 只用于检索，不会自动带进新会话")
        } else {
            sectionLabel(shown.active.isEmpty ? "还没有记忆" : "生效中 · \(shown.active.count)")
        }
        if shown.active.isEmpty {
            Text(store.tab == .about
                 ? "在任一会话里输入 /记住 回复一律用简体中文，或在下面直接添加。"
                 : store.tab == .history ? "在“整理设置”里打开自动整理后，结束的会话会整理到这里。"
                 : store.tab == .experience ? "整理解决了问题的会话时，会顺带记下可复用的解法。"
                 : "在会话里输入 /记住 只在 main 分支工作，或在下面直接添加。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.l)
        }
        ForEach(shown.active) { item in row(item) }
        if !shown.history.isEmpty {
            Button {
                showHistory.toggle()
            } label: {
                Label("历史版本 · 已取代 \(shown.history.filter { $0.state == "superseded" }.count) · 已归档 \(shown.history.filter { $0.state == "archived" }.count)",
                      systemImage: showHistory ? "chevron.down" : "chevron.right")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            .buttonStyle(.plain)
            .padding(.top, AeroricSpace.s)
            .accessibilityIdentifier("memory.history")
            if showHistory {
                ForEach(shown.history) { item in row(item).opacity(0.7) }
            }
        }
        if !store.tab.modelWritten { addRow.padding(.top, AeroricSpace.m) }
        if store.tab == .about && !store.native.isEmpty {
            sectionLabel("各 Harness 自己的记忆文件 · 只读").padding(.top, AeroricSpace.l)
            ForEach(store.native) { file in
                NativeMemoryRow(file: file, note: store.importNotes[file.path],
                                onImport: { Task { await store.importNative(file) } })
            }
        }
    }

    /// B5-2 整理记录.
    @ViewBuilder private var runsContent: some View {
        sectionLabel(MemoryText.runsSummary(store.runsPage))
        if store.runsPage.runs.isEmpty {
            Text(store.settings.hasModel ? "还没有整理过。会话结束后会自动整理，也可以在会话菜单里点“整理这次会话”。"
                 : "先在“整理设置”里选一个分析模型。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.l)
        }
        ForEach(store.runsPage.runs) { run in runRow(run) }
    }

    private func runRow(_ run: LiveMemoryRun) -> some View {
        HStack(spacing: AeroricSpace.m) {
            CheckpointBadge(text: MemoryText.runState(run.state),
                            tint: run.state == "done" ? AeroricColor.success
                                : run.state == "failed" ? AeroricColor.danger : AeroricColor.textSecondary)
            // CU F2: the title gives way; model, tokens, cost and the
            // reason stay readable.
            Text(Date(timeIntervalSince1970: Double(run.atMs) / 1000).formatted(date: .omitted, time: .shortened))
                .font(.system(size: AeroricType.caption).monospacedDigit())
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize()
            Text(run.title.isEmpty ? "未命名会话" : run.title)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(minWidth: 60, maxWidth: .infinity, alignment: .leading)
            if let detail = MemoryText.runDetail(run) {
                Text(detail)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .fixedSize()
                    .layoutPriority(2)
            }
            if let reason = MemoryText.runReason(run) {
                Text(reason)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(run.state == "failed" ? AeroricColor.danger : AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 260, alignment: .leading)
                    .layoutPriority(1)
            }
            if run.state == "done" {
                CheckpointBadge(text: MemoryText.runOutput(run), tint: AeroricColor.accent)
            }
            if run.state == "failed" {
                Button("重试") { Task { await store.retry(run) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
            }
            if !run.taskId.isEmpty {
                Button { onOpenTask(run.taskId) } label: {
                    Image(systemName: "arrow.up.forward").foregroundStyle(AeroricColor.textTertiary)
                }
                .buttonStyle(.plain)
                .help("打开这个会话")
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 34)
        .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
        .help(MemoryText.runLine(run))
        .accessibilityIdentifier("memory.run.\(run.id)")
    }

    private func sectionLabel(_ title: String, tint: Color = AeroricColor.textTertiary) -> some View {
        Text(title)
            .font(.system(size: AeroricType.caption, weight: .medium))
            .foregroundStyle(tint)
            .lineLimit(1)
            .padding(.horizontal, AeroricSpace.s)
            .padding(.top, AeroricSpace.s)
    }

    private func symbol(_ item: LiveMemoryItem) -> String {
        if item.pinned { return "pin" }
        if item.state == "pending" { return "sparkles" }
        if item.kind == "sessionFact" { return "clock.arrow.circlepath" }
        if item.kind == "experience" { return "lightbulb" }
        return item.kind == "profile" ? "person" : "doc.text"
    }

    @ViewBuilder private func row(_ item: LiveMemoryItem) -> some View {
        let pending = item.state == "pending"
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: symbol(item))
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(pending ? AeroricColor.warning : item.pinned ? AeroricColor.accent : AeroricColor.textSecondary)
                .frame(width: 16)
            if let category = MemoryText.category(item.category) {
                CheckpointBadge(text: category, tint: AeroricColor.accent)
            }
            Text(item.text)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(MemoryText.help(item))
            if item.kind == "experience" && item.state == "active" {
                CheckpointBadge(text: MemoryText.confidence(item.confidence),
                                tint: item.confidence == "high" ? AeroricColor.success
                                    : item.confidence == "medium" ? AeroricColor.accent : AeroricColor.textSecondary)
                Text(MemoryText.stats(item))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .fixedSize()
                voteButton(item, helpful: true)
                voteButton(item, helpful: false)
            } else if item.state == "active" || pending {
                CheckpointBadge(text: MemoryText.source(item),
                                tint: item.assertion == "userExplicit" ? AeroricColor.success : AeroricColor.accent)
            } else {
                CheckpointBadge(text: MemoryText.state(item), tint: AeroricColor.textSecondary)
            }
            if pending {
                Button("确认") { Task { await store.confirm(item, accept: true) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .accessibilityIdentifier("memory.confirm")
                Button("拒绝") { Task { await store.confirm(item, accept: false) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("拒绝后它会被遗忘，Agent 不能再提同样的内容")
            } else {
                menu(item)
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 34)
        .background(Capsule(style: .continuous).fill(pending ? AeroricColor.warning.opacity(0.1) : AeroricColor.layer2))
        .accessibilityIdentifier("memory.row.\(item.id)")
    }

    private func voteButton(_ item: LiveMemoryItem, helpful: Bool) -> some View {
        Button {
            Task { await store.feedback(item, helpful: helpful) }
        } label: {
            Image(systemName: helpful ? "hand.thumbsup" : "hand.thumbsdown")
                .font(.system(size: AeroricType.caption))
                .frame(width: 24, height: 22)
                .background(Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .foregroundStyle(AeroricColor.textSecondary)
        .help(helpful ? "有用：更可信、更靠前" : "不对：降一档，低档再标就归档")
        .accessibilityLabel(helpful ? "有用" : "不对")
    }

    private func menu(_ item: LiveMemoryItem) -> some View {
        Menu {
            if item.state == "active" {
                Button("有用", systemImage: "hand.thumbsup") { Task { await store.feedback(item, helpful: true) } }
                Button("不对", systemImage: "hand.thumbsdown") { Task { await store.feedback(item, helpful: false) } }
                Divider()
            }
            if item.kind == "sessionFact" || item.kind == "experience" {
                if let task = item.evidence.first?.taskId {
                    Button("打开这个会话", systemImage: "arrow.up.forward") { onOpenTask(task) }
                }
                if item.state == "active" {
                    Button("归档", systemImage: "archivebox") { Task { await store.archive(item, true) } }
                }
            } else if item.state == "active" {
                Button("修改…", systemImage: "pencil") {
                    editText = item.text
                    editing = item
                }
                Button(item.pinned ? "取消置顶" : "置顶", systemImage: "pin") { Task { await store.pin(item) } }
                if item.projectId != nil {
                    Button("改为全局", systemImage: "globe") { Task { await store.move(item, to: nil) } }
                } else if let scope = store.scope {
                    Button("只用于当前项目", systemImage: "folder") { Task { await store.move(item, to: scope) } }
                }
                Button("归档", systemImage: "archivebox") { Task { await store.archive(item, true) } }
            } else if item.state == "archived" {
                Button("恢复", systemImage: "arrow.uturn.backward") { Task { await store.archive(item, false) } }
            }
            Divider()
            Button("永久遗忘…", systemImage: "trash", role: .destructive) { forgetting = item }
        } label: {
            Image(systemName: "ellipsis")
                .foregroundStyle(AeroricColor.textTertiary)
                .frame(width: 20, height: 20)
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .accessibilityIdentifier("memory.menu")
    }

    private var addRow: some View {
        HStack(spacing: AeroricSpace.m) {
            if store.tab == .about {
                CapsulePicker(title: MemoryText.category(category) ?? "偏好", systemImage: "person") {
                    ForEach(MemoryText.categories, id: \.self) { c in
                        Button(MemoryText.category(c) ?? c) { category = c }
                    }
                }
            } else {
                CapsulePicker(title: "事实", systemImage: "doc.text") { Button("事实与约定") {} }
            }
            CapsuleField(placeholder: store.tab == .about ? "记一条关于你的，比如：回复一律用简体中文"
                                                          : "记一条，比如：发布只在宿主空闲时重启",
                         text: $draft, height: 30)
                .onSubmit(add)
                .accessibilityIdentifier("memory.add.text")
            Button("记住", action: add)
                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                .fixedSize()
                .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("memory.add")
        }
    }

    private func add() {
        let text = draft.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return }
        draft = ""
        Task { await store.add(text, category: store.tab == .about ? category : nil) }
    }

    private func editSheet(_ item: LiveMemoryItem) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("修改记忆").font(.system(size: AeroricType.title, weight: .semibold))
            Text("旧的这条会标成“已取代”，留在历史版本里。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            CapsuleField(placeholder: item.text, text: $editText)
            HStack {
                Spacer()
                Button("取消") { editing = nil }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    let text = editText.trimmingCharacters(in: .whitespaces)
                    editing = nil
                    if !text.isEmpty && text != item.text { Task { await store.edit(item, to: text) } }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 460)
    }
}

/// A harness's own memory file, folded to its title until opened.
private struct NativeMemoryRow: View {
    let file: LiveNativeMemory
    /// B5-2: what 导入为候选 did.
    var note: String?
    var onImport: () -> Void = {}
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Button {
                    open.toggle()
                } label: {
                    HStack(spacing: AeroricSpace.m) {
                        Image(systemName: open ? "chevron.down" : "chevron.right")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(AeroricColor.textTertiary)
                        Image(systemName: "lock.doc")
                            .foregroundStyle(AeroricColor.textSecondary)
                        Text(file.title).lineLimit(1)
                        Spacer(minLength: AeroricSpace.m)
                        CheckpointBadge(text: MemoryText.harness(file.harness), tint: AeroricColor.textSecondary)
                    }
                    .font(.system(size: AeroricType.secondary))
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(height: 30)
                    .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .help(file.path)
                Button("导入为候选", action: onImport)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("按行导入到“待确认”，不调用模型；已有的和遗忘过的会跳过")
                    .accessibilityIdentifier("memory.native.import")
            }
            if let note {
                Text(note)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .padding(.horizontal, AeroricSpace.l)
            }
            if open {
                Text(file.truncated ? file.text + "\n…（只显示前 8KB）" : file.text)
                    .font(AeroricType.mono(AeroricType.code))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(AeroricSpace.l)
                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
            }
        }
    }
}

/// B5-2: the 整理设置 sheet — the analysis model, 自动整理, the daily limit.
struct MemoryExtractSettingsSheet: View {
    let store: MemoryStore
    let onClose: () -> Void
    @State private var draft = LiveMemorySettings()

    static let limits = [10, 30, 60, 100]

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("整理设置").font(.system(size: AeroricType.title, weight: .semibold))
            Text("会话结束时，用你选的模型把对话整理成历史线索和待确认的记忆。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
            row("分析模型") {
                CapsulePicker(title: MemoryText.modelTitle(draft, choices: store.models), systemImage: "cpu") {
                    if store.models.isEmpty {
                        Text("没有可用的模型：先在 Agent 配置里加一个支持 Chat 的连接")
                    }
                    ForEach(store.models) { choice in
                        Button("\(choice.connection) · \(choice.model)") {
                            draft.connectionId = choice.connectionId
                            draft.model = choice.model
                        }
                    }
                }
                .accessibilityIdentifier("memory.extract.model")
            }
            row("自动整理") {
                Toggle(draft.auto ? "开" : "关", isOn: Binding(
                    get: { draft.auto },
                    set: { on in
                        if on && !draft.hasModel {
                            store.errorText = "先选一个分析模型，再打开自动整理"
                        } else {
                            draft.auto = on
                        }
                    }
                ))
                .toggleStyle(CapsuleToggleStyle())
                .accessibilityIdentifier("memory.extract.auto")
            }
            row("每天最多") {
                CapsuleSlider(selection: Binding(
                    get: { Self.limits.min { abs($0 - draft.dailyLimit) < abs($1 - draft.dailyLimit) } ?? 30 },
                    set: { draft.dailyLimit = $0 }
                ), groupTitle: "每天最多整理几次", mode: .titleOnly,
                   segments: Self.limits.map { .init(value: $0, title: "\($0) 次") })
            }
            Text("只整理开启了记忆的项目 · 只发对话文字，不发工具输出 · 密钥先遮掉 · 候选都要你确认 · 打开之前结束的会话不会被整理")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button("取消", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    Task {
                        if await store.saveSettings(draft) { onClose() }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("memory.extract.save")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 480)
        .task {
            draft = store.settings
            await store.loadModels()
        }
    }

    private func row<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Text(title)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: 64, alignment: .leading)
            content()
            Spacer(minLength: 0)
        }
    }
}
