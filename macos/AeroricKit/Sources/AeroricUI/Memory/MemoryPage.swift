import SwiftUI
import AeroricProtocol

/// B5-1 (2026-10-31): the "记忆" page in the sidebar (Q-M7 b): a scope
/// (global or a project), five tabs, capsule rows. Proposals wait on top;
/// old versions and the archive fold under 历史版本.
public struct MemoryPage: View {
    @Bindable var store: MemoryStore
    let projects: [(id: String, name: String)]
    @State private var confirmingAll = false
    @State private var rejectingAll = false
    /// Pending rows drawn; more on request (thousands after an import).
    @State private var pendingShown = 100
    @State private var expanded: Set<String> = []
    @State private var draft = ""
    @State private var category = "preference"
    @State private var showHistory = false
    @State private var editing: LiveMemoryItem?
    @State private var editText = ""
    @State private var forgetting: LiveMemoryItem?
    @State private var showSettings = false
    @State private var planningHistory = false
    @State private var editingSkill: LiveSkillProposal?
    @State private var showClosedSkills = false
    /// B6: the memory whose 相关改动 sheet is open.
    @State private var chainOf: LiveMemoryItem?
    /// C1: the evidence whose 查看原话 sheet is open.
    @State private var sourceOf: MemoryEvidenceRef?
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
                LazyVStack(alignment: .leading, spacing: AeroricSpace.s) {
                    if let later = store.tab.later {
                        PaneEmptyState(systemImage: "hourglass", title: store.tab.title, hint: later)
                            .frame(maxWidth: .infinity)
                            .padding(.top, AeroricSpace.xxl)
                    } else if store.tab == .log {
                        runsContent
                    } else if store.tab == .skills {
                        skillsContent
                    } else {
                        content
                    }
                }
                .padding(AeroricSpace.xl)
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
        .sheet(item: $editingSkill) { proposal in
            SkillEditSheet(proposal: proposal, onSave: { edit in
                editingSkill = nil
                Task { await store.editProposal(proposal, edit) }
            }, onCancel: { editingSkill = nil })
        }
        .sheet(item: $chainOf) { item in
            MemoryChainSheet(store: store, item: item, onOpenTask: { task in
                chainOf = nil
                onOpenTask(task)
            }, onClose: { chainOf = nil })
        }
        .sheet(item: $sourceOf) { ref in
            MemoryEvidenceSheet(store: store, item: ref.item, index: ref.index, onOpenTask: { task in
                sourceOf = nil
                onOpenTask(task)
            }, onClose: { sourceOf = nil })
        }
        .sheet(isPresented: $showSettings) {
            MemoryExtractSettingsSheet(store: store, onClose: { showSettings = false })
        }
        .sheet(isPresented: $planningHistory) {
            MemoryHistorySheet(store: store, projects: projects, initialProject: store.scope,
                               onClose: { store.stopHistory(); planningHistory = false })
        }
        .confirmationDialog("确认这一页全部 \(store.shown.pending.count) 条待确认？", isPresented: $confirmingAll) {
            Button("全部确认") { Task { await store.confirmAll(accept: true) } }
        } message: {
            Text("它们会生效，新会话会带上其中的事实和偏好。")
        }
        .confirmationDialog("拒绝这一页全部 \(store.shown.pending.count) 条待确认？", isPresented: $rejectingAll) {
            Button("全部拒绝", role: .destructive) { Task { await store.confirmAll(accept: false) } }
        } message: {
            Text("拒绝后会被遗忘，Agent 和导入都不能再提同样的内容。")
        }
        .onChange(of: store.tab) { _, _ in pendingShown = 100 }
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
        .windowTitle(windowTitle)
        .accessibilityIdentifier("memory.page")
    }

    // MARK: Header

    private var scopeTitle: String {
        guard let scope = store.scope else { return "全局" }
        return projects.first { $0.id == scope }?.name ?? "项目"
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            CapsulePicker(title: scopeTitle, systemImage: store.scope == nil ? "globe" : "folder") {
                Button("全局") { store.scope = nil }
                if !projects.isEmpty { Divider() }
                ForEach(projects, id: \.id) { project in
                    Button(project.name) { store.scope = project.id }
                }
            }
            .accessibilityIdentifier("memory.scope")
            if store.scope != nil {
                Toggle("为这个项目启用记忆", isOn: Binding(
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
            Spacer(minLength: 0)
        }
    }

    private var windowTitle: WindowTitle {
        WindowTitle("记忆", subtitle: store.pending.isEmpty ? "你确认过的长期信息，新会话启动时自动带上"
                        : "\(store.pending.count) 条待确认",
                    systemImage: "brain", subtitleTint: store.pending.isEmpty ? .quiet : .warning)
    }

    private var tabs: some View {
        HStack(spacing: AeroricSpace.s) {
            tabButtons
            Spacer(minLength: AeroricSpace.m)
            if store.tab == .log {
                Button { planningHistory = true } label: {
                    Label("整理历史会话…", systemImage: "clock.arrow.circlepath")
                        .lineLimit(1)
                        .fixedSize()
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .disabled(projects.isEmpty)
                .accessibilityIdentifier("memory.history.open")
            }
            if store.tab != .skills && store.tab != .history {
                Button {
                    Task { await store.dream() }
                } label: {
                    Label(store.dreaming ? "正在整理…" : "现在整理", systemImage: "wand.and.stars")
                        .lineLimit(1)
                        .fixedSize()
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .disabled(!store.settings.hasModel || store.dreaming)
                .help(store.settings.hasModel
                      ? "把\(scopeTitle)里字面相近的记忆交给分析模型：重复的经验自动合并，其余的进待确认；可在整理记录里撤销"
                      : "先在“整理设置”里选一个分析模型")
                .accessibilityIdentifier("memory.dream")
            }
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
        if let note = store.dreamNote {
            Label("刚才整理：\(note)", systemImage: "wand.and.stars")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.s)
                .accessibilityIdentifier("memory.dream.note")
        }
        let dreams = store.pendingDreams(store.tab)
        if !dreams.isEmpty {
            sectionLabel("整理建议 · \(dreams.count)", tint: AeroricColor.warning)
            ForEach(dreams) { proposal in dreamCard(proposal) }
        }
        let accepted = store.acceptedDreams(store.tab)
        if !accepted.isEmpty {
            ForEach(accepted) { proposal in acceptedDreamRow(proposal) }
        }
        if !shown.pending.isEmpty {
            HStack(spacing: AeroricSpace.m) {
                sectionLabel("待确认 · \(shown.pending.count)", tint: AeroricColor.warning)
                Spacer(minLength: AeroricSpace.m)
                if let busy = store.busyText {
                    Text(busy).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1).fixedSize()
                }
                if shown.pending.count > 1 {
                    Button("全部确认") { confirmingAll = true }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .disabled(store.busyText != nil)
                    Button("全部拒绝") { rejectingAll = true }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .disabled(store.busyText != nil)
                }
            }
            let groups = Dictionary(grouping: shown.pending.prefix(pendingShown), by: MemoryText.pendingGroup)
            if groups.count > 1 {
                ForEach(groups.keys.sorted(), id: \.self) { group in
                    let all = shown.pending.filter { MemoryText.pendingGroup($0) == group }
                    HStack(spacing: AeroricSpace.m) {
                        Label("\(group) · \(all.count)", systemImage: "tray.full")
                            .font(.system(size: AeroricType.caption, weight: .medium))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .lineLimit(1)
                        Spacer(minLength: AeroricSpace.m)
                        Button("确认这组") { Task { await store.confirmAll(accept: true, group: group) } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                        Button("拒绝这组") { Task { await store.confirmAll(accept: false, group: group) } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    }
                    .padding(.top, AeroricSpace.s)
                    ForEach(groups[group] ?? []) { item in row(item) }
                }
            } else {
                ForEach(shown.pending.prefix(pendingShown)) { item in row(item) }
            }
            if shown.pending.count > pendingShown {
                Button("再显示 \(min(100, shown.pending.count - pendingShown)) 条 · 还有 \(shown.pending.count - pendingShown) 条") {
                    pendingShown += 100
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
            }
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

    /// B6: one dream proposal — its items, the model's pick marked, a
    /// 保留这条 on each; keeping one marks the others replaced by it.
    /// B6 second batch: an accepted proposal, with 撤回.
    private func acceptedDreamRow(_ proposal: LiveDreamProposal) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "checkmark.circle")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            Text(MemoryText.dreamDone(proposal))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(1)
            Spacer(minLength: AeroricSpace.m)
            Button("撤回") { Task { await store.revertDream(proposal) } }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .disabled(store.busyText != nil)
                .help("恢复这次整理之前的样子；同样的建议以后不再提")
                .accessibilityIdentifier("memory.dream.revert")
        }
        .padding(.horizontal, AeroricSpace.s)
        .accessibilityIdentifier("memory.dream.accepted.\(proposal.id)")
    }

    @ViewBuilder private func dreamCard(_ proposal: LiveDreamProposal) -> some View {
        switch proposal.relation {
        case "stale": staleCard(proposal)
        case "compress": compressCard(proposal)
        default: groupCard(proposal)
        }
    }

    private func dreamHeader(_ proposal: LiveDreamProposal, systemImage: String) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.warning)
                .frame(width: 16)
            Text(MemoryText.dreamHeading(proposal))
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .lineLimit(1)
            if !proposal.reason.isEmpty, proposal.relation != "stale" {
                Text(proposal.reason)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            Spacer(minLength: AeroricSpace.m)
        }
    }

    private func dreamLine(_ text: String, help: String? = nil) -> some View {
        Text(text)
            .font(.system(size: AeroricType.secondary))
            .lineLimit(2)
            .truncationMode(.tail)
            .frame(maxWidth: .infinity, alignment: .leading)
            .help(help ?? text)
            .padding(.horizontal, AeroricSpace.l)
            .frame(minHeight: 30)
            .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
    }

    private func dreamButtons(_ proposal: LiveDreamProposal, keep: String, keepHelp: String,
                              accept: String, acceptHelp: String) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Spacer(minLength: AeroricSpace.m)
            Button(keep) { Task { await store.decideDream(proposal, keep: nil, accept: false) } }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .help(keepHelp)
                .accessibilityIdentifier("memory.dream.reject")
            Button(accept) { Task { await store.decideDream(proposal, keep: nil, accept: true) } }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .fixedSize()
                .help(acceptHelp)
                .accessibilityIdentifier("memory.dream.accept")
        }
        .disabled(store.busyText != nil)
    }

    /// B6 second batch: 过时检测.
    private func staleCard(_ proposal: LiveDreamProposal) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            dreamHeader(proposal, systemImage: "clock.badge.questionmark")
            dreamLine(store.item(proposal.keep)?.text ?? proposal.keep,
                      help: store.item(proposal.keep).map(MemoryText.help))
            Text(MemoryText.dreamMissing(proposal))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .padding(.horizontal, AeroricSpace.l)
            dreamButtons(proposal, keep: "还有用", keepHelp: "留着这条；这些名字仍然找不到时不再提",
                         accept: "归档", acceptHelp: "移到历史版本里的“已归档”，不再带进会话和检索；能撤回")
        }
        .padding(AeroricSpace.m)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.warning.opacity(0.1)))
        .accessibilityIdentifier("memory.dream.\(proposal.id)")
    }

    /// B6 second batch: 画像压缩.
    private func compressCard(_ proposal: LiveDreamProposal) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            dreamHeader(proposal, systemImage: "rectangle.compress.vertical")
            Text("改写后（原来 \(proposal.ids.count) 条，置顶的不动）")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .padding(.horizontal, AeroricSpace.l)
            ForEach(Array(proposal.rewrite.enumerated()), id: \.offset) { _, line in
                dreamLine(line.text)
            }
            DisclosureGroup("原来的 \(proposal.ids.count) 条") {
                ForEach(proposal.ids, id: \.self) { id in
                    dreamLine(store.item(id)?.text ?? id)
                }
            }
            .font(.system(size: AeroricType.caption))
            .padding(.horizontal, AeroricSpace.l)
            dreamButtons(proposal, keep: "保持原样", keepHelp: "不改写；同样这几条以后不再提",
                         accept: "用改写后的", acceptHelp: "原来的几条标为“已取代”，在历史版本里还能看到；能撤回")
        }
        .padding(AeroricSpace.m)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.warning.opacity(0.1)))
        .accessibilityIdentifier("memory.dream.\(proposal.id)")
    }

    private func groupCard(_ proposal: LiveDreamProposal) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: proposal.relation == "conflict" ? "exclamationmark.triangle" : "square.on.square")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.warning)
                    .frame(width: 16)
                Text(MemoryText.dreamHeading(proposal))
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .lineLimit(1)
                if !proposal.reason.isEmpty {
                    Text(proposal.reason)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                Spacer(minLength: AeroricSpace.m)
                Button("都留着") { Task { await store.decideDream(proposal, keep: nil, accept: false) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("不改动这几条，以后整理也不再问这一组")
                    .accessibilityIdentifier("memory.dream.reject")
            }
            ForEach(proposal.ids, id: \.self) { id in
                let suggested = id == proposal.keep
                HStack(spacing: AeroricSpace.m) {
                    Text(store.item(id)?.text ?? id)
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(2)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .help(store.item(id).map(MemoryText.help) ?? id)
                    if let item = store.item(id) {
                        CheckpointBadge(text: MemoryText.source(item),
                                        tint: item.assertion == "userExplicit" ? AeroricColor.success : AeroricColor.accent)
                    }
                    Button(suggested ? "保留这条（建议）" : "保留这条") {
                        Task { await store.decideDream(proposal, keep: id, accept: true) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(suggested ? .tinted : .ghost, compact: true))
                    .fixedSize()
                    .help("留下这一条，其余几条标为“已取代”，在历史版本里还能看到")
                    .accessibilityIdentifier("memory.dream.keep")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(minHeight: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
            }
        }
        .padding(AeroricSpace.m)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(AeroricColor.warning.opacity(0.1)))
        .accessibilityIdentifier("memory.dream.\(proposal.id)")
    }

    /// B5-3b 技能提案.
    @ViewBuilder private var skillsContent: some View {
        let groups = store.proposalGroups
        if store.proposals.isEmpty {
            sectionLabel("还没有技能提案")
            Text("一条经验被标“有用” 3 次、升到高置信时，会自动提议把它升格为技能。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.l)
        }
        if !groups.pending.isEmpty {
            sectionLabel("待审 · \(groups.pending.count)", tint: AeroricColor.warning)
            ForEach(groups.pending) { proposal in skillRow(proposal) }
        }
        if !groups.accepted.isEmpty {
            sectionLabel("已采纳 · \(groups.accepted.count) · 本项目新开的会话会带上")
            ForEach(groups.accepted) { proposal in skillRow(proposal) }
        }
        if !groups.closed.isEmpty {
            Button {
                showClosedSkills.toggle()
            } label: {
                Label("已停用 · 已拒绝 · \(groups.closed.count)",
                      systemImage: showClosedSkills ? "chevron.down" : "chevron.right")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            .buttonStyle(.plain)
            .padding(.top, AeroricSpace.s)
            if showClosedSkills {
                ForEach(groups.closed) { proposal in skillRow(proposal).opacity(0.7) }
            }
        }
    }

    private func skillRow(_ proposal: LiveSkillProposal) -> some View {
        let pending = proposal.state == "pending"
        return HStack(spacing: AeroricSpace.m) {
            Image(systemName: proposal.state == "accepted" ? "sparkles" : "lightbulb")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(pending ? AeroricColor.warning
                                 : proposal.state == "accepted" ? AeroricColor.success : AeroricColor.textSecondary)
                .frame(width: 16)
            Text(proposal.name)
                .font(.system(size: AeroricType.secondary, weight: .medium).monospaced())
                .lineLimit(1)
                .fixedSize()
            Text(proposal.description)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(proposal.body)
            CheckpointBadge(text: proposal.global ? "所有项目" : "本项目", tint: AeroricColor.textSecondary)
            if let v = proposal.activeVersion {
                CheckpointBadge(text: "v\(v)", tint: AeroricColor.accent)
            }
            if pending {
                Button("采纳") { Task { await store.decide(proposal, accept: true) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .accessibilityIdentifier("memory.skill.accept")
                Button("拒绝") { Task { await store.decide(proposal, accept: false) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("拒绝后这条经验不会再被提议")
                Button("编辑…") { editingSkill = proposal }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
            } else {
                if proposal.state != "accepted" {
                    CheckpointBadge(text: MemoryText.proposalState(proposal.state), tint: AeroricColor.textSecondary)
                }
                skillMenu(proposal)
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, 7)
        .frame(minHeight: 34)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous).fill(pending ? AeroricColor.warning.opacity(0.1) : AeroricColor.layer2))
        .accessibilityIdentifier("memory.skill.\(proposal.id)")
    }

    private func skillMenu(_ proposal: LiveSkillProposal) -> some View {
        Menu {
            if proposal.state != "rejected" {
                Button("编辑…", systemImage: "pencil") { editingSkill = proposal }
                Menu("版本历史") {
                    ForEach(proposal.versions.reversed()) { version in
                        Button(version.n == proposal.activeVersion ? "v\(version.n)（当前）" : "回到 v\(version.n)") {
                            Task { await store.useVersion(proposal, version.n) }
                        }
                        .disabled(version.n == proposal.activeVersion)
                    }
                }
                Divider()
                if proposal.state == "disabled" {
                    Button("启用", systemImage: "play") { Task { await store.disable(proposal, false) } }
                } else {
                    Button("停用", systemImage: "pause") { Task { await store.disable(proposal, true) } }
                }
            }
            if let task = proposal.taskId {
                Button("打开来源会话", systemImage: "arrow.up.forward") { onOpenTask(task) }
            }
        } label: {
            Image(systemName: "ellipsis")
                .foregroundStyle(AeroricColor.textTertiary)
                .frame(width: 20, height: 20)
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
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
                                : run.state == "partial" ? AeroricColor.warning
                                : run.state == "failed" ? AeroricColor.danger : AeroricColor.textSecondary)
            // CU F2: the title gives way; model, tokens, cost and the
            // reason stay readable.
            Text(Date(timeIntervalSince1970: Double(run.atMs) / 1000).formatted(date: .omitted, time: .shortened))
                .font(.system(size: AeroricType.caption).monospacedDigit())
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize()
            Text(MemoryText.runTitle(run))
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
                    .foregroundStyle(run.state == "failed" ? AeroricColor.danger
                                     : run.state == "partial" ? AeroricColor.warning : AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 260, alignment: .leading)
                    .layoutPriority(1)
            }
            if run.state == "done" || run.state == "partial" {
                CheckpointBadge(text: MemoryText.runOutput(run), tint: AeroricColor.accent)
            }
            if run.undoneMs != nil {
                CheckpointBadge(text: "已撤销", tint: AeroricColor.textSecondary)
            } else if MemoryText.canUndo(run) {
                Button("撤销这次整理") { Task { await store.undoDream(run) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("自动合并的经验恢复成生效中，还没确认的整理建议收回；你已确认的不变")
                    .accessibilityIdentifier("memory.dream.undo")
            }
            if !run.isDream && (run.state == "failed" || run.state == "partial") {
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
        let foldable = !pending && MemoryText.isLong(item.text)
        let open = pending || expanded.contains(item.id)
        HStack(alignment: open ? .top : .center, spacing: AeroricSpace.m) {
            if foldable {
                Image(systemName: open ? "chevron.down" : "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(width: 10, height: 18)
                    .contentShape(Rectangle())
                    .onTapGesture { toggle(item) }
                    .accessibilityLabel(open ? "收起" : "展开")
                    .accessibilityAddTraits(.isButton)
            }
            Image(systemName: symbol(item))
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(pending ? AeroricColor.warning : item.pinned ? AeroricColor.accent : AeroricColor.textSecondary)
                .frame(width: 16)
            if let category = MemoryText.category(item.category) {
                CheckpointBadge(text: category, tint: AeroricColor.accent)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(item.text)
                    .font(.system(size: AeroricType.secondary))
                    .lineLimit(open ? nil : 1)
                    .truncationMode(.tail)
                    .fixedSize(horizontal: false, vertical: open)
                    .textSelection(.enabled)
                if open, let source = MemoryText.evidenceLine(item) {
                    HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.s) {
                        Text(source)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(2)
                            .truncationMode(.middle)
                        if let index = MemoryText.sourceIndex(item) {
                            Button("查看原话") { sourceOf = MemoryEvidenceRef(item: item, index: index) }
                                .buttonStyle(.plain)
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.accent)
                                .fixedSize()
                                .help("在来源会话里找到这句原话")
                                .accessibilityIdentifier("memory.evidence.open")
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture { if foldable { toggle(item) } }
            .help(MemoryText.help(item))
            if item.kind == "experience", let promoted = MemoryText.promotion(store.proposal(for: item.id)) {
                CheckpointBadge(text: promoted, tint: AeroricColor.accent)
            }
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
            if pending && store.deciding.contains(item.id) {
                Text("处理中…")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize()
            } else if pending {
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
        .padding(.vertical, open ? AeroricSpace.m : 0)
        .frame(minHeight: 34)
        .background(RoundedRectangle(cornerRadius: 17, style: .continuous)
            .fill(pending ? AeroricColor.warning.opacity(0.1) : AeroricColor.layer2))
        .accessibilityIdentifier("memory.row.\(item.id)")
    }

    private func toggle(_ item: LiveMemoryItem) {
        if expanded.contains(item.id) { expanded.remove(item.id) } else { expanded.insert(item.id) }
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
            if let index = MemoryText.sourceIndex(item) {
                Button("查看原话…", systemImage: "text.quote") {
                    sourceOf = MemoryEvidenceRef(item: item, index: index)
                }
            }
            if MemoryText.hasChain(item) {
                Button("相关改动…", systemImage: "chevron.left.forwardslash.chevron.right") { chainOf = item }
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
            row("按意思找") {
                Toggle(draft.semantic ? "开" : "关", isOn: $draft.semantic)
                    .toggleStyle(CapsuleToggleStyle())
                    .help("召回和整理时也按意思找相近的记忆，不只看字面")
                    .accessibilityIdentifier("memory.extract.semantic")
            }
            if draft.semantic {
                VStack(alignment: .leading, spacing: 2) {
                    if let semantic = store.semantic {
                        Text(MemoryText.semanticLine(semantic))
                            .foregroundStyle(semantic.error == nil ? AeroricColor.textSecondary : AeroricColor.warning)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Text("用随手记设置里的 embedding 模型。nomic-embed-text 对中文分不清相关和无关，建议换 bge-m3 这类支持中文的模型。")
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .font(.system(size: AeroricType.caption))
                .padding(.leading, 64 + AeroricSpace.m)
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
            await store.loadSemantic()
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

/// Q4 (2026-11-02): 整理历史会话 — pick a project and a time, see how many
/// sessions and roughly what it costs, then run it one session at a time.
struct MemoryHistorySheet: View {
    let store: MemoryStore
    let projects: [(id: String, name: String)]
    let initialProject: String?
    let onClose: () -> Void
    @State private var projectID: String?
    @State private var days = 30

    static let ranges = [(7, "最近 7 天"), (30, "最近 30 天"), (90, "最近 90 天"), (0, "全部")]

    private var sinceMs: UInt64? {
        days == 0 ? nil : UInt64(Date().timeIntervalSince1970 * 1000) - UInt64(days) * 86_400_000
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("整理历史会话").font(.system(size: AeroricType.title, weight: .semibold))
            Text("用分析模型读导入的历史会话，整理成历史线索和待确认的记忆。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
            row("项目") {
                CapsulePicker(title: projects.first { $0.id == projectID }?.name ?? "选择项目", systemImage: "folder") {
                    ForEach(projects, id: \.id) { project in
                        Button(project.name) { projectID = project.id }
                    }
                }
                .disabled(store.history.running)
                .accessibilityIdentifier("memory.history.project")
            }
            row("时间") {
                CapsuleSlider(selection: $days, groupTitle: "整理哪段时间的会话", mode: .titleOnly,
                              segments: Self.ranges.map { .init(value: $0.0, title: $0.1) })
                    .disabled(store.history.running)
            }
            summary
            Text("只发对话文字，不发工具输出 · 密钥先遮掉 · 结果进“待确认” · 关掉这个窗口会停下")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button(store.history.running ? "停止" : "关闭") {
                    if store.history.running { store.stopHistory() } else { onClose() }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .accessibilityIdentifier("memory.history.stop")
                if !store.history.running {
                    Button("开始整理") { store.startHistory() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                        .disabled(store.historyPlan.map { MemoryText.historyBlocker($0) != nil } ?? true)
                        .keyboardShortcut(.defaultAction)
                        .accessibilityIdentifier("memory.history.start")
                }
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520)
        .onAppear { projectID = initialProject ?? projects.first?.id }
        .task(id: "\(projectID ?? "")|\(days)") {
            if let projectID { await store.planHistory(projectID: projectID, sinceMs: sinceMs) }
        }
    }

    @ViewBuilder private var summary: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let plan = store.historyPlan {
                line(MemoryText.historyCount(plan), symbol: "tray.full")
                line(MemoryText.historyCost(plan), symbol: "dollarsign.circle")
                line(MemoryText.historyLimit(plan), symbol: "gauge.with.dots.needle.33percent",
                     tint: plan.remainingToday < plan.pending ? AeroricColor.warning : nil)
                if let blocker = MemoryText.historyBlocker(plan), store.history.total == 0 {
                    line(blocker, symbol: "exclamationmark.triangle", tint: AeroricColor.warning)
                }
            } else {
                line("正在统计…", symbol: "hourglass")
            }
            if store.history.total > 0 {
                ProgressView(value: Double(store.history.finished), total: Double(max(store.history.total, 1)))
                    .progressViewStyle(.linear)
                    .tint(AeroricColor.accent)
                line(MemoryText.historyProgress(store.history),
                     symbol: store.history.running ? "arrow.triangle.2.circlepath" : "checkmark.circle",
                     tint: store.history.running ? AeroricColor.accent : nil)
                    .accessibilityIdentifier("memory.history.progress")
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(16))
    }

    private func line(_ text: String, symbol: String, tint: Color? = nil) -> some View {
        Label(text, systemImage: symbol)
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(tint ?? AeroricColor.textSecondary)
            .lineLimit(1)
            .truncationMode(.tail)
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

/// B5-3b: 编辑技能提案 — name, description, scope and the SKILL.md.
struct SkillEditSheet: View {
    let proposal: LiveSkillProposal
    let onSave: (LiveSkillEdit) -> Void
    let onCancel: () -> Void
    @State private var name = ""
    @State private var description = ""
    @State private var text = ""
    @State private var global = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("编辑技能提案").font(.system(size: AeroricType.title, weight: .semibold))
            Text(proposal.state == "pending" ? "采纳前随时可改" : "保存会生成新版本，可随时回到旧版")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            CapsuleField(placeholder: "技能名，如 learned-1a2b3c", text: $name)
            CapsuleField(placeholder: "一句话：做什么、什么时候用", text: $description)
            CapsuleSlider(selection: $global, groupTitle: "作用域", mode: .titleOnly,
                          segments: [.init(value: false, title: "本项目"), .init(value: true, title: "所有项目")])
            TextEditor(text: $text)
                .font(AeroricType.mono(AeroricType.code))
                .scrollIndicators(.never)
                .scrollContentBackground(.hidden)
                .padding(AeroricSpace.m)
                .frame(height: 260)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                .accessibilityIdentifier("memory.skill.body")
            HStack {
                Spacer()
                Button("取消", action: onCancel)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    onSave(LiveSkillEdit(
                        name: name == proposal.name ? nil : name,
                        description: description == proposal.description ? nil : description,
                        body: text == proposal.body ? nil : text,
                        global: global == proposal.global ? nil : global))
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("memory.skill.save")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 560)
        .onAppear {
            name = proposal.name
            description = proposal.description
            text = proposal.body
            global = proposal.global
        }
    }
}

/// B6 (2026-11-13): 代码版本链 — the code the memory's sessions changed,
/// block by block (file and the definition it falls in), newest first;
/// undone turns stay listed. Each block opens to its diff.
struct MemoryChainSheet: View {
    let store: MemoryStore
    let item: LiveMemoryItem
    let onOpenTask: (String) -> Void
    let onClose: () -> Void
    @State private var open: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("相关改动").font(.system(size: AeroricType.title, weight: .semibold))
            Text(item.text)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(2)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: AeroricSpace.s) {
                    if let chain = store.chain {
                        if chain.isEmpty {
                            Text("这条记忆的来源会话没有记下改动：只记开启记忆的项目、改过文件的轮次，保留 180 天。")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        ForEach(chain) { change in row(change) }
                    } else {
                        ProgressView().controlSize(.small)
                    }
                }
            }
            .scrollIndicators(.hidden)
            .frame(height: 360)
            HStack {
                Spacer()
                Button("关闭", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 620)
        .task { await store.loadChain(item) }
    }

    @ViewBuilder private func row(_ change: LiveCodeChange) -> some View {
        let isOpen = open.contains(change.id)
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: isOpen ? "chevron.down" : "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(width: 10)
                Text(MemoryText.changePlace(change))
                    .font(.system(size: AeroricType.secondary).monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(MemoryText.changeDetail(change))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(change.state == "undone" ? AeroricColor.warning : AeroricColor.textSecondary)
                    .lineLimit(1)
                    .fixedSize()
                Button { onOpenTask(change.taskId) } label: {
                    Image(systemName: "arrow.up.forward").foregroundStyle(AeroricColor.textTertiary)
                }
                .buttonStyle(.plain)
                .help("打开改动它的会话")
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 30)
            .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
            .contentShape(Capsule())
            .onTapGesture {
                if isOpen { open.remove(change.id) } else { open.insert(change.id) }
            }
            if isOpen && !change.diff.isEmpty {
                Text(change.diff)
                    .font(AeroricType.mono(AeroricType.code))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(AeroricSpace.l)
                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
            }
        }
        .accessibilityIdentifier("memory.chain.\(change.id)")
    }
}

/// C1: which evidence of which memory the 查看原话 sheet shows.
struct MemoryEvidenceRef: Identifiable {
    let item: LiveMemoryItem
    let index: Int
    var id: String { "\(item.id)#\(index)" }
}

/// C1 (2026-11-19): 查看原话 — the source message read again, the words
/// marked; or why it cannot be shown (changed, gone, not recorded), with
/// the excerpt kept at the time.
struct MemoryEvidenceSheet: View {
    let store: MemoryStore
    let item: LiveMemoryItem
    let index: Int
    let onOpenTask: (String) -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("原话").font(.system(size: AeroricType.title, weight: .semibold))
            Text(item.text)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(2)
            if let error = store.lookupError {
                Text(error)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.danger)
                    .fixedSize(horizontal: false, vertical: true)
            } else if let lookup = store.lookup {
                Text(MemoryText.lookupStatus(lookup))
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(lookup.status == "located" ? AeroricColor.textSecondary : AeroricColor.warning)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("memory.evidence.status")
                ScrollView {
                    Text(lookup.message.map(marked) ?? AttributedString(lookup.excerpt))
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(lookup.message == nil ? AeroricColor.textSecondary : AeroricColor.text)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(AeroricSpace.l)
                }
                .scrollIndicators(.automatic)
                .frame(maxHeight: 320)
                .fixedSize(horizontal: false, vertical: true)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
            } else {
                ProgressView().controlSize(.small)
            }
            HStack {
                Spacer()
                if let task = store.lookup?.taskId, store.lookup?.status != "gone" {
                    Button("打开这个会话") { onOpenTask(task) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted))
                        .accessibilityIdentifier("memory.evidence.openTask")
                }
                Button("关闭", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 620)
        .task { await store.loadLookup(item, index: index) }
    }

    /// The message with the words marked.
    private func marked(_ message: LiveEvidenceMessage) -> AttributedString {
        let parts = MemoryText.split(message)
        var words = AttributedString(parts.words)
        words.backgroundColor = AeroricColor.accent.opacity(0.22)
        words.font = .system(size: AeroricType.secondary, weight: .semibold)
        return AttributedString(parts.before) + words + AttributedString(parts.after)
    }
}
