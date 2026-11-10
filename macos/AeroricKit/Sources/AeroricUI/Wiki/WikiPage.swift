import SwiftUI
import AeroricProtocol

/// B5.5-1 (2026-10-31): a project's Wiki — the page tree on the left, the
/// page with its source footnotes on the right; sections the user changed
/// carry a thin accent bar. 编辑, 修订, 重新编译, 导出 and issues act on it.
public struct WikiPage: View {
    @Bindable var store: WikiStore
    let projects: [(id: String, name: String)]
    /// A footnote: open the note (project, relative path) or the memory.
    let onOpenSource: (String, LiveWikiSource) -> Void
    @State private var editing = false
    @State private var draft = ""
    @State private var showIssues = false
    @AppStorage("aeroric.wiki.lastProject") private var lastScope: String?
    @State private var exportDir = "docs/wiki"
    @State private var exportFiles: LiveWikiExportPlan?
    @State private var showExport = false
    @State private var collapsed: Set<String> = []

    public init(store: WikiStore, projects: [(id: String, name: String)],
                onOpenSource: @escaping (String, LiveWikiSource) -> Void = { _, _ in }) {
        self.store = store
        self.projects = projects
        self.onOpenSource = onOpenSource
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            header
            if store.scope != nil, let notice = WikiText.gapNotice(store.gaps) {
                Label(notice, systemImage: "exclamationmark.triangle")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(height: 28)
                    .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.12)))
                    .accessibilityIdentifier("wiki.gaps")
            }
            if store.scope == nil {
                PaneEmptyState(systemImage: "book.closed", title: "选一个项目",
                               hint: "Wiki 按项目编译：决策页来自 .agents/notes，操作手册来自学会的经验与技能。")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                HStack(alignment: .top, spacing: AeroricSpace.l) {
                    tree.frame(width: 300)
                    pageView
                }
            }
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.top, AeroricSpace.l)
        .padding(.bottom, AeroricSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(AeroricColor.canvas)
        .task(id: store.scope ?? "") { await store.reload() }
        .onAppear {
            // The project the Wiki was last opened on, across restarts.
            if store.scope == nil, let last = lastScope, projects.contains(where: { $0.id == last }) {
                store.show(scope: last)
            }
        }
        .onChange(of: store.scope) { _, scope in if let scope { lastScope = scope } }
        .sheet(isPresented: $editing) { editSheet }
        .sheet(isPresented: $showIssues) { issuesSheet }
        .sheet(isPresented: $showExport) { exportSheet }
        .alert("操作失败", isPresented: Binding(get: { store.errorText != nil }, set: { if !$0 { store.errorText = nil } })) {
            Button("好") { store.errorText = nil }
        } message: {
            Text(store.errorText ?? "")
        }
        .windowTitle(WindowTitle("Wiki", subtitle: store.notice ?? "\(store.pages.count) 页 · 笔记、经验或技能有变化时自动重新编译",
                                 systemImage: "book.closed"))
        .accessibilityIdentifier("wiki.page")
    }

    // MARK: Header

    private var scopeTitle: String {
        guard let scope = store.scope else { return "选择项目" }
        return projects.first { $0.id == scope }?.name ?? "项目"
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            CapsulePicker(title: scopeTitle, systemImage: "folder") {
                ForEach(projects, id: \.id) { project in
                    Button(project.name) { store.show(scope: project.id) }
                }
            }
            .accessibilityIdentifier("wiki.scope")
            if store.scope != nil {
                Button { Task { await store.compile() } } label: { Label("重新编译", systemImage: "arrow.clockwise") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(store.busy)
                    .fixedSize()
                    .accessibilityIdentifier("wiki.compile")
                Button { showExport = true } label: { Label("导出…", systemImage: "square.and.arrow.up") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(store.pages.isEmpty)
                    .fixedSize()
                Button { showIssues = true } label: {
                    Label("issue · \(store.openIssues.count)", systemImage: "exclamationmark.bubble")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(store.openIssues.isEmpty ? .ghost : .tinted, compact: true))
                .fixedSize()
                .accessibilityIdentifier("wiki.issues")
                if store.pages.contains(where: { $0.kind == "module" }) {
                    Button { Task { await store.explainAll() } } label: {
                        Label("全部生成说明", systemImage: "sparkles")
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(store.explainState.running || store.explainState.model == nil)
                    .fixedSize()
                    .help("给缺说明或说明过时的模块页生成职责和常见坑；用记忆页里选的分析模型")
                    .accessibilityIdentifier("wiki.explainAll")
                    let line = WikiText.explainLine(store.explainState)
                    if !line.isEmpty {
                        Text(line)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                            .accessibilityIdentifier("wiki.explainState")
                    }
                }
            }
            Spacer(minLength: 0)
        }
    }

    // MARK: Tree

    private var tree: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 2) {
                if store.pages.isEmpty {
                    Text("还没有页面。点“重新编译”从 .agents/notes 和学会的经验生成。")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(AeroricSpace.m)
                }
                let tree = store.tree
                ForEach(tree.top) { node($0) }
                ForEach(tree.decisions, id: \.category) { group in
                    Button {
                        if collapsed.contains(group.category) { collapsed.remove(group.category) }
                        else { collapsed.insert(group.category) }
                    } label: {
                        HStack(spacing: AeroricSpace.s) {
                            Image(systemName: collapsed.contains(group.category) ? "chevron.right" : "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                            Text("\(group.category) \(group.pages.count)").lineLimit(1)
                        }
                        .font(.system(size: AeroricType.caption, weight: .medium))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .padding(.horizontal, AeroricSpace.m)
                        .padding(.top, AeroricSpace.m)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    if !collapsed.contains(group.category) {
                        ForEach(group.pages) { node($0) }
                    }
                }
            }
            .padding(AeroricSpace.s)
        }
        .scrollIndicators(.never)
        .background(AeroricRadius.shape(20).fill(AeroricColor.layer1))
        .overlay(AeroricRadius.shape(20).stroke(AeroricColor.separator2, lineWidth: 0.5))
    }

    private func node(_ summary: LiveWikiPageSummary) -> some View {
        let title = WikiText.splitTitle(summary.title)
        return HoverCapsuleRow(selected: store.selected == summary.slug, height: title.detail == nil ? 28 : 44,
                        action: { Task { await store.open(summary.slug) } }) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: WikiText.symbol(summary.kind))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(width: 14)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title.main)
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(1)
                        .truncationMode(.tail)
                    if let detail = title.detail {
                        Text(detail)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                .help(summary.title)
                Spacer(minLength: 0)
                if store.sourceUnavailable(slug: summary.slug, kind: summary.kind) {
                    Image(systemName: "link.badge.plus")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.warning)
                        .help("来源暂时读不到")
                }
                if summary.edited > 0 {
                    Circle().fill(AeroricColor.accent).frame(width: 5, height: 5).help("有你改过的段落")
                }
            }
        }
        .background(Capsule(style: .continuous).fill(AeroricColor.layer2.opacity(store.selected == summary.slug ? 0 : 0.7)))
        .accessibilityIdentifier("wiki.node.\(summary.slug)")
    }

    // MARK: Page

    @ViewBuilder private var pageView: some View {
        ScrollView {
            if let page = store.page {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    pageHeader(page)
                    if store.sourceUnavailable(slug: page.slug, kind: page.kind) {
                        Text("来源暂时读不到，这是上次编译的内容")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.warning)
                            .lineLimit(1)
                    }
                    ForEach(page.blocks.filter { !($0.text.isEmpty && $0.userEdited) }) { block in
                        blockView(block)
                    }
                }
                .padding(AeroricSpace.xl)
                .frame(maxWidth: 780, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .scrollIndicators(.never)
        .background(AeroricRadius.shape(26).fill(AeroricColor.layer1))
        .overlay(AeroricRadius.shape(26).stroke(AeroricColor.separator2, lineWidth: 0.5))
    }

    private func pageHeader(_ page: LiveWikiPage) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Text(page.title)
                .font(.system(size: AeroricType.title, weight: .semibold))
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: AeroricSpace.m)
            if let status = page.status {
                CheckpointBadge(text: status, tint: WikiText.statusTint(status))
            }
            if let category = page.category {
                CheckpointBadge(text: category, tint: AeroricColor.textSecondary)
            }
            if let commit = page.sourceCommit {
                CheckpointBadge(text: "读自提交 \(commit.prefix(7))", tint: AeroricColor.textSecondary)
                    .help("本页的代码在提交 \(commit) 时读出")
            }
            if page.kind == "module" {
                let has = page.blocks.contains { $0.key.hasSuffix("#duties") }
                Button { Task { await store.explain() } } label: {
                    if store.explaining {
                        HStack(spacing: AeroricSpace.xs) { ProgressView().controlSize(.mini); Text("生成中…") }
                    } else {
                        Label(has ? "重新生成说明" : "生成说明", systemImage: "sparkles")
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .disabled(store.explaining || store.explainState.running || store.explainState.model == nil)
                .fixedSize()
                .help(store.explainState.model.map { "用 \($0) 根据这个模块的代码写职责和常见坑，每条都要有出处" }
                      ?? "先在记忆页里选分析模型")
                .accessibilityIdentifier("wiki.explain")
            }
            CapsulePicker(title: "修订 v\(page.revision)", systemImage: "clock.arrow.circlepath") {
                ForEach(store.revisions) { revision in
                    Button("v\(revision.n) · \(WikiText.reason(revision.reason)) · "
                           + Date(timeIntervalSince1970: Double(revision.atMs) / 1000)
                            .formatted(date: .abbreviated, time: .shortened)) {
                        Task { await store.revert(revision.n) }
                    }
                    .disabled(revision.n == page.revision)
                }
            }
            .accessibilityIdentifier("wiki.revisions")
            Button {
                draft = page.markdown
                editing = true
            } label: { Label("编辑", systemImage: "pencil") }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .fixedSize()
            .accessibilityIdentifier("wiki.edit")
        }
    }

    private func blockView(_ block: LiveWikiBlock) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let heading = block.heading {
                let style = WikiText.section(heading)
                HStack(spacing: AeroricSpace.s) {
                    RoundedRectangle(cornerRadius: 1.5, style: .continuous)
                        .fill(style.tint)
                        .frame(width: 3, height: 15)
                    Image(systemName: style.symbol)
                        .font(.system(size: AeroricType.secondary, weight: .medium))
                        .foregroundStyle(style.tint)
                    Text(style.title)
                        .font(.system(size: AeroricType.body, weight: .semibold))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                }
                .padding(.top, AeroricSpace.s)
            }
            // The sidebar's size (owner, 2026-11-02).
            SelectableMarkdown(text: block.text, size: AeroricType.body, reading: true) { url in
                guard let slug = WikiText.linkedSlug(url) else { return false }
                Task { await store.open(slug) }
                return true
            }
            HStack(spacing: AeroricSpace.s) {
                if let source = block.source, let scope = store.scope {
                    Button { onOpenSource(scope, source) } label: {
                        Label(WikiText.sourceLabel(source, all: block.sources), systemImage: WikiText.sourceSymbol(source))
                            .font(.system(size: AeroricType.caption))
                            .lineLimit(1)
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .help(WikiText.sourceHelp(source, all: block.sources))
                }
                if block.userEdited {
                    Text(store.openIssues.contains { $0.blockKey == block.key }
                         ? "你改过 · 来源有更新，见 issue" : "你改过")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.accent)
                        .lineLimit(1)
                }
            }
        }
        .padding(.leading, block.userEdited ? AeroricSpace.m : 0)
        .overlay(alignment: .leading) {
            if block.userEdited {
                Rectangle().fill(AeroricColor.accent).frame(width: 2)
            }
        }
    }

    // MARK: Sheets

    private var editSheet: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("编辑 · \(store.page?.title ?? "")").font(.system(size: AeroricType.title, weight: .semibold))
            Text("第一行 # 是标题，## 是小节。改过的标题和小节以后重新编译也不会被覆盖，来源有更新时会开 issue。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            TextEditor(text: $draft)
                .font(AeroricType.mono(AeroricType.code))
                .scrollIndicators(.never)
                .scrollContentBackground(.hidden)
                .padding(AeroricSpace.m)
                .frame(height: 420)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                .accessibilityIdentifier("wiki.editor")
            HStack {
                Spacer()
                Button("取消") { editing = false }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("保存") {
                    editing = false
                    let text = draft
                    Task { await store.save(text) }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("wiki.save")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 720)
    }

    private var issuesSheet: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("Wiki issue").font(.system(size: AeroricType.title, weight: .semibold))
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.m) {
                    if store.issues.isEmpty {
                        Text("没有 issue。").foregroundStyle(AeroricColor.textTertiary)
                    }
                    ForEach(store.issues) { issue in
                        VStack(alignment: .leading, spacing: AeroricSpace.s) {
                            HStack(spacing: AeroricSpace.s) {
                                CheckpointBadge(text: WikiText.issueKind(issue.kind),
                                                tint: issue.open ? AeroricColor.warning : AeroricColor.textSecondary)
                                Text(issue.slug).font(.system(size: AeroricType.caption).monospaced())
                                    .foregroundStyle(AeroricColor.textTertiary)
                                Spacer()
                                if issue.open {
                                    if issue.suggestion != nil {
                                        Button("采纳建议") { Task { await store.resolve(issue, accept: true) } }
                                            .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                                    }
                                    Button("关闭") { Task { await store.resolve(issue, accept: false) } }
                                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                                } else {
                                    Text("已处理").font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                }
                            }
                            Text(issue.text).font(.system(size: AeroricType.secondary))
                            if let suggestion = issue.suggestion {
                                Text(suggestion)
                                    .font(AeroricType.mono(AeroricType.code))
                                    .foregroundStyle(AeroricColor.textSecondary)
                                    .padding(AeroricSpace.m)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                            }
                        }
                        .opacity(issue.open ? 1 : 0.6)
                    }
                }
            }
            .scrollIndicators(.never)
            .frame(height: 380)
            HStack { Spacer(); Button("完成") { showIssues = false }.buttonStyle(AeroricCapsuleButtonStyle(.primary)) }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 620)
    }

    private var exportSheet: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("导出到仓库").font(.system(size: AeroricType.title, weight: .semibold))
            Text("把所有页面写成 Markdown 到项目里的这个目录，不会删除那里已有的其他文件。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            CapsuleField(systemImage: "folder", placeholder: "docs/wiki", text: $exportDir)
                .onChange(of: exportDir) { _, _ in exportFiles = nil; store.exportError = nil }
            if let why = store.exportError {
                Label(why, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(why)
                    .accessibilityIdentifier("wiki.export.error")
            }
            if let plan = exportFiles {
                Text(WikiText.exportLine(plan))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(plan.overwrites.isEmpty ? AeroricColor.textSecondary : AeroricColor.warning)
                    .lineLimit(1)
                    .help(plan.overwrites.isEmpty ? plan.files.joined(separator: "\n") : plan.overwrites.joined(separator: "\n"))
            }
            HStack {
                Spacer()
                Button("取消") { showExport = false; exportFiles = nil; store.exportError = nil }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                if exportFiles == nil {
                    Button("预览要写的文件") {
                        Task { exportFiles = await store.exportPlan(exportDir) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                } else {
                    Button("确认导出") {
                        showExport = false
                        exportFiles = nil
                        let dir = exportDir
                        Task { await store.export(dir) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .accessibilityIdentifier("wiki.export.confirm")
                }
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520)
    }
}

