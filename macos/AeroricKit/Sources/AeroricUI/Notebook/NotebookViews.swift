import SwiftUI

/// P7-05 Action 2：随手记十视图。视图只承载布局与回调，宿主调用
/// （NotebookClient）由 App 层接线注入；store 持簿记与纯逻辑。
///
/// 复用约束：正文编辑用 [`NativeTextEditor`]（真实 NSTextView/TextKit 桥）、
/// 预览用 [`MarkdownPreview`]（原生明文渲染，永不走 HTML）——与文件编辑器
/// 同一套 document/Markdown 基础组件，不另造第二份。

// MARK: - 1. NotebookSidebar

/// 侧栏：vault 注册表＋笔记树（懒加载展开）。新建/删除经回调交宿主命令；
/// 删除是软删（进 `<vault>/.notebook/trash/`），本面板没有永久删除入口。
public struct NotebookSidebar: View {
    @Bindable var store: NotebookStore
    public var onExpand: ((_ path: String) -> Void)?
    public var onNewNote: (() -> Void)?
    public var onNewFolder: (() -> Void)?
    public var onOpenTrash: (() -> Void)?
    public var onSelect: ((_ path: String, _ isDir: Bool) -> Void)?
    @State private var selection: String?

    public init(
        store: NotebookStore,
        onExpand: ((_ path: String) -> Void)? = nil,
        onNewNote: (() -> Void)? = nil,
        onNewFolder: (() -> Void)? = nil,
        onOpenTrash: (() -> Void)? = nil,
        onSelect: ((_ path: String, _ isDir: Bool) -> Void)? = nil
    ) {
        self.store = store
        self.onExpand = onExpand
        self.onNewNote = onNewNote
        self.onNewFolder = onNewFolder
        self.onOpenTrash = onOpenTrash
        self.onSelect = onSelect
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("随手记").font(.headline)
                Spacer()
                Menu {
                    Button("新建笔记") { onNewNote?() }
                    Button("新建文件夹") { onNewFolder?() }
                    Divider()
                    Button("回收站…") { onOpenTrash?() }
                } label: {
                    Image(systemName: "plus")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .accessibilityLabel("随手记操作")
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)

            List {
                ForEach(store.tree) { entry in
                    SidebarBranch(
                        entry: entry, onExpand: onExpand, onSelect: onSelect)
                }
            }
            .listStyle(.plain)
            .accessibilityIdentifier("notebook.sidebar")
        }
    }
}

/// 顶层分支（懒加载：首次展开触发 onExpand，由 App 层拉子目录）。
private struct SidebarBranch: View {
    let entry: NotebookStore.TreeEntry
    let onExpand: ((_ path: String) -> Void)?
    let onSelect: ((_ path: String, _ isDir: Bool) -> Void)?
    @State private var expanded = false

    var body: some View {
        if entry.isDir {
            DisclosureGroup(isExpanded: $expanded) {
                if let children = entry.children {
                    ForEach(children) { child in
                        SidebarBranch(
                            entry: child, onExpand: onExpand, onSelect: onSelect)
                    }
                }
            } label: {
                row
            }
            .onChange(of: expanded) { _, isOn in
                if isOn { onExpand?(entry.path) }
            }
        } else {
            row
        }
    }

