import AeroricClient
import SwiftUI

/// A change the file tree asks the App to make (2026-10-30). Removing goes
/// to the Trash; the page offers no permanent delete.
public enum FileEdit: Equatable, Sendable {
    case create(parent: String, name: String, directory: Bool)
    case rename(path: String, newName: String)
    case trash(path: String)

    /// The folder whose listing changes.
    public var parent: String {
        switch self {
        case let .create(parent, _, _): parent
        case let .rename(path, _), let .trash(path): FileEditText.parent(of: path)
        }
    }
}

/// The file tree's words and paths, kept out of the view so they can be tested.
public enum FileEditText {
    public static func child(of parent: String, name: String) -> String {
        parent.isEmpty ? name : "\(parent)/\(name)"
    }

    public static func parent(of path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return "" }
        return String(path[..<slash])
    }

    /// Linked folders are read-only here: they belong to other folders on the Mac.
    public static func writable(_ path: String) -> Bool {
        !path.hasPrefix(LinkedFolderPath.prefix)
    }

    public static func formTitle(_ operation: FilesOperation, parentName: String) -> String {
        let place = parentName.isEmpty ? "项目根目录" : parentName
        switch operation {
        case .createFile: return "在 \(place) 里新建文件"
        case .createDirectory: return "在 \(place) 里新建文件夹"
        case let .rename(path): return "重命名「\(name(of: path))」"
        case .delete: return ""
        }
    }

    public static func name(of path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return path }
        return String(path[path.index(after: slash)...])
    }
}

/// Files page tree (2026-10-30, design approved): capsule rows indented by
/// depth; on hover a "…" menu to create inside a folder, rename, or move to
/// the Trash (confirmed); ＋文件 / ＋文件夹 create at the project root.
public struct FileBrowserView: View {
    let store: FileTreeStore
    let title: String
    public var onExpand: ((_ path: String) -> Void)?
    /// A file row was clicked (directories toggle instead).
    public var onOpen: ((_ path: String) -> Void)?
    /// Nil keeps the tree read-only.
    public var onEdit: ((FileEdit) -> Void)?

    @State private var naming: FilesOperation?
    @State private var name = ""
    @State private var trashing: FileTreeStore.Node?

