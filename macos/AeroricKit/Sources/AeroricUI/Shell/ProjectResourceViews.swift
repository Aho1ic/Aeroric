import AeroricClient
import QuickLook
import SwiftUI

/// The session panel's file tree (M3; writes 2026-10-30). Selecting a file
/// or double-clicking forwards one typed target to the centre preview. New
/// file / new folder in the header, the rest from the context menu; names
/// are typed in place. Linked folders are listed after the project's own
/// entries and stay read-only.
public struct ProjectResourceInspector: View {
    let book: ProjectResourceBook
    let projectID: String
    let projectName: String
    let onOpen: (ResourcePreviewTarget) -> Void
    let onLoadDirectory: (String) -> Void
    let onRefresh: () -> Void
    /// Applies a create / rename / move to the Trash; nil on success, else
    /// the reason to show. Nil keeps the tree read-only.
    let onEdit: ((FileEdit) async -> String?)?
    /// The project's folder on this Mac; nil for remote projects, which have
    /// no local file for Quick Look.
    let localRoot: String?
    /// Space shows the system Quick Look panel for the selected file.
    @State private var quickLookURL: URL?
    @State private var naming: FileNaming?
    @State private var nameText = ""
    @State private var trashing: FilesClient.FileEntry?
    @State private var editError: String?
    @State private var submitting = false
    @FocusState private var nameFocused: Bool

    public init(book: ProjectResourceBook, projectID: String, projectName: String,
                onOpen: @escaping (ResourcePreviewTarget) -> Void,
                onLoadDirectory: @escaping (String) -> Void, onRefresh: @escaping () -> Void,
                onEdit: ((FileEdit) async -> String?)? = nil,
                localRoot: String? = nil) {
        self.book = book; self.projectID = projectID; self.projectName = projectName
        self.onOpen = onOpen; self.onLoadDirectory = onLoadDirectory; self.onRefresh = onRefresh
        self.onEdit = onEdit; self.localRoot = localRoot
    }

    /// The local file behind a tree path: inside the project, or inside a
    /// linked folder. Nil for remote projects.
    private func localURL(_ path: String?) -> URL? {
        guard let path else { return nil }
        if let absolute = LinkedFolderPath.absolute(path) { return URL(fileURLWithPath: absolute) }
        guard let localRoot else { return nil }
        return URL(fileURLWithPath: localRoot).appendingPathComponent(path)
    }

    /// Space toggles Quick Look for `path`; ignored for remote projects.
    private func toggleQuickLook(_ path: String?) -> KeyPress.Result {
        if quickLookURL != nil { quickLookURL = nil; return .handled }
        guard let url = localURL(path) else { return .ignored }
        quickLookURL = url
        return .handled
    }