    private var row: some View {
        HStack(spacing: 4) {
            Image(systemName: entry.isDir ? "folder" : "doc.text")
                .foregroundStyle(entry.isDir ? Color.accentColor : .secondary)
            Text(entry.name)
                .lineLimit(1)
                .help(entry.truncated ? "子级被截断（目录深度/数量上限）" : entry.name)
            if entry.truncated {
                Image(systemName: "exclamationmark.triangle")
                    .font(.caption2)
                    .foregroundStyle(.orange)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if entry.isDir {
                expanded = true
                onExpand?(entry.path)
            } else {
                onSelect?(entry.path, false)
            }
        }
    }
}

// MARK: - 2. NoteDocumentView

/// 笔记正文：真实 TextKit 编辑器＋Markdown 明文预览＋**冲突弹窗**。
/// 保存走宿主 `notebook.save_note`（expectedRevision 基线）；冲突时不静默
/// 覆盖——弹窗给「覆盖（force）/放弃/取消」三个显式选项。
public struct NoteDocumentView: View {
    @Bindable var store: NotebookStore
    public var path: String
    @Binding public var content: String
    public var isRichText: Bool
    public var onSave: ((_ force: Bool) -> Void)?
    public var onOverwriteAs: (() -> Void)?
    public var onDiscard: (() -> Void)?
    public var onTogglePreview: (() -> Void)?
    @Binding public var showsPreview: Bool
    @State private var pendingConflict: NotebookStore.FileSig?