    public init(store: FileTreeStore, title: String = "", onExpand: ((_ path: String) -> Void)? = nil,
                onOpen: ((_ path: String) -> Void)? = nil, onEdit: ((FileEdit) -> Void)? = nil) {
        self.store = store
        self.title = title
        self.onExpand = onExpand
        self.onOpen = onOpen
        self.onEdit = onEdit
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            header
            if let error = store.actionError {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "exclamationmark.triangle")
                    Text(error).lineLimit(1)
                    Spacer(minLength: 0)
                    Button { store.actionError = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                        .buttonStyle(.plain).accessibilityLabel("关闭")
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.danger)
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 26)
                .background(AeroricColor.danger.opacity(0.12), in: Capsule(style: .continuous))
                .help(error)
            }
            if let naming { nameForm(naming) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 1) {
                    rows(path: "", depth: 0)
                }
                .padding(.vertical, AeroricSpace.xs)
            }
            .scrollIndicators(.hidden)
        }
        .padding(AeroricSpace.m)
        .alert("把「\(trashing?.name ?? "")」移到废纸篓？", isPresented: Binding(get: { trashing != nil }, set: { if !$0 { trashing = nil } }),
               presenting: trashing) { node in
            Button("移到废纸篓", role: .destructive) { onEdit?(.trash(path: node.path)) }
            Button("取消", role: .cancel) {}
        } message: { node in
            Text(node.isDir ? "文件夹和里面的内容可以在访达的废纸篓里找回。" : "可以在访达的废纸篓里找回。")
        }
        .accessibilityIdentifier("files.browser")
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.s) {
            Text(title).font(.system(size: AeroricType.secondary, weight: .semibold))
                .foregroundStyle(AeroricColor.textSecondary).lineLimit(1)
            Spacer(minLength: AeroricSpace.s)
            if onEdit != nil {
                Button { begin(.createFile(path: "")) } label: { Label("文件", systemImage: "doc.badge.plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    .help("在项目根目录新建文件")
                Button { begin(.createDirectory(path: "")) } label: { Label("文件夹", systemImage: "folder.badge.plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    .help("在项目根目录新建文件夹")
            }
        }
    }

    private func begin(_ operation: FilesOperation) {
        if case let .rename(path) = operation { name = FileEditText.name(of: path) } else { name = "" }
        naming = operation
    }

    private func nameForm(_ operation: FilesOperation) -> some View {
        let parentName: String = {
            switch operation {
            case let .createFile(path), let .createDirectory(path): FileEditText.name(of: path)
            default: ""
            }
        }()
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(FileEditText.formTitle(operation, parentName: parentName))
                .font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary).lineLimit(1)
            CapsuleField(placeholder: operation == .createDirectory(path: parentPath(operation)) ? "文件夹名" : "文件名",
                         text: $name)
                .onSubmit { submit(operation) }
            HStack(spacing: AeroricSpace.s) {
                Spacer()
                Button("取消") { naming = nil }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Button(isRename(operation) ? "重命名" : "创建") { submit(operation) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(AeroricSpace.m)
        .background(AeroricColor.layer2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(AeroricColor.separator2))
    }

    private func parentPath(_ operation: FilesOperation) -> String {
        switch operation {
        case let .createFile(path), let .createDirectory(path), let .rename(path), let .delete(path): path
        }
    }

    private func isRename(_ operation: FilesOperation) -> Bool {
        if case .rename = operation { return true }
        return false
    }

    private func submit(_ operation: FilesOperation) {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        switch operation {
        case let .createFile(parent): onEdit?(.create(parent: parent, name: trimmed, directory: false))
        case let .createDirectory(parent): onEdit?(.create(parent: parent, name: trimmed, directory: true))
        case let .rename(path): onEdit?(.rename(path: path, newName: trimmed))
        case .delete: break
        }
        naming = nil
        name = ""
    }

    private func rows(path: String, depth: Int) -> AnyView {
        if let nodes = store.visibleChildren(of: path) {
            return AnyView(ForEach(nodes) { node in
                FileRow(node: node, depth: depth,
                        isExpanded: store.expandedPaths.contains(node.path),
                        isLoading: store.isLoading(node.path),
                        editable: onEdit != nil && FileEditText.writable(node.path),
                        onToggle: { if store.toggleExpanded(node.path) { onExpand?(node.path) } },
                        onOpen: { onOpen?(node.path) },
                        onOperation: { operation in
                            if case .delete = operation { trashing = node } else { begin(operation) }
                        })
                if store.expandedPaths.contains(node.path) {
                    rows(path: node.path, depth: depth + 1)
                }
            })
        }
        if store.isLoading(path) {
            return AnyView(ProgressView().controlSize(.small).padding(.leading, CGFloat(depth) * 14 + 24))
        }
        return AnyView(EmptyView())
    }
}

/// One row of the tree: chevron for folders, icon, name; a "…" menu on hover.
public struct FileRow: View {
    let node: FileTreeStore.Node
    let depth: Int
    let isExpanded: Bool
    let isLoading: Bool
    let editable: Bool
    let onToggle: () -> Void
    let onOpen: () -> Void
    let onOperation: (FilesOperation) -> Void
    @State private var hovered = false

    public var body: some View {
        HStack(spacing: AeroricSpace.s) {
            Group {
                if node.isDir {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                } else {
                    Color.clear
                }
            }
            .frame(width: 10)
            Image(systemName: node.isDir ? "folder" : (node.isSymlink ? "link" : "doc"))
                .foregroundStyle(node.isDir ? AeroricColor.accent : AeroricColor.textSecondary)
                .frame(width: 16)
            Text(node.name).lineLimit(1).truncationMode(.middle)
            if isLoading { ProgressView().controlSize(.mini) }
            Spacer(minLength: 0)
            if editable && hovered {
                Menu {
                    if node.isDir {
                        Button("新建文件…", systemImage: "doc.badge.plus") { onOperation(.createFile(path: node.path)) }
                        Button("新建文件夹…", systemImage: "folder.badge.plus") { onOperation(.createDirectory(path: node.path)) }
                        Divider()
                    }
                    Button("重命名…", systemImage: "pencil") { onOperation(.rename(path: node.path)) }
                    Button("移到废纸篓…", systemImage: "trash", role: .destructive) { onOperation(.delete(path: node.path)) }
                } label: {
                    Image(systemName: "ellipsis").frame(width: 20, height: 20)
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .accessibilityIdentifier("files.menu.\(node.path)")
            }
        }
        .font(.system(size: AeroricType.secondary))
        .padding(.leading, CGFloat(depth) * 14 + AeroricSpace.s)
        .padding(.trailing, AeroricSpace.s)
        .frame(height: 28)
        .background { if hovered { Capsule(style: .continuous).fill(AeroricColor.hover) } }
        .contentShape(Capsule(style: .continuous))
        .onTapGesture { if node.isDir { onToggle() } else { onOpen() } }
        .onHover { hovered = $0 }
        .help(node.path)
        .accessibilityIdentifier("files.row.\(node.path)")
    }
}

/// 文件操作（新建/重命名/删除三分；删除进废纸篓——UI 不提供永久删除入口）。
public enum FilesOperation: Equatable {
    case createFile(path: String)
    case createDirectory(path: String)
    case rename(path: String)
    case delete(path: String)
}

/// 操作表单（Action 4：FileOperationSheet——输入名＋提交；无效名由宿主拒绝，
/// 客户端不做路径校验替代）。
public struct FileOperationSheet: View {
    let operation: FilesOperation
    public var onSubmit: (_ parentPath: String, _ name: String) -> Void
    public var onCancel: () -> Void
    @State private var name = ""

    public init(
        operation: FilesOperation,
        onSubmit: @escaping (_ parentPath: String, _ name: String) -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.operation = operation
        self.onSubmit = onSubmit
        self.onCancel = onCancel
    }

    private var parentPath: String {
        switch operation {
        case let .createFile(path), let .createDirectory(path): path
        case let .rename(path), let .delete(path): path
        }
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.headline)
            TextField("名称", text: $name)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("files.operation.name")
            HStack {
                Button("确定") { onSubmit(parentPath, name) }
                    .disabled(name.isEmpty)
                    .accessibilityIdentifier("files.operation.submit")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("files.operation.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 320)
        .accessibilityIdentifier("files.operation.sheet")
    }

    private var title: String {
        switch operation {
        case .createFile: "新建文件"
        case .createDirectory: "新建文件夹"
        case .rename: "重命名"
        case .delete: "删除（进废纸篓）"
        }
    }
}

/// 搜索面板（Action 4：搜索结果支持 cursor——结果分页由宿主 limit 承载，
/// 客户端只展示与触发下一页）。
public struct SearchPanel: View {
    public struct Match: Identifiable {
        public var id: String { "\(path):\(line)" }
        public var path: String
        public var line: UInt64
        public var excerpt: String

        public init(path: String, line: UInt64, excerpt: String) {
            self.path = path
            self.line = line
            self.excerpt = excerpt
        }
    }

    let matches: [Match]
    public var onQuery: ((_ query: String) -> Void)?
    @State private var query = ""

    public init(matches: [Match], onQuery: ((_ query: String) -> Void)? = nil) {
        self.matches = matches
        self.onQuery = onQuery
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("搜索…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { onQuery?(query) }
                    .accessibilityIdentifier("files.search.query")
                Button("搜索") { onQuery?(query) }
                    .accessibilityIdentifier("files.search.submit")
            }
            .padding(8)
            if matches.isEmpty {
                // AP8 2026-10-13: 与连接面板同一缺陷——空结果与"渲染失败"在视觉上
                // 无法区分。`Files` 工作区给 SearchPanel 传的是硬编码的 `matches: []`
                // （见 TaskWorkspaceView），所以这里**总是**空的，必须说清是"还没有结果"。
                VStack(spacing: 8) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 24))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Text("没有搜索结果")
                        .font(.callout)
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text("输入关键词后按回车或点「搜索」")
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("files.search.empty")
            } else {
                List(matches) { match in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(match.path):\(match.line)")
                            .font(.caption)
                            .foregroundStyle(AeroricColor.textTertiary)
                        Text(match.excerpt)
                            .font(.caption.monospaced())
                            .lineLimit(2)
                    }
                    .accessibilityIdentifier("files.search.result.\(match.id)")
                }
                .accessibilityIdentifier("files.search.results")
            }
        }
        // No panel-level identifier here: a container identifier **replaces**
        // every descendant identifier in the AX tree, so `files.search.query`,
        // `files.search.empty` and `files.search.results` would all read as the
        // container id and none would be addressable. Measured 2026-10-14;
        // same rule as `WorkspaceDetail` / `BootstrapInfoBlock` / `DebugPanel`.
        // Identifiers live on the leaves only. See G51.
    }
}

/// 替换预览单（Action 4：替换先 preview，再提交匹配版本的改动——stale
/// 区间由宿主拒绝覆盖，UI 显示 skip 计数）。
public struct ReplacePreviewSheet: View {
    public let preview: FilesClient.ReplacePreview
    public var onApply: () -> Void
    public var onCancel: () -> Void

    public init(preview: FilesClient.ReplacePreview, onApply: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.preview = preview
        self.onApply = onApply
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("替换预览：\(preview.query) → \(preview.replacement)")
                .font(.headline)
            List(preview.files, id: \.path) { file in
                VStack(alignment: .leading, spacing: 2) {
                    Text(file.path)
                        .font(.callout)
                    Text("\(file.matches.count) 处命中")
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .accessibilityIdentifier("files.replace.preview.\(file.path)")
            }
            HStack {
                Button("应用替换") { onApply() }
                    .accessibilityIdentifier("files.replace.apply")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("files.replace.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 420, minHeight: 300)
        .accessibilityIdentifier("files.replace.sheet")
    }
}