    private var scene: ProjectResourceScene { book.scene(projectID) }
    /// 2026-10-27: the tree reads as a compact outline (was 26pt plus the
    /// sidebar list's own medium-row padding, which spread files far apart).
    static var rowHeight: CGFloat { 22 }
    static var indent: CGFloat { 14 }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if let editError { errorLine(editError) }
            HairlineDivider()
            fileList
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .alert("把「\(trashing?.name ?? "")」移到废纸篓？",
               isPresented: Binding(get: { trashing != nil }, set: { if !$0 { trashing = nil } }),
               presenting: trashing) { node in
            Button("移到废纸篓", role: .destructive) { submit(.trash(path: node.path)) }
            Button("取消", role: .cancel) {}
        } message: { node in
            Text(node.isDir ? "文件夹和里面的内容可以在访达的废纸篓里找回。" : "可以在访达的废纸篓里找回。")
        }
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: "folder").foregroundStyle(AeroricColor.textTertiary)
            Text(projectName)
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .lineLimit(1).truncationMode(.middle)
                .help(projectName)
            Spacer(minLength: AeroricSpace.xs)
            if onEdit != nil {
                NativeIconButton(systemImage: "doc.badge.plus", label: "新建文件",
                                 identifier: "resources.new-file") { begin(.create(parent: newItemParent, directory: false)) }
                NativeIconButton(systemImage: "folder.badge.plus", label: "新建文件夹",
                                 identifier: "resources.new-folder") { begin(.create(parent: newItemParent, directory: true)) }
            }
            NativeIconButton(systemImage: "arrow.clockwise", label: "刷新\(projectName)资源",
                             identifier: "resources.refresh", action: onRefresh)
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 36)
    }

    /// Header buttons create next to the selected file, else at the root.
    private var newItemParent: String {
        guard let selected = scene.listSelection else { return "" }
        let parent = FileEditText.parent(of: selected)
        return FileEditText.writable(parent) ? parent : ""
    }

    private func errorLine(_ error: String) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: "exclamationmark.triangle")
            Text(error).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: AeroricSpace.xs)
            Button { editError = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                .buttonStyle(.plain)
                .accessibilityLabel("关闭")
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.danger)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 26)
        .background(AeroricColor.danger.opacity(0.12), in: Capsule(style: .continuous))
        .padding(.horizontal, AeroricSpace.s)
        .padding(.bottom, AeroricSpace.xs)
        .help(error)
        .accessibilityIdentifier("resources.error")
    }

    // MARK: - Tree

    private struct TreeRow: Identifiable {
        var id: String { node.path }
        let node: FilesClient.FileEntry
        let depth: Int
    }
    private var rows: [TreeRow] {
        func children(_ path: String, depth: Int) -> [TreeRow] {
            (scene.directories[path] ?? []).flatMap { node in
                [TreeRow(node: node, depth: depth)]
                    + (node.isDir && scene.expandedPaths.contains(node.path) ? children(node.path, depth: depth + 1) : [])
            }
        }
        return children("", depth: 0)
    }
    /// Single click only selects (2026-10-26 C); double click or Return opens
    /// a tab through the list's primary action. Folders carry a tag only so
    /// the context menu knows which row was clicked; they never stay selected.
    private var fileSelection: Binding<String?> {
        Binding(get: { scene.listSelection }, set: { path in
            guard let path, rows.contains(where: { $0.id == path && !$0.node.isDir }) else { return }
            book.edit(projectID) { $0.listSelection = path }
            // Quick Look follows the selection while its panel is open.
            if quickLookURL != nil { quickLookURL = localURL(path) }
        })
    }
    private var fileURLs: [URL] {
        rows.filter { !$0.node.isDir }.compactMap { localURL($0.id) }
    }
    private var fileList: some View {
        VStack(alignment: .leading, spacing: 0) {
            readNotice(scene.directoryStates[""] ?? .notLoaded, hasCache: scene.directories[""] != nil)
            if scene.directoryStates[""] == .loaded, scene.directories[""]?.isEmpty == true, naming == nil {
                AeroricEmptyState(systemImage: "folder", title: "目录为空")
            }
            NativeSelectionList(selection: fileSelection) {
                if case let .create(parent, directory) = naming, parent.isEmpty {
                    nameField(placeholder: directory ? "文件夹名" : "文件名", depth: 0)
                        .listRowInsets(insets)
                }
                ForEach(rows) { row in
                    if row.node.isDir { folderRow(row) } else { fileRow(row) }
                }
                .listRowSeparator(.hidden)
            }
            .contextMenu(forSelectionType: String?.self, menu: { paths in
                menu(for: paths.compactMap { $0 }.first)
            }, primaryAction: { paths in
                guard let path = paths.compactMap({ $0 }).first,
                      rows.contains(where: { $0.id == path && !$0.node.isDir }) else { return }
                onOpen(.init(projectID: projectID, path: path, kind: .file))
            })
            .onKeyPress(.space) { toggleQuickLook(scene.listSelection) }
            .quickLookPreview($quickLookURL, in: fileURLs)
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
            .scrollIndicators(.hidden)
            .environment(\.defaultMinListRowHeight, Self.rowHeight)
            .environment(\.sidebarRowSize, .small)
            .accessibilityIdentifier("resources.files")
        }
    }

    private func folderRow(_ row: TreeRow) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            if naming == .rename(path: row.id) {
                nameField(placeholder: "文件夹名", depth: 0)
            } else {
                Button {
                    let expanded = scene.expandedPaths.contains(row.id)
                    book.edit(projectID) {
                        if expanded { $0.expandedPaths.remove(row.id) } else { $0.expandedPaths.insert(row.id) }
                    }
                    if !expanded, scene.directories[row.id] == nil { onLoadDirectory(row.id) }
                } label: {
                    HStack(spacing: AeroricSpace.xs) {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .rotationEffect(.degrees(scene.expandedPaths.contains(row.id) ? 90 : 0))
                            .frame(width: 10)
                        resourceName(row.node.name, symbol: row.node.isSymlink ? "link" : "folder.fill",
                                     tint: AeroricColor.accent.opacity(0.75), folder: true)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .help(row.node.path)
                .accessibilityLabel("\(row.node.name)，目录")
            }
            if case let .create(parent, directory) = naming, parent == row.id {
                nameField(placeholder: directory ? "文件夹名" : "文件名", depth: 1)
            }
            if scene.expandedPaths.contains(row.id) {
                if let status = scene.directoryStates[row.id], status != .loaded {
                    readNotice(status, hasCache: scene.directories[row.id] != nil)
                    Button("重试此目录") { onLoadDirectory(row.id) }.buttonStyle(.link)
                } else if scene.directories[row.id]?.isEmpty == true {
                    Text("空目录")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .padding(.leading, Self.indent + 14)
                        .frame(height: Self.rowHeight)
                }
            }
        }
        .modifier(TreeIndent(depth: row.depth, indent: Self.indent))
        .tag(Optional(row.id))
        .listRowInsets(insets)
    }

    @ViewBuilder private func fileRow(_ row: TreeRow) -> some View {
        Group {
            if naming == .rename(path: row.id) {
                nameField(placeholder: "文件名", depth: 0)
            } else {
                resourceName(row.node.name,
                             symbol: row.node.isSymlink ? "link" : FileSymbol.symbol(forFile: row.node.name),
                             tint: AeroricColor.textTertiary)
                    .padding(.leading, 14)
                    .help(row.node.path)
                    .accessibilityLabel("\(row.node.name)，\(row.node.path)")
            }
        }
        .modifier(TreeIndent(depth: row.depth, indent: Self.indent))
        .tag(Optional(row.id))
        .listRowInsets(insets)
    }

    // MARK: - Writes

    @ViewBuilder private func menu(for path: String?) -> some View {
        let node = path.flatMap { path in rows.first(where: { $0.id == path })?.node }
        let folder = node.map { $0.isDir ? $0.path : FileEditText.parent(of: $0.path) } ?? ""
        let canWrite = onEdit != nil && FileEditText.writable(folder)
        if canWrite {
            Button("新建文件", systemImage: "doc.badge.plus") { begin(.create(parent: folder, directory: false)) }
            Button("新建文件夹", systemImage: "folder.badge.plus") { begin(.create(parent: folder, directory: true)) }
        }
        if let node {
            let writable = onEdit != nil && FileEditText.writable(node.path)
            if writable {
                Button("重命名", systemImage: "pencil") { begin(.rename(path: node.path)) }
            }
            Divider()
            if !node.isDir, let url = localURL(node.path) {
                Button("快速查看", systemImage: "eye") { quickLookURL = url }
            }
            if let url = localURL(node.path) {
                Button("在访达中显示", systemImage: "folder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            }
            Button("复制路径", systemImage: "doc.on.doc") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(localURL(node.path)?.path ?? node.path, forType: .string)
            }
            if writable {
                Divider()
                Button("移到废纸篓…", systemImage: "trash", role: .destructive) { trashing = node }
            }
        }
    }

    private func begin(_ next: FileNaming) {
        editError = nil
        switch next {
        case let .rename(path): nameText = FileEditText.name(of: path)
        case let .create(parent, _):
            nameText = ""
            if !parent.isEmpty, !scene.expandedPaths.contains(parent) {
                book.edit(projectID) { _ = $0.expandedPaths.insert(parent) }
                if scene.directories[parent] == nil { onLoadDirectory(parent) }
            }
        }
        naming = next
    }

    private func nameField(placeholder: String, depth: Int) -> some View {
        CapsuleField(placeholder: placeholder, text: $nameText, height: Self.rowHeight)
            .focused($nameFocused)
            .onSubmit(commitName)
            .onExitCommand { naming = nil }
            .disabled(submitting)
            .padding(.leading, CGFloat(depth) * Self.indent + 10)
            .onAppear { DispatchQueue.main.async { nameFocused = true } }
            .accessibilityIdentifier("resources.name-field")
    }

    private func commitName() {
        let name = nameText.trimmingCharacters(in: .whitespaces)
        guard let naming, !name.isEmpty else { self.naming = nil; return }
        switch naming {
        case let .create(parent, directory): submit(.create(parent: parent, name: name, directory: directory))
        case let .rename(path):
            guard name != FileEditText.name(of: path) else { self.naming = nil; return }
            submit(.rename(path: path, newName: name))
        }
    }

    private func submit(_ edit: FileEdit) {
        guard let onEdit else { return }
        submitting = true
        Task { @MainActor in
            let error = await onEdit(edit)
            submitting = false
            editError = error
            if error == nil { naming = nil }
        }
    }

    // MARK: - Shared

    private func resourceName(_ name: String, symbol: String, tint: Color, folder: Bool = false) -> some View {
        HStack(spacing: AeroricSpace.s - 2) {
            Image(systemName: symbol)
                .font(.system(size: 11))
                .frame(width: 14)
                .foregroundStyle(tint)
            Text(name)
                .font(.system(size: AeroricType.secondary, weight: folder ? .medium : .regular))
                .foregroundStyle(folder ? AeroricColor.text : AeroricColor.textSecondary)
                .lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .frame(height: Self.rowHeight)
    }
    /// Depth is drawn by `TreeIndent` inside the row, so every row starts at
    /// the same inset and the guides line up.
    private var insets: EdgeInsets {
        EdgeInsets(top: 0, leading: AeroricSpace.xs, bottom: 0, trailing: AeroricSpace.s)
    }
    @ViewBuilder private func readNotice(_ status: ResourceReadStatus, hasCache: Bool) -> some View {
        if let notice = status.notice {
            Text(notice + (hasCache ? "（保留缓存，版本未知）" : ""))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s)
        }
    }
}

