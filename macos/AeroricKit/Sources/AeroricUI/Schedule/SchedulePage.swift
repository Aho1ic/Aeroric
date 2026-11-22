import AeroricProtocol
import SwiftUI

/// A12 (2026-11-14): the 定时任务 page — rules that open a new session in a
/// project at set times, each with its 运行记录.
public struct SchedulePage: View {
    @Bindable var store: ScheduleStore
    let projects: [(id: String, name: String)]
    let agents: [AgentChoice]
    /// 2026-11-22: a project's Agent configurations that can start.
    let targets: (String) -> [RerunTarget]
    let loadTargets: (String) async -> Void
    let onOpenTask: (String) -> Void
    @State private var editing: LiveScheduleRule?
    @State private var expanded: Set<String> = []
    @State private var deleting: LiveScheduleRule?

    public init(store: ScheduleStore, projects: [(id: String, name: String)], agents: [AgentChoice],
                targets: @escaping (String) -> [RerunTarget] = { _ in [] },
                loadTargets: @escaping (String) async -> Void = { _ in },
                onOpenTask: @escaping (String) -> Void) {
        self.store = store
        self.projects = projects
        self.agents = agents
        self.targets = targets
        self.loadTargets = loadTargets
        self.onOpenTask = onOpenTask
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text("到点在项目里开一个新会话，发你写好的提示词。可以先跑一条检查命令，没变化就不唤醒模型。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(2)
                Spacer(minLength: AeroricSpace.m)
                Button {
                    editing = LiveScheduleRule(projectId: projects.first?.id ?? "", agent: agents.first?.id ?? "")
                } label: {
                    Label("新建", systemImage: "plus")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                .fixedSize()
                .disabled(projects.isEmpty)
                .accessibilityIdentifier("schedule.new")
            }
            if let error = store.errorText {
                CapsuleErrorLine(error, identifier: "schedule.error") { store.errorText = nil }
            }
            if store.rules.isEmpty {
                ContentUnavailableView("还没有定时任务", systemImage: "clock.arrow.circlepath",
                                       description: Text(projects.isEmpty ? "先打开一个项目。" : "比如每天早上看看昨晚有没有新提交、每周五写周报。"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.s) {
                        ForEach(store.rules) { rule in row(rule) }
                    }
                }
                .scrollIndicators(.hidden)
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: NativePanelGeometry.mainShell.readingColumnWidth, maxHeight: .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        .windowTitle(WindowTitle("定时任务", subtitle: store.rules.isEmpty ? nil : "\(store.rules.count) 条",
                                 systemImage: "clock.arrow.circlepath"))
        .task { await store.reload() }
        .sheet(item: $editing) { rule in
            ScheduleEditorSheet(store: store, rule: rule, projects: projects, agents: agents, targets: targets,
                                loadTargets: loadTargets) { editing = nil }
        }
        .confirmationDialog("删除这个定时任务？", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                            presenting: deleting) { rule in
            Button("删除“\(rule.name)”和它的运行记录", role: .destructive) { Task { await store.delete(rule) } }
        }
        .accessibilityIdentifier("schedule.page")
    }

    private func projectName(_ id: String) -> String {
        projects.first { $0.id == id }?.name ?? "项目已移除"
    }

    private func agentName(_ id: String) -> String {
        agents.first { $0.id == id }?.title ?? id
    }

    @ViewBuilder private func row(_ rule: LiveScheduleRule) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Toggle("", isOn: Binding(get: { rule.enabled }, set: { on in Task { await store.setEnabled(rule, on) } }))
                    .toggleStyle(CapsuleToggleStyle())
                    .labelsHidden()
                    .accessibilityIdentifier("schedule.enabled.\(rule.id)")
                VStack(alignment: .leading, spacing: 2) {
                    Text(rule.name).font(.system(size: AeroricType.body, weight: .medium)).lineLimit(1)
                    Text([ScheduleText.repeatText(rule.repeatRule), projectName(rule.projectId),
                          ScheduleText.runner(rule, targets: targets(rule.projectId), agentName: agentName(rule.agent)),
                          rule.check == nil ? nil : "先检查", rule.activeHours.map { "仅 " + ScheduleText.hours($0) },
                          rule.whenBusy == "after" ? "忙时补跑" : nil].compactMap { $0 }.joined(separator: " · "))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: AeroricSpace.m)
                Text(ScheduleText.next(rule))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .fixedSize()
                Button(store.running == rule.id ? "运行中…" : "立即运行") { Task { await store.runNow(rule); expanded.insert(rule.id) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .disabled(store.running != nil)
                    .help("不等时间，现在按规则跑一次（照样先检查）")
                    .accessibilityIdentifier("schedule.runNow.\(rule.id)")
                Menu {
                    Button("编辑…") { editing = rule }
                    Button(expanded.contains(rule.id) ? "收起运行记录" : "运行记录") { toggleRuns(rule) }
                    Divider()
                    Button("删除…", role: .destructive) { deleting = rule }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .accessibilityIdentifier("schedule.menu.\(rule.id)")
            }
            if expanded.contains(rule.id) {
                runsList(rule)
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.layer1))
        .accessibilityIdentifier("schedule.row.\(rule.id)")
    }

    private func toggleRuns(_ rule: LiveScheduleRule) {
        if expanded.remove(rule.id) == nil {
            expanded.insert(rule.id)
            Task { await store.loadRuns(rule) }
        }
    }

    @ViewBuilder private func runsList(_ rule: LiveScheduleRule) -> some View {
        let runs = store.runs[rule.id] ?? []
        if runs.isEmpty {
            Text("还没有运行过")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .padding(.leading, AeroricSpace.xl)
        }
        ForEach(runs.prefix(20)) { run in
            HStack(spacing: AeroricSpace.m) {
                Text(ScheduleText.date(run.atMs))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(width: 96, alignment: .leading)
                Text(ScheduleText.outcome(run))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(ScheduleText.failed(run) ? AeroricColor.warning : AeroricColor.textSecondary)
                    .lineLimit(1)
                if let detail = ScheduleText.detail(run) {
                    Text(detail)
                        .font(AeroricType.mono(AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .help(detail)
                }
                Spacer(minLength: AeroricSpace.m)
                if let task = run.taskId, run.outcome == "started" {
                    Button("打开会话") { onOpenTask(task) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
            }
            .padding(.leading, AeroricSpace.xl)
            .frame(minHeight: 26)
        }
    }
}

/// A12: 新建 / 编辑 a rule.
struct ScheduleEditorSheet: View {
    let store: ScheduleStore
    let projects: [(id: String, name: String)]
    let agents: [AgentChoice]
    let targets: (String) -> [RerunTarget]
    let loadTargets: (String) async -> Void
    let onClose: () -> Void
    @State private var draft: LiveScheduleRule
    @State private var kind: Kind
    @State private var time: Date
    @State private var day: Date
    @State private var weekdays: Set<Int>
    @State private var hours: Int
    @State private var check: String
    /// E16: the result structure as JSON text; empty asks for none.
    @State private var resultSchema: String
    @State private var limitHours: Bool
    @State private var hoursStart: Date
    @State private var hoursEnd: Date

    enum Kind: Hashable { case once, daily, weekly, every }
    static let intervals = [1, 2, 4, 6, 12, 24]

    init(store: ScheduleStore, rule: LiveScheduleRule, projects: [(id: String, name: String)], agents: [AgentChoice],
         targets: @escaping (String) -> [RerunTarget] = { _ in [] },
         loadTargets: @escaping (String) async -> Void = { _ in },
         onClose: @escaping () -> Void) {
        self.store = store
        self.projects = projects
        self.agents = agents
        self.targets = targets
        self.loadTargets = loadTargets
        self.onClose = onClose
        _draft = State(initialValue: rule)
        _check = State(initialValue: rule.check ?? "")
        _resultSchema = State(initialValue: rule.resultSchema.map(ScheduleText.pretty) ?? "")
        let calendar = Calendar.current
        let today = calendar.startOfDay(for: Date())
        func at(_ minute: Int) -> Date { today.addingTimeInterval(Double(minute * 60)) }
        _limitHours = State(initialValue: rule.activeHours != nil)
        _hoursStart = State(initialValue: at(rule.activeHours?.start ?? 9 * 60))
        _hoursEnd = State(initialValue: at(rule.activeHours?.end ?? 18 * 60))
        switch rule.repeatRule {
        case let .once(atMs):
            let date = Date(timeIntervalSince1970: Double(atMs) / 1000)
            _kind = State(initialValue: .once)
            _time = State(initialValue: date)
            _day = State(initialValue: date)
            _weekdays = State(initialValue: [1, 2, 3, 4, 5])
            _hours = State(initialValue: 6)
        case let .daily(minute):
            _kind = State(initialValue: .daily)
            _time = State(initialValue: at(minute))
            _day = State(initialValue: Date())
            _weekdays = State(initialValue: [1, 2, 3, 4, 5])
            _hours = State(initialValue: 6)
        case let .weekly(days, minute):
            _kind = State(initialValue: .weekly)
            _time = State(initialValue: at(minute))
            _day = State(initialValue: Date())
            _weekdays = State(initialValue: Set(days))
            _hours = State(initialValue: 6)
        case let .every(h, _):
            _kind = State(initialValue: .every)
            _time = State(initialValue: at(9 * 60))
            _day = State(initialValue: Date())
            _weekdays = State(initialValue: [1, 2, 3, 4, 5])
            _hours = State(initialValue: h)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text(draft.id.isEmpty ? "新建定时任务" : "编辑定时任务")
                .font(.system(size: AeroricType.title, weight: .semibold))
            if draft.id.isEmpty { templates }
            CapsuleField(placeholder: "名字，比如：早上看新提交", text: $draft.name)
                .accessibilityIdentifier("schedule.edit.name")
            row("项目") {
                CapsulePicker(title: projects.first { $0.id == draft.projectId }?.name ?? "选一个项目", systemImage: "folder") {
                    ForEach(projects, id: \.id) { project in
                        Button(project.name) { draft.projectId = project.id }
                    }
                }
            }
            row("Agent") {
                CapsulePicker(title: agents.first { $0.id == draft.agent }?.title ?? "选一个 Agent", systemImage: "cpu") {
                    ForEach(agents) { agent in
                        Button(agent.title) {
                            // Another Agent: a configuration of the last one goes.
                            if agent.id != draft.agent, draft.configurationId != nil {
                                draft.configurationId = nil
                                draft.model = nil
                            }
                            draft.agent = agent.id
                        }
                    }
                }
            }
            configurationRows
            row("重复") {
                CapsuleSlider(selection: $kind, groupTitle: "重复方式", mode: .titleOnly, segments: [
                    .init(value: .once, title: "一次"), .init(value: .daily, title: "每天"),
                    .init(value: .weekly, title: "每周"), .init(value: .every, title: "间隔"),
                ])
            }
            row("时间") { timeControls }
            row("忙时") {
                CapsuleSlider(selection: $draft.whenBusy, groupTitle: "上一次还在跑时", mode: .titleOnly, segments: [
                    .init(value: "skip", title: "跳过"), .init(value: "after", title: "结束后补一次"),
                ])
            }
            row("时段") {
                Toggle("只在这段时间运行", isOn: $limitHours)
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .accessibilityIdentifier("schedule.edit.hours")
                if limitHours {
                    DatePicker("", selection: $hoursStart, displayedComponents: .hourAndMinute).labelsHidden()
                    Text("到").foregroundStyle(AeroricColor.textSecondary)
                    DatePicker("", selection: $hoursEnd, displayedComponents: .hourAndMinute).labelsHidden()
                }
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                Text("提示词").font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
                TextEditor(text: $draft.prompt)
                    .font(.system(size: AeroricType.body))
                    .scrollIndicators(.never)
                    .scrollContentBackground(.hidden)
                    .padding(AeroricSpace.m)
                    .frame(height: 110)
                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                    .accessibilityIdentifier("schedule.edit.prompt")
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                CapsuleField(systemImage: "terminal", placeholder: "可选的前置检查，例如 git fetch -q && git log HEAD..@{u} --oneline | grep .",
                             text: $check)
                    .accessibilityIdentifier("schedule.edit.check")
                Text("在项目目录里用你的登录 shell 运行，30 秒内退出码为 0 才开会话，输出的前 2000 字附在提示词后面；不为 0 就不唤醒模型。会话用这个 Agent 的默认权限模式，需要审批时会进“需要我处理”。每条每天最多开 24 个会话，错过的时间只补最近一次；不在活跃时段的时间推迟到下一段开始时补一次，“立即运行”不受时段限制。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            resultSchemaField
            if let error = store.errorText {
                CapsuleErrorLine(error, identifier: "schedule.edit.error") { store.errorText = nil }
            }
            HStack {
                Spacer()
                Button("取消", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    Task {
                        store.errorText = nil
                        switch ScheduleText.schema(from: resultSchema) {
                        case let .failure(error):
                            store.errorText = error.message
                        case let .success(schema):
                            var rule = built
                            rule.resultSchema = schema
                            if await store.save(rule) { onClose() }
                        }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("schedule.edit.save")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 560)
        .task(id: draft.projectId) { await loadTargets(draft.projectId) }
    }

    /// 2026-11-22: the project's Agent configurations; picking one names
    /// the Agent, and its models are offered.
    @ViewBuilder private var configurationRows: some View {
        let configurations = targets(draft.projectId)
        let picked = configurations.first { $0.id == draft.configurationId }
        if !configurations.isEmpty || draft.configurationId != nil {
            row("配置") {
                CapsulePicker(title: draft.configurationId.map { ScheduleText.configuration($0, targets: configurations) }
                                  ?? "不用配置（Agent 自己的登录）",
                              systemImage: "slider.horizontal.3") {
                    Button("不用配置（Agent 自己的登录）") {
                        draft.configurationId = nil
                        draft.model = nil
                    }
                    if !configurations.isEmpty { Divider() }
                    ForEach(configurations) { target in
                        Button(RerunText.title(target, among: configurations)) {
                            draft.configurationId = target.id
                            draft.agent = target.agent
                            draft.model = target.models.first
                        }
                    }
                }
                .accessibilityIdentifier("schedule.edit.configuration")
            }
            if let picked, !picked.models.isEmpty {
                row("模型") {
                    CapsulePicker(title: draft.model ?? "默认模型", systemImage: "sparkles") {
                        ForEach(picked.models, id: \.self) { name in
                            Button(name) { draft.model = name }
                        }
                    }
                    .accessibilityIdentifier("schedule.edit.model")
                }
            }
        }
    }

    /// E16: an optional JSON structure the result must fit.
    private var resultSchemaField: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("结果结构（可选）").font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
            TextEditor(text: $resultSchema)
                .font(AeroricType.mono(AeroricType.caption))
                .scrollIndicators(.never)
                .scrollContentBackground(.hidden)
                .padding(AeroricSpace.m)
                .frame(height: 72)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                .accessibilityLabel("结果结构")
                .accessibilityIdentifier("schedule.edit.result-schema")
            Text("填一个 JSON Schema，例如 {\"type\": \"object\", \"required\": [\"status\"]}。Agent 要在最后一条回复里用 json 代码块交出符合它的结果；会话结束后检查，不合格或没给，这次运行记为失败。支持 type、properties、required、additionalProperties: false、items、enum。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// 2026-11-14 常用模板: fills the draft, all of it editable.
    private var templates: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("从模板开始").font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
            ScrollView(.horizontal) {
                HStack(spacing: AeroricSpace.s) {
                    ForEach(ScheduleTemplate.all) { template in
                        Button { apply(template) } label: {
                            Label(template.name, systemImage: template.systemImage)
                        }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("schedule.template.\(template.id)")
                    }
                }
            }
            .scrollIndicators(.never)
        }
    }

    private func apply(_ template: ScheduleTemplate) {
        let today = Calendar.current.startOfDay(for: Date())
        func at(_ minute: Int) -> Date { today.addingTimeInterval(Double(minute * 60)) }
        draft.name = template.name
        draft.prompt = template.prompt
        check = template.check ?? ""
        switch template.repeatRule {
        case let .once(atMs):
            kind = .once
            day = Date(timeIntervalSince1970: Double(atMs) / 1000)
        case let .daily(minute):
            kind = .daily
            time = at(minute)
        case let .weekly(days, minute):
            kind = .weekly
            weekdays = Set(days)
            time = at(minute)
        case let .every(h, _):
            kind = .every
            hours = h
            draft.repeatRule = .every(hours: h, fromMs: UInt64(Date().timeIntervalSince1970 * 1000))
        }
    }

    @ViewBuilder private var timeControls: some View {
        switch kind {
        case .once:
            DatePicker("", selection: $day, in: Date()..., displayedComponents: [.date, .hourAndMinute])
                .labelsHidden()
        case .daily:
            DatePicker("", selection: $time, displayedComponents: .hourAndMinute).labelsHidden()
        case .weekly:
            HStack(spacing: AeroricSpace.s) {
                ForEach(1...7, id: \.self) { day in
                    Button(ScheduleText.weekdayNames[day - 1]) {
                        if weekdays.contains(day) { weekdays.remove(day) } else { weekdays.insert(day) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(weekdays.contains(day) ? .tinted : .ghost, compact: true))
                    .fixedSize()
                }
                DatePicker("", selection: $time, displayedComponents: .hourAndMinute).labelsHidden()
            }
        case .every:
            CapsuleSlider(selection: $hours, groupTitle: "间隔", mode: .titleOnly,
                          segments: Self.intervals.map { .init(value: $0, title: $0 == 24 ? "1 天" : "\($0) 小时") })
        }
    }

    /// The draft with its times taken from the controls.
    private var built: LiveScheduleRule {
        var rule = draft
        let parts = Calendar.current.dateComponents([.hour, .minute], from: time)
        let minute = (parts.hour ?? 9) * 60 + (parts.minute ?? 0)
        rule.repeatRule = switch kind {
        case .once: .once(atMs: UInt64(day.timeIntervalSince1970 * 1000))
        case .daily: .daily(minute: minute)
        case .weekly: .weekly(weekdays: weekdays.sorted(), minute: minute)
        case .every:
            if case let .every(_, from) = draft.repeatRule {
                .every(hours: hours, fromMs: from)
            } else {
                .every(hours: hours, fromMs: UInt64(Date().timeIntervalSince1970 * 1000))
            }
        }
        let trimmed = check.trimmingCharacters(in: .whitespacesAndNewlines)
        rule.check = trimmed.isEmpty ? nil : trimmed
        rule.activeHours = limitHours ? LiveActiveHours(start: Self.minute(hoursStart), end: Self.minute(hoursEnd)) : nil
        return rule
    }

    private static func minute(_ date: Date) -> Int {
        let parts = Calendar.current.dateComponents([.hour, .minute], from: date)
        return (parts.hour ?? 0) * 60 + (parts.minute ?? 0)
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