    public init(
        store: NotebookStore,
        path: String,
        content: Binding<String>,
        isRichText: Bool = false,
        showsPreview: Binding<Bool> = .constant(false),
        onSave: ((_ force: Bool) -> Void)? = nil,
        onOverwriteAs: (() -> Void)? = nil,
        onDiscard: (() -> Void)? = nil,
        onTogglePreview: (() -> Void)? = nil
    ) {
        self.store = store
        self.path = path
        self._content = content
        self.isRichText = isRichText
        self._showsPreview = showsPreview
        self.onSave = onSave
        self.onOverwriteAs = onOverwriteAs
        self.onDiscard = onDiscard
        self.onTogglePreview = onTogglePreview
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text((path as NSString).lastPathComponent)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                if store.dirtyPaths.contains(path) {
                    Circle().fill(.orange).frame(width: 8, height: 8)
                        .accessibilityLabel("有未保存修改")
                }
                Spacer()
                Button {
                    onTogglePreview?()
                } label: {
                    Image(systemName: showsPreview ? "pencil.line" : "eye")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel(showsPreview ? "切换到编辑" : "切换到预览")
                Button("保存") { onSave?(false) }
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(!store.dirtyPaths.contains(path))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)

            if isRichText && !showsPreview {
                // richtext 迁移前：富文本笔记只读展示 + 预览，不在原生里伪造 WYSIWYG。
                MarkdownPreview(markdown: content)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if showsPreview {
                MarkdownPreview(markdown: content)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                NativeTextEditor(text: $content)
                    .onChange(of: content) { _, _ in store.recordDirty(path: path) }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .sheet(item: $pendingConflict) { disk in
            // 复用文件编辑器的冲突弹窗：覆盖 / 另存 / 放弃 / 取消，没有默认覆盖。
            SaveConflictSheet(
                path: path,
                onOverwrite: {
                    pendingConflict = nil
                    onSave?(true)
                },
                onSaveAs: {
                    pendingConflict = nil
                    onOverwriteAs?()
                },
                onDiscard: {
                    pendingConflict = nil
                    onDiscard?()
                },
                onCancel: { pendingConflict = nil }
            )
        }
    }

    /// 宿主保存回执的接入口：`.conflict` 弹窗，`.saved` 由调用方调
    /// `store.applySaved`。供 App 层在 onSave 回调里使用。
    public func resolve(outcome: NotebookClientSaveOutcome) {
        switch outcome {
        case let .saved(sig):
            store.applySaved(path: path, sig: sig)
        case let .conflict(disk):
            pendingConflict = disk
        }
    }
}

/// App 层保存回执（与 NotebookClient.saveNote 的返回一一对应）。
public enum NotebookClientSaveOutcome: Equatable {
    case saved(NotebookStore.FileSig)
    case conflict(disk: NotebookStore.FileSig)
}

extension NotebookStore.FileSig: Identifiable {
    public var id: String { "\(mtimeMs)-\(hash)" }
}

// MARK: - 3. NoteMetadataEditor

/// 属性面板：磁盘元数据（symlink_metadata，不跟软链）＋ frontmatter 字段。
public struct NoteMetadataEditor: View {
    public var stat: NotebookStat?
    public var fields: [String: [String]]

    public init(stat: NotebookStat?, fields: [String: [String]]) {
        self.stat = stat
        self.fields = fields
    }

    public var body: some View {
        Form {
            if let stat {
                LabeledContent("大小", value: ByteCountFormatter.string(fromByteCount: Int64(stat.size), countStyle: .file))
                LabeledContent("修改于", value: formatDate(stat.modifiedMs))
                if let created = stat.createdMs {
                    LabeledContent("创建于", value: formatDate(created))
                }
            }
            ForEach(fields.keys.sorted(), id: \.self) { key in
                LabeledContent(key, value: fields[key]?.joined(separator: ", ") ?? "")
            }
        }
        .padding()
        .accessibilityIdentifier("notebook.metadata")
    }

    private func formatDate(_ ms: Int64) -> String {
        Date(msSinceEpoch: ms).formatted(date: .abbreviated, time: .shortened)
    }
}

public struct NotebookStat: Equatable, Sendable {
    public var size: UInt64
    public var modifiedMs: Int64
    public var createdMs: Int64?

    public init(size: UInt64, modifiedMs: Int64, createdMs: Int64?) {
        self.size = size
        self.modifiedMs = modifiedMs
        self.createdMs = createdMs
    }
}

extension Date {
    init(msSinceEpoch ms: Int64) {
        self.init(timeIntervalSince1970: TimeInterval(ms) / 1000)
    }
}

// MARK: - 4. BacklinksPanel

/// 反链面板：扫全库的 `[[wikilink]]` 出现（原始 body + 行号 + 行预览），
/// 解析规则（stem/标题/路径优先级）在前端一份——这里只呈现。
public struct BacklinksPanel: View {
    public struct Backlink: Identifiable, Equatable {
        public var path: String
        public var line: UInt32
        public var preview: String
        public var embed: Bool

        public var id: String { "\(path)#\(line)" }

        public init(path: String, line: UInt32, preview: String, embed: Bool) {
            self.path = path
            self.line = line
            self.preview = preview
            self.embed = embed
        }
    }

    public var links: [Backlink]
    public var onOpen: ((_ path: String) -> Void)?

    public init(links: [Backlink], onOpen: ((_ path: String) -> Void)? = nil) {
        self.links = links
        self.onOpen = onOpen
    }

    public var body: some View {
        Group {
            if links.isEmpty {
                ContentUnavailableView("没有反向链接", systemImage: "link.badge.plus")
            } else {
                List(links) { link in
                    Button {
                        onOpen?(link.path)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(link.preview).font(.callout).lineLimit(2)
                            Text(link.embed ? "嵌入 · 第 \(link.line) 行" : "第 \(link.line) 行")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .buttonStyle(.plain)
                }
                .listStyle(.plain)
            }
        }
        .accessibilityIdentifier("notebook.backlinks")
    }
}

// MARK: - 5. TagBrowser

/// 标签浏览器：聚合（哪个标签几处、分布哪几篇）＋跨文件改写。
/// 改写按扫描器给的字节区间在宿主完成；这里显示 old→new 确认。
public struct TagBrowser: View {
    @Bindable var store: NotebookStore
    public var onSelectTag: ((_ key: String) -> Void)?
    public var onRename: ((_ old: String, _ new: String) -> Void)?
    @State private var renameTarget: NotebookStore.TagSummary?
    @State private var newTag = ""

    public init(
        store: NotebookStore,
        onSelectTag: ((_ key: String) -> Void)? = nil,
        onRename: ((_ old: String, _ new: String) -> Void)? = nil
    ) {
        self.store = store
        self.onSelectTag = onSelectTag
        self.onRename = onRename
    }

    public var body: some View {
        List {
            ForEach(store.tags) { tag in
                HStack {
                    VStack(alignment: .leading) {
                        Text("#\(tag.sample)").font(.callout)
                        Text("\(tag.count) 处 · \(tag.files.count) 篇")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("改名") {
                        newTag = tag.sample
                        renameTarget = tag
                    }
                    .buttonStyle(.borderless)
                }
                .contentShape(Rectangle())
                .onTapGesture { onSelectTag?(tag.key) }
            }
        }
        .listStyle(.plain)
        .accessibilityIdentifier("notebook.tags")
        .sheet(item: $renameTarget) { tag in
            VStack(spacing: 12) {
                Text("把 #\(tag.sample) 改名").font(.headline)
                TextField("新标签（不含 #）", text: $newTag)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 220)
                HStack {
                    Button("取消") { renameTarget = nil }
                        .keyboardShortcut(.cancelAction)
                    Button("改写全部 \(tag.count) 处") {
                        onRename?(tag.sample, newTag)
                        renameTarget = nil
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(newTag.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .padding(20)
            .frame(minWidth: 320)
        }
    }
}

// MARK: - 6. NoteTaskInbox

/// 任务收件箱：全库 `- [ ]` 任务（行号按整个 .md 文件数，与标签/反链同一
/// 坐标系）。勾选翻转经宿主保存走基线冲突链——不绕过 expectedRevision。
public struct NoteTaskInbox: View {
    @Bindable var store: NotebookStore
    public var onToggle: ((_ entry: NotebookStore.TaskEntry) -> Void)?
    public var onOpen: ((_ path: String) -> Void)?

    public init(
        store: NotebookStore,
        onToggle: ((_ entry: NotebookStore.TaskEntry) -> Void)? = nil,
        onOpen: ((_ path: String) -> Void)? = nil
    ) {
        self.store = store
        self.onToggle = onToggle
        self.onOpen = onOpen
    }

    public var body: some View {
        List {
            ForEach(store.tasks) { task in
                HStack(alignment: .top) {
                    Toggle(
                        task.checked ? "已完成" : "未完成",
                        isOn: Binding(
                            get: { task.checked },
                            set: { _ in onToggle?(task) }
                        )
                    )
                    .toggleStyle(.checkbox)
                    .labelsHidden()
                    VStack(alignment: .leading) {
                        Text(task.text)
                            .strikethrough(task.checked)
                        Button(task.path) { onOpen?(task.path) }
                            .buttonStyle(.link)
                            .font(.caption)
                    }
                }
            }
        }
        .listStyle(.plain)
        .accessibilityIdentifier("notebook.tasks")
    }
}

// MARK: - 7. TemplatePicker

/// 自定义模板选择器：占位符（`{{date}}`/`{{time}}`）按本地时区在客户端展开，
/// `{{title}}` 留到标题定稿。
public struct TemplatePicker: View {
    @Bindable var store: NotebookStore
    public var onUse: ((_ template: NotebookStore.Template, _ rendered: String, _ defaultName: String) -> Void)?

    public init(
        store: NotebookStore,
        onUse: ((_ template: NotebookStore.Template, _ rendered: String, _ defaultName: String) -> Void)? = nil
    ) {
        self.store = store
        self.onUse = onUse
    }

    public var body: some View {
        List(store.templates) { template in
            Button {
                // 占位符在前端展开：日期要按用户本地时区算。
                let body = NotebookStore.fillDateTime(template.body)
                let name = NotebookStore.fillDateTime(template.name)
                onUse?(template, body, name)
            } label: {
                VStack(alignment: .leading) {
                    Text(template.title).font(.callout)
                    Text(template.name).font(.caption).foregroundStyle(.secondary)
                }
            }
            .buttonStyle(.plain)
        }
        .listStyle(.plain)
        .overlay {
            if store.templates.isEmpty {
                ContentUnavailableView(
                    "没有自定义模板", systemImage: "doc.on.doc",
                    description: Text("在 vault 的 .notebook/templates/ 下放 .md 文件"))
            }
        }
        .accessibilityIdentifier("notebook.templates")
    }
}

// MARK: - 8. AttachmentView

/// 附件面板：列出 vault 附件（认得出的类型），图片预览读原始字节。
public struct AttachmentView: View {
    public struct Entry: Identifiable, Equatable {
        public var path: String
        public var name: String
        public var relativePath: String
        public var size: UInt64
        public var kind: String

        public var id: String { path }

        public init(path: String, name: String, relativePath: String, size: UInt64, kind: String) {
            self.path = path
            self.name = name
            self.relativePath = relativePath
            self.size = size
            self.kind = kind
        }
    }

    public var entries: [Entry]
    public var onAdd: (() -> Void)?
    public var onPreview: ((_ entry: Entry) -> Void)?

    public init(
        entries: [Entry],
        onAdd: (() -> Void)? = nil,
        onPreview: ((_ entry: Entry) -> Void)? = nil
    ) {
        self.entries = entries
        self.onAdd = onAdd
        self.onPreview = onPreview
    }

    public var body: some View {
        List {
            ForEach(entries) { entry in
                Button {
                    onPreview?(entry)
                } label: {
                    HStack {
                        Image(systemName: iconName(entry.kind))
                        VStack(alignment: .leading) {
                            Text(entry.name).lineLimit(1)
                            Text(ByteCountFormatter.string(fromByteCount: Int64(entry.size), countStyle: .file))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .buttonStyle(.plain)
            }
        }
        .listStyle(.plain)
        .toolbar {
            Button(action: { onAdd?() }) {
                Image(systemName: "plus")
            }
            .accessibilityLabel("添加附件")
        }
        .accessibilityIdentifier("notebook.attachments")
    }

    private func iconName(_ kind: String) -> String {
        switch kind {
        case "image", "svg": "photo"
        case "pdf": "doc.richtext"
        case "video": "film"
        case "audio": "waveform"
        case "archive": "doc.zipper"
        default: "doc"
        }
    }
}

// MARK: - 9. TrashView

/// 回收站：恢复 / 彻底删除（二段确认）/ 清空（二段确认）。彻底删除进系统
/// 回收站并清历史快照——不可撤销，所以每次都要显式确认。
public struct TrashView: View {
    @Bindable var store: NotebookStore
    public var onRestore: ((_ entry: NotebookStore.TrashEntry) -> Void)?
    public var onPurge: ((_ entry: NotebookStore.TrashEntry) -> Void)?
    public var onPurgeAll: (() -> Void)?
    @State private var purgeTarget: NotebookStore.TrashEntry?
    @State private var confirmPurgeAll = false

    public init(
        store: NotebookStore,
        onRestore: ((_ entry: NotebookStore.TrashEntry) -> Void)? = nil,
        onPurge: ((_ entry: NotebookStore.TrashEntry) -> Void)? = nil,
        onPurgeAll: (() -> Void)? = nil
    ) {
        self.store = store
        self.onRestore = onRestore
        self.onPurge = onPurge
        self.onPurgeAll = onPurgeAll
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("回收站（\(store.trash.count)）").font(.headline)
                Spacer()
                Button("清空…") { confirmPurgeAll = true }
                    .disabled(store.trash.isEmpty)
            }
            .padding(10)
            List {
                ForEach(store.trash) { entry in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(entry.name).lineLimit(1)
                            Text("原位置：\(entry.relativePath)")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("恢复") { onRestore?(entry) }
                            .buttonStyle(.borderless)
                        Button("彻底删除…") { purgeTarget = entry }
                            .buttonStyle(.borderless)
                    }
                }
            }
            .listStyle(.plain)
        }
        .accessibilityIdentifier("notebook.trash")
        .confirmationDialog(
            "彻底删除「\(purgeTarget?.name ?? "")」？",
            isPresented: Binding(get: { purgeTarget != nil }, set: { if !$0 { purgeTarget = nil } }),
            titleVisibility: .visible
        ) {
            Button("彻底删除（载荷进系统废纸篓）", role: .destructive) {
                if let entry = purgeTarget { onPurge?(entry) }
                purgeTarget = nil
            }
            Button("取消", role: .cancel) { purgeTarget = nil }
        } message: {
            Text("历史快照一并删除，恢复键将失效。")
        }
        .confirmationDialog(
            "清空回收站（\(store.trash.count) 条）？",
            isPresented: $confirmPurgeAll,
            titleVisibility: .visible
        ) {
            Button("全部彻底删除", role: .destructive) { onPurgeAll?() }
            Button("取消", role: .cancel) { }
        } message: {
            Text("每条都走单条彻底删除的路径，不可撤销。")
        }
    }
}

// MARK: - 10. ImportExportWizard

/// 导入/导出向导：七家导入格式 + 单文档/整库导出。导入失败展示**部分报告**
/// （已落地的部分不清掉），成功也显示报告笔记落点。
public struct ImportExportWizard: View {
    @Bindable var store: NotebookStore
    public var providers: [String]
    @State private var provider = "obsidian"
    @State private var sourcePath = ""
    public var onImport: ((_ provider: String, _ sourcePath: String) -> Void)?
    public var onExportFile: ((_ path: String, _ content: String) -> Void)?
    @State private var exportPath = ""
    @State private var exportContent = ""

    public init(
        store: NotebookStore,
        providers: [String] = ["obsidian", "logseq", "notion", "bear", "roam", "evernote", "apple-notes"],
        onImport: ((_ provider: String, _ sourcePath: String) -> Void)? = nil,
        onExportFile: ((_ path: String, _ content: String) -> Void)? = nil
    ) {
        self.store = store
        self.providers = providers
        self.onImport = onImport
        self.onExportFile = onExportFile
    }

    public var body: some View {
        Form {
            Section("导入") {
                Picker("来源", selection: $provider) {
                    ForEach(providers, id: \.self) { Text($0).tag($0) }
                }
                TextField("源目录 / 归档路径", text: $sourcePath)
                Button("开始导入") { onImport?(provider, sourcePath) }
                    .disabled(sourcePath.trimmingCharacters(in: .whitespaces).isEmpty)
                if let report = store.lastImport {
                    ImportReportView(report: report)
                }
            }
            Section("导出单文档") {
                TextField("目标路径（vault 内或 桌面/文档/下载）", text: $exportPath)
                TextField("内容（Markdown）", text: $exportContent, axis: .vertical)
                    .lineLimit(3...6)
                Button("导出") { onExportFile?(exportPath, exportContent) }
                    .disabled(
                        exportPath.trimmingCharacters(in: .whitespaces).isEmpty
                            || exportContent.isEmpty)
                Text("写入位置由服务端白名单限制（vault 内、桌面、文档、下载）。")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding()
        .frame(minWidth: 420)
        .accessibilityIdentifier("notebook.importexport")
    }
}

struct ImportReportView: View {
    let report: NotebookStore.ImportSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("「\(report.provider)」导入完成").font(.callout)
            HStack(spacing: 12) {
                Label("\(report.imported)", systemImage: "checkmark.circle").foregroundStyle(.green)
                Label("\(report.skipped)", systemImage: "arrow.uturn.right").foregroundStyle(.secondary)
                Label("\(report.failed)", systemImage: "xmark.circle").foregroundStyle(
                    report.failed > 0 ? .red : .secondary)
            }
            .font(.caption)
            if let reportPath = report.reportPath {
                Text("报告笔记：\(reportPath)")
                    .font(.caption).foregroundStyle(.secondary)
            } else if report.failed > 0 {
                Text("失败明细见导入报告（本轮仍有一部分已落地，未被丢弃）。")
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        .padding(8)
        .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
    }
}