/// What the tree is asking a name for.
enum FileNaming: Equatable {
    case create(parent: String, directory: Bool)
    case rename(path: String)
}

public struct CentralResourcePreview: View {
    let book: ProjectResourceBook
    let target: ResourcePreviewTarget
    let projectName: String
    let projectAvailable: Bool
    let onRetry: () -> Void
    let onOpenFile: () -> Void
    /// Saves a `.file` tab's text; nil keeps the tab read-only.
    let onSave: ((String) -> Void)?
    public init(book: ProjectResourceBook, target: ResourcePreviewTarget, projectName: String,
                projectAvailable: Bool, onRetry: @escaping () -> Void, onOpenFile: @escaping () -> Void,
                onSave: ((String) -> Void)? = nil) {
        self.book = book; self.target = target; self.projectName = projectName
        self.projectAvailable = projectAvailable; self.onRetry = onRetry; self.onOpenFile = onOpenFile
        self.onSave = onSave
    }
    private var scene: ProjectResourceScene { book.scene(target.projectID) }
    /// A loaded text file with a save path is edited in place (2026-10-26 C).
    private var editable: Bool {
        target.kind == .file && onSave != nil && scene.previewState == .loaded && scene.previewText != nil
    }
    private var draft: Binding<String> {
        Binding(get: { scene.drafts[target] ?? scene.previewText ?? "" },
                set: { book.setDraft($0, for: target) })
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text("\(projectName) · \(editable ? "文件" : target.kind.label)")
                    Spacer()
                    if editable, book.isDirty(target) {
                        Text("已修改").foregroundStyle(AeroricColor.textTertiary)
                        Button("保存") { onSave?(draft.wrappedValue) }
                            .keyboardShortcut("s")
                            .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                            .fixedSize()
                            .accessibilityIdentifier("preview.save")
                    }
                    NativeIconButton(systemImage: "arrow.clockwise", label: "重新读取此预览",
                                     identifier: "preview.retry", action: onRetry)
                        .disabled(!projectAvailable)
                }
                Text(target.path).font(AeroricType.mono(AeroricType.secondary)).textSelection(.enabled)
                Text(editable ? "可编辑 · ⌘S 保存" : "只读 · 宿主快照 · 版本未知")
                    .foregroundStyle(AeroricColor.textTertiary)
                if target.kind != .file {
                    Text("只读 · 宿主完整 diff（超过 1 MiB 会明确失败，不截断）")
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            .font(AeroricType.mono(AeroricType.caption))
            .padding(12)
            Divider()
            if !projectAvailable {
                PaneEmptyState(systemImage: "folder.badge.questionmark", title: "原项目不可用",
                               hint: "保留原预览身份，不会选择其他项目代替。")
            } else {
                if let notice = scene.previewState.notice {
                    Text(notice + (scene.previewText == nil ? "" : "（以下为缓存，版本未知）"))
                        .font(AeroricType.mono(AeroricType.caption)).textSelection(.enabled).padding(12)
                }
                if scene.previewState == .loading { ProgressView().controlSize(.small).padding(12) }
                if let text = scene.previewText {
                    if text.isEmpty {
                        Text(target.kind == .file ? "空文件" : "此文件没有可显示的文本 diff")
                            .font(AeroricType.mono(AeroricType.caption)).padding(12)
                    }
                    if editable {
                        TextEditor(text: draft)
                            .font(AeroricType.mono(AeroricType.code))
                            .scrollContentBackground(.hidden)
                            .padding(8)
                            .accessibilityIdentifier("preview.editor")
                    } else {
                        sourceLines
                    }
                } else {
                    Spacer(minLength: 0)
                }
                if target.kind == .untracked {
                    Button("只读查看原文件", action: onOpenFile).buttonStyle(.borderless).padding(12)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(AeroricColor.layer1)
        .accessibilityIdentifier("workspace.central-preview")
    }
    private var sourceLines: some View {
        ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(scene.previewLines) { line in
                    HStack(alignment: .top, spacing: 10) {
                        if target.kind != .file {
                            lineNumber(line.oldNumber)
                        }
                        lineNumber(line.newNumber)
                        Text(verbatim: line.text.isEmpty ? " " : line.text)
                            .font(AeroricType.mono(AeroricType.code))
                            .foregroundStyle(line.kind == .added ? AeroricColor.success
                                             : line.kind == .removed ? AeroricColor.danger : AeroricColor.text)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: true, vertical: true)
                    }
                    .frame(minHeight: 18, alignment: .topLeading)
                    .id(line.id)
                }
            }
            .scrollTargetLayout()
            .padding(12)
        }
        .scrollPosition(id: Binding(
            get: { scene.positions[target] },
            set: { book.rememberPosition($0, for: target) }
        ))
    }
    private func lineNumber(_ number: Int?) -> some View {
        Text(number.map(String.init) ?? "")
            .font(AeroricType.mono(AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            .frame(minWidth: 32, alignment: .trailing)
            .accessibilityHidden(true)
    }
}

/// Outline indentation with faint guide lines, one per ancestor directory
/// (VS Code / Finder list style), so deep trees stay readable at 22pt rows.
private struct TreeIndent: ViewModifier {
    let depth: Int
    let indent: CGFloat

    func body(content: Content) -> some View {
        HStack(spacing: 0) {
            ForEach(0..<depth, id: \.self) { _ in
                Rectangle()
                    .fill(AeroricColor.separator1)
                    .frame(width: 1)
                    .frame(width: indent, alignment: .center)
                    .frame(maxHeight: .infinity)
            }
            content
        }
    }
}
